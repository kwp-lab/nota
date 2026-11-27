//! Plans and writes lossless segments of a Nota Ogg Opus recording for
//! providers that limit the duration of one uploaded file.
//!
//! Positions are Ogg granule positions at 48 kHz, so they include the stream's
//! `OpusHead` pre-skip. Cuts always fall on packet boundaries, and segments are
//! remuxed without re-encoding.

use anyhow::{Context, Result, bail};
use ogg::{PacketReader, PacketWriteEndInfo, PacketWriter};
use opus::{Channels, Decoder};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Write};
use std::path::Path;

const OPUS_RATE: u64 = 48_000;
/// Decoder pre-roll carried before a cut and discarded through pre-skip.
const PRE_ROLL_SAMPLES: u64 = 3_840;
/// Packets decoded before a search window so the decoder state has settled.
const DECODER_WARMUP_PACKETS: usize = 25;
const PACKETS_PER_PAGE: usize = 50;

/// Bounds for one planned segment, in milliseconds of audible audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentLimits {
    /// Longest segment the planner aims for before searching for a pause.
    pub target_ms: u64,
    /// How far either side of a target cut the planner searches for a pause.
    pub search_window_ms: u64,
    /// Hard upper bound for any segment, checked after cuts move.
    pub max_segment_ms: u64,
    /// Shortest pause that counts as a safe cut.
    pub min_silence_ms: u64,
}

/// Limits that keep every DashScope segment under its two-hour diarization limit.
pub const DASHSCOPE_SEGMENT_LIMITS: SegmentLimits = SegmentLimits {
    target_ms: 105 * 60 * 1_000,
    search_window_ms: 5 * 60 * 1_000,
    max_segment_ms: 115 * 60 * 1_000,
    min_silence_ms: 400,
};

/// One planned segment. Sample positions are granule positions on packet
/// boundaries; millisecond positions are on the recording's audible timeline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlannedSegment {
    pub index: u32,
    pub start_sample: u64,
    pub end_sample: u64,
    pub start_ms: u64,
    pub end_ms: u64,
}

impl PlannedSegment {
    pub fn duration_ms(&self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }
}

/// Number of segments a recording of `duration_ms` is split into.
pub fn segment_count(duration_ms: u64, limits: SegmentLimits) -> u32 {
    duration_ms
        .div_ceil(limits.target_ms.max(1))
        .clamp(1, u32::MAX as u64) as u32
}

struct AudioPacket {
    start: u64,
    samples: u64,
}

struct StreamIndex {
    pre_skip: u64,
    packets: Vec<AudioPacket>,
    /// Final granule position, which may trim the last packet.
    end_granule: u64,
}

impl StreamIndex {
    fn stream_end(&self) -> u64 {
        self.packets
            .last()
            .map(|packet| packet.start + packet.samples)
            .unwrap_or(0)
    }

    fn audible_end(&self) -> u64 {
        self.end_granule.min(self.stream_end())
    }
}

fn samples_to_ms(samples: u64) -> u64 {
    samples.saturating_mul(1_000) / OPUS_RATE
}

fn ms_to_samples(ms: u64) -> u64 {
    ms.saturating_mul(OPUS_RATE) / 1_000
}

fn open_packets(path: &Path) -> Result<PacketReader<BufReader<File>>> {
    let file = File::open(path).with_context(|| format!("无法打开录音文件 {}", path.display()))?;
    Ok(PacketReader::new(BufReader::new(file)))
}

fn read_pre_skip(head: &[u8]) -> Result<u64> {
    if !head.starts_with(b"OpusHead") || head.len() < 19 {
        bail!("录音不是有效的 Ogg Opus 文件");
    }
    if head[9] != 1 {
        bail!("分段转写仅支持单声道 Nota 录音");
    }
    Ok(u16::from_le_bytes([head[10], head[11]]) as u64)
}

