use crate::models::AecMode;
use sonora::config::EchoCanceller;
use sonora::{AudioProcessing, Config, StreamConfig};
use std::collections::VecDeque;

pub const SAMPLE_RATE: u32 = 48_000;
pub const FRAME_SAMPLES: usize = 480;
const MIX_GAIN: f32 = 0.707_945_76;
const LIMIT: f32 = 0.891_250_9;

#[derive(Debug, Clone)]
pub struct AudioPacket {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub timestamp_100ns: u64,
}

struct AdaptiveStream {
    input: VecDeque<f32>,
    source_rate: u32,
    phase: f64,
    ratio_adjustment: f64,
    next_timestamp_100ns: Option<u64>,
}

impl AdaptiveStream {
    fn new() -> Self {
        Self {
            input: VecDeque::with_capacity(4096),
            source_rate: SAMPLE_RATE,
            phase: 0.0,
            ratio_adjustment: 1.0,
            next_timestamp_100ns: None,
        }
    }

    fn push(&mut self, packet: AudioPacket) {
        if packet.sample_rate != 0 {
            self.source_rate = packet.sample_rate;
        }
        let mut samples = packet.samples;
        if let Some(expected) = self.next_timestamp_100ns {
            let delta = packet.timestamp_100ns as i128 - expected as i128;
            // QPC is shared by independent WASAPI clients. Fill short capture
            // discontinuities, but treat long gaps as sleep/resume and reset.
            if delta > 20_000 && delta < 50_000_000 {
                let missing = ((delta as u128 * self.source_rate as u128) / 10_000_000)
                    .min((self.source_rate * 5) as u128) as usize;
                self.input.extend(std::iter::repeat_n(0.0, missing));
            } else if delta < -20_000 {
                let overlap = (((-delta) as u128 * self.source_rate as u128) / 10_000_000) as usize;
                if overlap >= samples.len() {
                    return;
                }
                samples.drain(..overlap);
            }
        }
        let duration = samples.len() as u64 * 10_000_000 / self.source_rate.max(1) as u64;
        self.next_timestamp_100ns = Some(packet.timestamp_100ns.saturating_add(duration));
        self.input.extend(samples);
        let target = (self.source_rate as usize / 10).max(1) * 3;
        let error = self.input.len() as isize - target as isize;
        self.ratio_adjustment =
            (1.0 + (error as f64 / target as f64) * 0.000_25).clamp(0.999, 1.001);
    }

    fn frame(&mut self) -> Vec<f32> {
        let mut output = Vec::with_capacity(FRAME_SAMPLES);
        let step = (self.source_rate as f64 / SAMPLE_RATE as f64) * self.ratio_adjustment;
        for _ in 0..FRAME_SAMPLES {
            let index = self.phase.floor() as usize;
            if index + 1 >= self.input.len() {
                output.push(0.0);
                continue;
            }
            let fraction = (self.phase - index as f64) as f32;
            let first = self.input[index];
            let second = self.input[index + 1];
            output.push(first + (second - first) * fraction);
            self.phase += step;
            let consumed = self.phase.floor() as usize;
            if consumed > 0 {
                for _ in 0..consumed.min(self.input.len()) {
                    self.input.pop_front();
                }
                self.phase -= consumed as f64;
            }
        }
        output
    }

    fn clear(&mut self) {
        self.input.clear();
        self.phase = 0.0;
        self.next_timestamp_100ns = None;
    }
}

pub struct AudioMixer {
    system: AdaptiveStream,
    microphone: AdaptiveStream,
    aec: Option<AudioProcessing>,
}

impl AudioMixer {
    pub fn new(aec_mode: AecMode, auto_should_enable: bool) -> Self {
        let enabled = matches!(aec_mode, AecMode::On)
            || (matches!(aec_mode, AecMode::Auto) && auto_should_enable);
        Self {
            system: AdaptiveStream::new(),
            microphone: AdaptiveStream::new(),
            aec: enabled.then(create_aec),
        }
    }

    pub fn push_system(&mut self, packet: AudioPacket) {
        self.system.push(packet);
    }