fn index_stream(path: &Path) -> Result<StreamIndex> {
    let mut reader = open_packets(path)?;
    let mut pre_skip = None;
    let mut packets = Vec::new();
    let mut position = 0u64;
    let mut end_granule = 0u64;
    let mut header_packets = 0;
    while let Some(packet) = reader.read_packet().context("无法读取 Ogg 页")? {
        if header_packets == 0 {
            pre_skip = Some(read_pre_skip(&packet.data)?);
            header_packets += 1;
            continue;
        }
        if header_packets == 1 {
            if !packet.data.starts_with(b"OpusTags") {
                bail!("录音缺少 OpusTags");
            }
            header_packets += 1;
            continue;
        }
        let samples = opus::packet::get_nb_samples(&packet.data, OPUS_RATE as u32)
            .context("Opus 数据包无法解析")? as u64;
        packets.push(AudioPacket {
            start: position,
            samples,
        });
        position += samples;
        if packet.last_in_page() {
            end_granule = packet.absgp_page();
        }
    }
    let pre_skip = pre_skip.context("录音缺少 OpusHead")?;
    if packets.is_empty() {
        bail!("录音没有音频数据");
    }
    Ok(StreamIndex {
        pre_skip,
        packets,
        end_granule: if end_granule == 0 {
            position
        } else {
            end_granule
        },
    })
}

/// Per-packet RMS for packets whose index is in `range`, decoding a short
/// warm-up before it. Indices outside the range are not decoded.
fn packet_rms(path: &Path, range: std::ops::Range<usize>) -> Result<Vec<f32>> {
    let mut reader = open_packets(path)?;
    let mut decoder = Decoder::new(OPUS_RATE as u32, Channels::Mono)?;
    let decode_from = range.start.saturating_sub(DECODER_WARMUP_PACKETS);
    let mut rms = Vec::with_capacity(range.len());
    let mut buffer = vec![0.0f32; 5_760];
    let mut index = 0usize;
    let mut headers = 0;
    while let Some(packet) = reader.read_packet().context("无法读取 Ogg 页")? {
        if headers < 2 {
            headers += 1;
            continue;
        }
        if index >= range.end {
            break;
        }
        if index >= decode_from {
            let count = decoder
                .decode_float(&packet.data, &mut buffer, false)
                .context("Opus 音频解码失败")?;
            if index >= range.start {
                let energy: f32 = buffer[..count].iter().map(|sample| sample * sample).sum();
                rms.push(if count == 0 {
                    0.0
                } else {
                    (energy / count as f32).sqrt()
                });
            }
        }
        index += 1;
    }
    Ok(rms)
}

/// Chooses a packet-boundary cut inside the longest pause of a window.
///
/// `packets` are `(start_sample, sample_count, rms)` in stream order. Returns
/// the boundary sample position. Falls back to the boundary nearest `target`
/// when no pause is at least `min_silence_samples` long.
fn choose_cut(packets: &[(u64, u64, f32)], target: u64, min_silence_samples: u64) -> u64 {
    let Some(first) = packets.first() else {
        return target;
    };
    let mut sorted: Vec<f32> = packets.iter().map(|packet| packet.2).collect();
    sorted.sort_by(f32::total_cmp);
    let noise_floor = sorted[sorted.len() / 10];
    // Adapt to background noise, but never treat speech-level energy as silence.
    let threshold = (noise_floor * 2.0).clamp(0.001, 0.02);

    let mut best: Option<(u64, u64)> = None; // (run samples, cut position)
    let mut run_start: Option<usize> = None;
    let consider = |start: usize, end: usize, best: &mut Option<(u64, u64)>| {
        let run_samples: u64 = packets[start..end].iter().map(|packet| packet.1).sum();
        if run_samples < min_silence_samples {
            return;
        }
        let midpoint = packets[start].0 + run_samples / 2;
        // Cut at the packet boundary inside the run that is nearest its midpoint.
        let cut = (start..=end)
            .map(|index| {
                packets
                    .get(index)
                    .map(|packet| packet.0)
                    .unwrap_or_else(|| packets[end - 1].0 + packets[end - 1].1)
            })
            .min_by_key(|boundary| boundary.abs_diff(midpoint))
            .unwrap_or(midpoint);
        let better = match *best {
            None => true,
            Some((length, position)) => {
                run_samples > length
                    || (run_samples == length && cut.abs_diff(target) < position.abs_diff(target))
            }
        };
        if better {
            *best = Some((run_samples, cut));
        }
    };
    for (index, packet) in packets.iter().enumerate() {
        if packet.2 <= threshold {
            run_start.get_or_insert(index);
        } else if let Some(start) = run_start.take() {
            consider(start, index, &mut best);
        }
    }
    if let Some(start) = run_start {
        consider(start, packets.len(), &mut best);
    }
    if let Some((_, cut)) = best {
        return cut;
    }
    packets
        .iter()
        .map(|packet| packet.0)
        .filter(|boundary| *boundary > first.0)
        .min_by_key(|boundary| boundary.abs_diff(target))
        .unwrap_or(first.0)
}

/// Plans segments for `path`. Every segment is at most `limits.max_segment_ms`;
/// the plan fails before any upload if that cannot be met.
pub fn plan_segments(path: &Path, limits: SegmentLimits) -> Result<Vec<PlannedSegment>> {
    let index = index_stream(path)?;
    let audible_samples = index.audible_end().saturating_sub(index.pre_skip);
    let duration_ms = samples_to_ms(audible_samples);
    let count = segment_count(duration_ms, limits) as u64;
    let window = ms_to_samples(limits.search_window_ms);
    let min_silence = ms_to_samples(limits.min_silence_ms);

    let mut cuts = Vec::with_capacity(count as usize + 1);
    cuts.push(0u64);
    for k in 1..count {
        let target = index.pre_skip + audible_samples * k / count;
        let low = target.saturating_sub(window);
        let high = target.saturating_add(window);
        let first = index.packets.partition_point(|packet| packet.start < low);
        let last = index.packets.partition_point(|packet| packet.start <= high);
        let first = first.max(1);
        if first >= last {
            bail!("录音过短，无法规划分段");
        }
        let rms = packet_rms(path, first..last)?;
        let candidates: Vec<(u64, u64, f32)> = index.packets[first..last]
            .iter()
            .zip(rms)
            .map(|(packet, rms)| (packet.start, packet.samples, rms))
            .collect();
        cuts.push(choose_cut(&candidates, target, min_silence));
    }
    cuts.push(index.audible_end());

    let to_timeline = |position: u64| samples_to_ms(position.saturating_sub(index.pre_skip));
    let mut plan = Vec::with_capacity(count as usize);
    for (k, pair) in cuts.windows(2).enumerate() {
        let segment = PlannedSegment {
            index: k as u32,
            start_sample: pair[0],
            end_sample: if k as u64 + 1 == count {
                index.stream_end()
            } else {
                pair[1]
            },
            start_ms: to_timeline(pair[0]),
            end_ms: to_timeline(pair[1]),
        };
        if segment.end_ms <= segment.start_ms {
            bail!("分段规划失败：第 {} 段为空", k + 1);
        }
        if segment.duration_ms() > limits.max_segment_ms {
            bail!(
                "分段规划失败：第 {} 段超过 {} 分钟上限",
                k + 1,
                limits.max_segment_ms / 60_000
            );
        }
        plan.push(segment);
    }
    Ok(plan)
}

fn patched_head(head: &[u8], pre_skip: u64) -> Result<Vec<u8>> {
    let pre_skip = u16::try_from(pre_skip).context("分段预滚过长")?;
    let mut head = head.to_vec();
    head[10..12].copy_from_slice(&pre_skip.to_le_bytes());
    Ok(head)
}