    pub fn push_microphone(&mut self, packet: AudioPacket) {
        self.microphone.push(packet);
    }

    pub fn next_frame(
        &mut self,
        system_present: bool,
        microphone_present: bool,
    ) -> (Vec<f32>, f32, f32) {
        let system = self.system.frame();
        let microphone = self.microphone.frame();
        let system_level = rms(&system);
        let microphone_level = rms(&microphone);

        let processed_microphone = if let Some(processor) = self.aec.as_mut() {
            let mut render_output = vec![0.0f32; FRAME_SAMPLES];
            let mut capture_output = vec![0.0f32; FRAME_SAMPLES];
            if processor
                .process_render_f32(&[&system], &mut [&mut render_output])
                .is_ok()
                && processor
                    .process_capture_f32(&[&microphone], &mut [&mut capture_output])
                    .is_ok()
            {
                capture_output
            } else {
                microphone.clone()
            }
        } else {
            microphone.clone()
        };

        let mut mixed = Vec::with_capacity(FRAME_SAMPLES);
        for index in 0..FRAME_SAMPLES {
            let sample = match (system_present, microphone_present) {
                (true, true) => (system[index] + processed_microphone[index]) * MIX_GAIN,
                (true, false) => system[index],
                (false, true) => processed_microphone[index],
                (false, false) => 0.0,
            };
            mixed.push(soft_limit(sample));
        }
        (mixed, system_level, microphone_level)
    }

    pub fn reset(&mut self, aec_mode: AecMode, auto_should_enable: bool) {
        self.system.clear();
        self.microphone.clear();
        let enabled = matches!(aec_mode, AecMode::On)
            || (matches!(aec_mode, AecMode::Auto) && auto_should_enable);
        self.aec = enabled.then(create_aec);
    }
}

fn create_aec() -> AudioProcessing {
    let stream = StreamConfig::new(SAMPLE_RATE, 1);
    AudioProcessing::builder()
        .config(Config {
            echo_canceller: Some(EchoCanceller::default()),
            ..Default::default()
        })
        .capture_config(stream)
        .render_config(stream)
        .build()
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32)
        .sqrt()
        .clamp(0.0, 1.0)
}

fn soft_limit(sample: f32) -> f32 {
    sample.clamp(-LIMIT, LIMIT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiter_never_clips() {
        for value in [-20.0, -2.0, -1.0, 0.0, 1.0, 2.0, 20.0] {
            assert!(soft_limit(value).abs() <= LIMIT);
        }
    }

    #[test]
    fn resampler_handles_clock_drift_without_unbounded_buffering() {
        let mut stream = AdaptiveStream::new();
        for packet in 0..1000 {
            stream.push(AudioPacket {
                samples: vec![0.25; 481],
                sample_rate: SAMPLE_RATE,
                timestamp_100ns: packet * 100_000,
            });
            let output = stream.frame();
            assert_eq!(output.len(), FRAME_SAMPLES);
        }
        assert!(stream.input.len() < 5_000);
    }

    #[test]
    fn qpc_gap_inserts_silence_but_sleep_gap_does_not() {
        let mut stream = AdaptiveStream::new();
        stream.push(AudioPacket {
            samples: vec![1.0; 480],
            sample_rate: SAMPLE_RATE,
            timestamp_100ns: 1_000_000,
        });
        let before_gap = stream.input.len();
        stream.push(AudioPacket {
            samples: vec![1.0; 480],
            sample_rate: SAMPLE_RATE,
            timestamp_100ns: 1_200_000,
        });
        assert!(stream.input.len() > before_gap + 480);

        let mut after_sleep = AdaptiveStream::new();
        after_sleep.push(AudioPacket {
            samples: vec![1.0; 480],
            sample_rate: SAMPLE_RATE,
            timestamp_100ns: 1_000_000,
        });
        after_sleep.push(AudioPacket {
            samples: vec![1.0; 480],
            sample_rate: SAMPLE_RATE,
            timestamp_100ns: 101_000_000,
        });
        assert_eq!(after_sleep.input.len(), 960);
    }
}