/// Writes one planned segment of `source` to a new file at `destination`.
///
/// A segment after the first carries at least 80 ms of earlier packets as
/// decoder pre-roll; its `OpusHead` pre-skip discards them so the first
/// audible sample is the planned start.
pub fn write_segment(source: &Path, segment: &PlannedSegment, destination: &Path) -> Result<()> {
    let mut reader = open_packets(source)?;
    let head = reader
        .read_packet()
        .context("无法读取 Ogg 页")?
        .context("录音缺少 OpusHead")?
        .data;
    let original_pre_skip = read_pre_skip(&head)?;
    let tags = reader
        .read_packet()
        .context("无法读取 Ogg 页")?
        .context("录音缺少 OpusTags")?
        .data;

    // Keep only packets from the pre-roll base onward, so memory stays bounded
    // by one segment instead of the whole recording.
    let preroll_floor = segment.start_sample.saturating_sub(PRE_ROLL_SAMPLES);
    let mut audio: std::collections::VecDeque<(u64, u64, Vec<u8>)> =
        std::collections::VecDeque::new();
    let mut position = 0u64;
    let mut end_granule = 0u64;
    let mut is_last = true;
    while let Some(packet) = reader.read_packet().context("无法读取 Ogg 页")? {
        let samples = opus::packet::get_nb_samples(&packet.data, OPUS_RATE as u32)
            .context("Opus 数据包无法解析")? as u64;
        if packet.last_in_page() {
            end_granule = packet.absgp_page();
        }
        let start = position;
        position += samples;
        if start + samples > segment.end_sample {
            if start >= segment.end_sample {
                is_last = false;
                break;
            }
            bail!("分段终点没有落在数据包边界");
        }
        audio.push_back((start, samples, packet.data));
        while segment.index > 0 && audio.len() > 1 && audio[1].0 <= preroll_floor {
            audio.pop_front();
        }
    }

    let (base, pre_skip) = if segment.index == 0 {
        (0, original_pre_skip)
    } else {
        let base = audio
            .iter()
            .map(|packet| packet.0)
            .filter(|start| *start <= preroll_floor)
            .max()
            .unwrap_or(0);
        if !audio.iter().any(|packet| packet.0 == segment.start_sample) {
            bail!("分段起点没有落在数据包边界");
        }
        (base, segment.start_sample - base)
    };
    let packets: Vec<_> = audio
        .into_iter()
        .filter(|packet| packet.0 >= base)
        .collect();
    if packets.is_empty() {
        bail!("分段没有音频数据");
    }

    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(destination)
        .with_context(|| format!("无法创建分段文件 {}", destination.display()))?;
    let serial = rand::rng().random::<u32>();
    let mut writer = PacketWriter::new(BufWriter::with_capacity(64 * 1024, file));
    writer.write_packet(
        patched_head(&head, pre_skip)?,
        serial,
        PacketWriteEndInfo::EndPage,
        0,
    )?;
    writer.write_packet(tags, serial, PacketWriteEndInfo::EndPage, 0)?;
    let total = packets.len();
    for (offset, (start, samples, data)) in packets.into_iter().enumerate() {
        let final_packet = offset + 1 == total;
        let mut granule = start + samples - base;
        if final_packet && is_last && end_granule > base {
            // Keep the original end trimming of the recording's last packet.
            granule = granule.min(end_granule - base);
        }
        let end_info = if final_packet {
            PacketWriteEndInfo::EndStream
        } else if (offset + 1).is_multiple_of(PACKETS_PER_PAGE) {
            PacketWriteEndInfo::EndPage
        } else {
            PacketWriteEndInfo::NormalPacket
        };
        writer.write_packet(data, serial, end_info, granule)?;
    }
    let mut inner = writer.into_inner();
    inner.flush()?;
    inner.get_ref().sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{OpusOggWriter, SAMPLE_RATE};

    const FRAME: usize = 960;

    /// Writes a mono recording made of `(milliseconds, audible)` parts.
    fn write_recording(path: &Path, parts: &[(u64, bool)]) {
        assert_eq!(SAMPLE_RATE, 48_000);
        let mut writer = OpusOggWriter::create(path).unwrap();
        let mut phase = 0.0f32;
        for (ms, audible) in parts {
            let frames = (*ms as usize * 48) / FRAME;
            for _ in 0..frames {
                let frame: Vec<f32> = (0..FRAME)
                    .map(|_| {
                        phase += 2.0 * std::f32::consts::PI * 440.0 / 48_000.0;
                        if *audible { 0.3 * phase.sin() } else { 0.0 }
                    })
                    .collect();
                writer.push_frame(&frame).unwrap();
            }
        }
        writer.finish().unwrap();
    }

    fn decoded_ms(path: &Path) -> u64 {
        let index = index_stream(path).unwrap();
        samples_to_ms(index.audible_end() - index.pre_skip)
    }

    fn test_dir() -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("nota-segments-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    const LIMITS: SegmentLimits = SegmentLimits {
        target_ms: 10_000,
        search_window_ms: 2_000,
        max_segment_ms: 12_000,
        min_silence_ms: 400,
    };

    #[test]
    fn segment_count_divides_evenly_instead_of_leaving_a_short_tail() {
        let limits = DASHSCOPE_SEGMENT_LIMITS;
        assert_eq!(segment_count(119 * 60_000, limits), 2);
        assert_eq!(segment_count(121 * 60_000, limits), 2);
        assert_eq!(segment_count(210 * 60_000, limits), 2);
        assert_eq!(segment_count(211 * 60_000, limits), 3);
        assert_eq!(segment_count(240 * 60_000, limits), 3);
    }

    #[test]
    fn choose_cut_prefers_the_longest_pause_and_falls_back_to_target() {
        let packet = |index: u64, rms: f32| (index * 960, 960, rms);
        let mut packets: Vec<_> = (0..100).map(|index| packet(index, 0.2)).collect();
        for item in packets.iter_mut().take(30).skip(20) {
            item.2 = 0.0; // 200 ms pause
        }
        for item in packets.iter_mut().take(85).skip(60) {
            item.2 = 0.0; // 500 ms pause
        }
        let cut = choose_cut(&packets, 50 * 960, 19_200);
        assert!((60 * 960..=85 * 960).contains(&cut));
        assert_eq!(cut % 960, 0);

        let speech: Vec<_> = (0..100).map(|index| packet(index, 0.2)).collect();
        assert_eq!(choose_cut(&speech, 50 * 960 + 100, 19_200), 50 * 960);
    }

    #[test]
    fn plans_cuts_inside_pauses_and_writes_contiguous_lossless_segments() {
        let root = test_dir();
        let source = root.join("meeting.ogg");
        // 19 s total, so two segments; a pause sits around the 9.5 s target.
        write_recording(&source, &[(8_600, true), (1_000, false), (9_400, true)]);
        let total_ms = decoded_ms(&source);

        let plan = plan_segments(&source, LIMITS).unwrap();
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0].start_ms, 0);
        assert_eq!(plan[0].end_ms, plan[1].start_ms);
        assert_eq!(plan[1].end_ms, total_ms);
        assert!((8_600..=9_600).contains(&plan[0].end_ms), "{plan:?}");

        let mut written_ms = 0;
        for segment in &plan {
            let path = root.join(format!("segment-{}.ogg", segment.index));
            write_segment(&source, segment, &path).unwrap();
            let length = decoded_ms(&path);
            assert!(
                length.abs_diff(segment.duration_ms()) <= 1,
                "{segment:?} {length}"
            );
            written_ms += length;
            let mut reader =
                crate::asr::OggPcm16Reader::open(&path, segment.duration_ms()).unwrap();
            let samples = reader.read_samples(16_000 * 30).unwrap();
            assert!(!samples.is_empty());
            assert!(
                write_segment(&source, segment, &path).is_err(),
                "create_new"
            );
        }
        assert!(written_ms.abs_diff(total_ms) <= 2);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn later_segments_carry_preroll_discarded_by_pre_skip() {
        let root = test_dir();
        let source = root.join("meeting.ogg");
        write_recording(&source, &[(19_000, true)]);
        let plan = plan_segments(&source, LIMITS).unwrap();
        assert_eq!(plan.len(), 2);
        let path = root.join("second.ogg");
        write_segment(&source, &plan[1], &path).unwrap();
        let index = index_stream(&path).unwrap();
        assert!(index.pre_skip >= PRE_ROLL_SAMPLES);
        assert!(index.pre_skip < PRE_ROLL_SAMPLES + 960);
        assert!(
            samples_to_ms(index.audible_end() - index.pre_skip).abs_diff(plan[1].duration_ms())
                <= 1
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn planning_rejects_segments_over_the_hard_limit() {
        let root = test_dir();
        let source = root.join("meeting.ogg");
        write_recording(&source, &[(9_000, true)]);
        let strict = SegmentLimits {
            target_ms: 10_000,
            search_window_ms: 1_000,
            max_segment_ms: 5_000,
            min_silence_ms: 400,
        };
        assert!(plan_segments(&source, strict).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
