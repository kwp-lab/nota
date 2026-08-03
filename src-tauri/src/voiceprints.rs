use crate::asr::{
    OggPcm16Reader, SpeakerEmbeddingCapabilities, SpeakerSampleAnalysisOutcome,
    analyze_speaker_samples, fetch_speaker_embedding_capabilities, write_pcm16_wav,
};
use crate::models::{
    AsrProviderCredentials, AsrProviderKind, ParticipantProfile, SpeakerIdentificationAssignment,
    SpeakerIdentificationCandidate, SpeakerIdentificationSession, SpeakerSampleStatus,
    TranscriptSegment,
};
use crate::storage::{Storage, VoiceprintEnrollment};
use anyhow::{Context, Result, bail};
use parking_lot::Mutex;
use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

const SAMPLE_RATE: u64 = 16_000;
const TARGET_SAMPLE_SECONDS: u64 = 30;
const MAX_RANGE_SECONDS: u64 = 6;
const FALLBACK_PREVIEW_SECONDS: u64 = 8;
const MIN_CANDIDATE_SPACING_MS: u64 = 10_000;
const MIN_MATCH_SIMILARITY: f32 = 0.78;
const MIN_MATCH_MARGIN: f32 = 0.05;
const MAX_PENDING_SESSIONS: usize = 8;

#[derive(Debug, Clone)]
struct SampleRange {
    start_ms: u64,
    end_ms: u64,
}

#[derive(Debug, Clone)]
struct SpeakerPlan {
    raw_speaker: String,
    fallback_preview: Option<SampleRange>,
    ranges: Vec<SampleRange>,
}

#[derive(Debug, Clone)]
struct ExtractedRange {
    start_ms: u64,
    end_ms: u64,
    samples: Vec<i16>,
}

#[derive(Debug, Clone)]
struct PendingCandidate {
    view: SpeakerIdentificationCandidate,
    embedding_fingerprint: String,
    embedding: Option<Vec<f32>>,
}

#[derive(Debug, Clone)]
struct PendingSession {
    recording_id: String,
    generation: u32,
    candidates: Vec<PendingCandidate>,
}

pub struct VoiceprintManager {
    storage: Arc<Storage>,
    temp_directory: PathBuf,
    sessions: Mutex<HashMap<String, PendingSession>>,
}

impl VoiceprintManager {
    pub fn new(storage: Arc<Storage>) -> Result<Self> {
        let temp_directory = storage.paths().recovery.join("VoiceprintTemp");
        std::fs::create_dir_all(&temp_directory)?;
        clean_temp_directory(&temp_directory)?;
        Ok(Self {
            storage,
            temp_directory,
            sessions: Mutex::new(HashMap::new()),
        })
    }

    pub fn identify(
        &self,
        recording_id: &str,
        provider_id: Option<&str>,
    ) -> Result<SpeakerIdentificationSession> {
        let recording = self.storage.find_recording(recording_id)?;
        if !Path::new(&recording.path).is_file() {
            bail!("录音文件不存在，无法提取声纹");
        }
        let transcript = self.storage.transcript(recording_id)?;
        if transcript.status != crate::models::TranscriptionStatus::Completed {
            bail!("请先完成这场会议的文字转写");
        }
        let credentials = self.resolve_provider(provider_id)?;
        let capabilities = fetch_speaker_embedding_capabilities(&credentials)?;
        let generation = self.storage.transcription_generation(recording_id)?;

        let max_samples_by_bytes = capabilities.max_bytes.saturating_sub(44) / 2;
        let target_samples = SAMPLE_RATE
            .saturating_mul(TARGET_SAMPLE_SECONDS.min(capabilities.analysis_max_total_seconds))
            .min(max_samples_by_bytes);
        let minimum_enrollment_samples =
            SAMPLE_RATE.saturating_mul(capabilities.analysis_min_accepted_seconds);
        if target_samples < minimum_enrollment_samples {
            bail!("服务端声纹样本限制不足以容纳最短有效音频");
        }
        let plans = build_speaker_plans(&transcript.segments, target_samples, &capabilities)?;
        if plans.is_empty() {
            bail!("当前转写结果中没有可识别的 speaker 标签");
        }
        let samples =
            extract_planned_samples(Path::new(&recording.path), recording.duration_ms, &plans)?;
        let stored = self.storage.voiceprint_embeddings()?;
        let mut pending = Vec::with_capacity(plans.len());
        for plan in plans {
            let speaker_samples = samples.get(&plan.raw_speaker).cloned().unwrap_or_default();
            let fallback_preview = plan.fallback_preview.as_ref();
            let mut view = SpeakerIdentificationCandidate {
                raw_speaker: plan.raw_speaker.clone(),
                total_speech_ms: fallback_preview
                    .map_or(0, |range| range.end_ms.saturating_sub(range.start_ms)),
                preview_start_ms: fallback_preview.map_or(0, |range| range.start_ms),
                preview_end_ms: fallback_preview.map_or(0, |range| range.end_ms),
                embedding_extracted: false,
                sample_status: if fallback_preview.is_some() {
                    SpeakerSampleStatus::PreviewOnly
                } else {
                    SpeakerSampleStatus::Unavailable
                },
                status_message: fallback_preview
                    .map(|_| "当前片段仅供人工确认；达到纯净度门槛后才会保存声纹".to_owned()),
                error_message: None,
                suggested_participant_id: None,
                suggested_participant_name: None,
                match_score: None,
            };
            let minimum_candidate_samples =
                SAMPLE_RATE.saturating_mul(capabilities.analysis_min_clip_seconds) as usize;
            let analysis_samples = speaker_samples
                .iter()
                .filter(|range| range.samples.len() >= minimum_candidate_samples)
                .cloned()
                .collect::<Vec<_>>();
            let available_samples = analysis_samples
                .iter()
                .map(|range| range.samples.len())
                .sum::<usize>();
            if analysis_samples.is_empty() {
                view.status_message = Some(
                    "没有达到分析时长的片段；仍可试听原始片段并手动标记，本次不会保存声纹"
                        .to_owned(),
                );
                pending.push(PendingCandidate {
                    view,
                    embedding_fingerprint: String::new(),
                    embedding: None,
                });
                continue;
            }

            let mut wav_paths = Vec::with_capacity(analysis_samples.len());
            let extracted = (|| {
                for (index, range) in analysis_samples.iter().enumerate() {
                    let wav_path = self.temp_directory.join(format!(
                        "{}-{}-{index}.wav",
                        Uuid::new_v4(),
                        sanitize_file_component(&plan.raw_speaker)
                    ));
                    write_pcm16_wav(&wav_path, &range.samples)?;
                    wav_paths.push(wav_path);
                }
                analyze_speaker_samples(&credentials, &wav_paths)
            })();
            for wav_path in &wav_paths {
                let _ = std::fs::remove_file(wav_path);
            }
            match extracted {
                Ok(result) => {
                    let validated = (|| -> Result<(u64, u64)> {
                        let available_audio_ms = available_samples as u64 * 1_000 / SAMPLE_RATE;
                        if result.accepted_audio_ms > available_audio_ms {
                            bail!("服务端返回了无效的纯净声音累计时长");
                        }
                        let preview_source = analysis_samples
                            .get(result.preview.file_index)
                            .context("纯净声音样本响应引用了不存在的候选片段")?;
                        let preview_source_duration = preview_source
                            .end_ms
                            .saturating_sub(preview_source.start_ms);
                        if result.preview.end_ms > preview_source_duration {
                            bail!("服务端返回的纯净声音试听范围超出候选片段");
                        }
                        Ok((
                            preview_source
                                .start_ms
                                .saturating_add(result.preview.start_ms),
                            preview_source
                                .start_ms
                                .saturating_add(result.preview.end_ms),
                        ))
                    })();
                    match validated {
                        Ok((preview_start_ms, preview_end_ms)) => {
                            view.preview_start_ms = preview_start_ms;
                            view.preview_end_ms = preview_end_ms;
                            match (result.outcome, result.embedding) {
                                (SpeakerSampleAnalysisOutcome::Enrollable, Some(embedding)) => {
                                    if result.purity_score < capabilities.analysis_min_purity
                                        || result.accepted_ranges.is_empty()
                                        || result.accepted_audio_ms
                                            < capabilities
                                                .analysis_min_accepted_seconds
                                                .saturating_mul(1_000)
                                    {
                                        view.status_message = Some(
                                            "服务端未返回满足入库标准的声纹；当前片段仅供人工确认"
                                                .to_owned(),
                                        );
                                        pending.push(PendingCandidate {
                                            view,
                                            embedding_fingerprint: String::new(),
                                            embedding: None,
                                        });
                                        continue;
                                    }
                                    view.total_speech_ms = result.accepted_audio_ms;
                                    let suggestion =
                                        best_match(&embedding, &result.fingerprint, &stored);
                                    if let Some(best) = suggestion {
                                        view.match_score = Some(best.score);
                                        if best.confident {
                                            view.suggested_participant_id =
                                                Some(best.participant_id);
                                            view.suggested_participant_name =
                                                Some(best.display_name);
                                        }
                                    }
                                    view.embedding_extracted = true;
                                    view.sample_status = SpeakerSampleStatus::Enrollable;
                                    view.status_message =
                                        Some("已找到满足入库标准的纯净单人声音".to_owned());
                                    pending.push(PendingCandidate {
                                        view,
                                        embedding_fingerprint: result.fingerprint,
                                        embedding: Some(embedding),
                                    });
                                }
                                (SpeakerSampleAnalysisOutcome::PreviewOnly, None) => {
                                    view.total_speech_ms =
                                        preview_end_ms.saturating_sub(preview_start_ms);
                                    view.sample_status = SpeakerSampleStatus::PreviewOnly;
                                    view.status_message = Some(
                                        "可试听并手动标记姓名；当前声音不满足声纹入库标准"
                                            .to_owned(),
                                    );
                                    pending.push(PendingCandidate {
                                        view,
                                        embedding_fingerprint: String::new(),
                                        embedding: None,
                                    });
                                }
                                _ => unreachable!("分析响应已在 HTTP 边界完成一致性校验"),
                            }
                        }
                        Err(error) => {
                            view.error_message = Some(format!("试听范围无效：{error:#}"));
                            pending.push(PendingCandidate {
                                view,
                                embedding_fingerprint: String::new(),
                                embedding: None,
                            });
                        }
                    }
                }
                Err(_error) => {
                    view.status_message = Some(
                        "声纹服务未能完成纯净分析；已保留原始片段供人工确认，本次不会保存声纹"
                            .to_owned(),
                    );
                    pending.push(PendingCandidate {
                        view,
                        embedding_fingerprint: String::new(),
                        embedding: None,
                    });
                }
            }
        }

        let session_id = Uuid::new_v4().to_string();
        let response = SpeakerIdentificationSession {
            id: session_id.clone(),
            recording_id: recording_id.to_owned(),
            speaker_count: pending.len() as u32,
            voiceprint_count: pending
                .iter()
                .filter(|candidate| candidate.embedding.is_some())
                .count() as u32,
            candidates: pending
                .iter()
                .map(|candidate| candidate.view.clone())
                .collect(),
        };
        let mut sessions = self.sessions.lock();
        if sessions.len() >= MAX_PENDING_SESSIONS
            && let Some(oldest_key) = sessions.keys().next().cloned()
        {
            sessions.remove(&oldest_key);
        }
        sessions.insert(
            session_id,
            PendingSession {
                recording_id: recording_id.to_owned(),
                generation,
                candidates: pending,
            },
        );
        Ok(response)
    }

    pub fn save(
        &self,
        session_id: &str,
        assignments: Vec<SpeakerIdentificationAssignment>,
    ) -> Result<String> {
        let session = self
            .sessions
            .lock()
            .get(session_id)
            .cloned()
            .context("说话人识别会话已过期，请重新提取")?;
        if self
            .storage
            .transcription_generation(&session.recording_id)?
            != session.generation
        {
            bail!("会议已经重新转写，请重新执行说话人识别");
        }
        let candidates = session
            .candidates
            .iter()
            .map(|candidate| (candidate.view.raw_speaker.as_str(), candidate))
            .collect::<HashMap<_, _>>();
        let mut seen = std::collections::HashSet::new();
        let mut enrollments = Vec::new();
        for assignment in assignments {
            if !seen.insert(assignment.raw_speaker.clone()) {
                bail!("同一个 speaker 只能保存一次");
            }
            let candidate = candidates
                .get(assignment.raw_speaker.as_str())
                .context("识别结果中不存在该 speaker")?;
            let match_score = assignment
                .participant_id
                .as_deref()
                .filter(|id| candidate.view.suggested_participant_id.as_deref() == Some(*id))
                .and(candidate.view.match_score);
            enrollments.push(VoiceprintEnrollment {
                raw_speaker: assignment.raw_speaker,
                participant_id: assignment.participant_id,
                new_display_name: assignment.new_display_name,
                match_score,
                embedding_fingerprint: candidate.embedding_fingerprint.clone(),
                embedding: candidate.embedding.clone(),
                preview_start_ms: candidate.view.preview_start_ms,
                preview_end_ms: candidate.view.preview_end_ms,
                speech_duration_ms: candidate.view.total_speech_ms,
            });
        }
        self.storage.save_speaker_identification(
            &session.recording_id,
            session.generation,
            &enrollments,
        )?;
        self.sessions.lock().remove(session_id);
        Ok(session.recording_id)
    }

    pub fn discard(&self, session_id: &str) {
        self.sessions.lock().remove(session_id);
    }

    pub fn list_participants(&self) -> Result<Vec<ParticipantProfile>> {
        self.storage.list_participants()
    }

    fn resolve_provider(&self, requested_id: Option<&str>) -> Result<AsrProviderCredentials> {
        let settings = self.storage.settings()?;
        let selected = requested_id
            .filter(|id| !id.trim().is_empty())
            .map(ToOwned::to_owned)
            .or(settings.voiceprint_provider_id)
            .or_else(|| {
                settings.active_asr_provider_id.and_then(|id| {
                    self.storage
                        .find_asr_provider(&id)
                        .ok()
                        .filter(|provider| provider.provider.kind == AsrProviderKind::FunAsr)
                        .map(|_| id)
                })
            })
            .or_else(|| {
                self.storage
                    .list_asr_providers()
                    .ok()?
                    .into_iter()
                    .find(|provider| provider.kind == AsrProviderKind::FunAsr)
                    .map(|provider| provider.id)
            })
            .context("请先在声纹管理中选择一个 Nota ASR Server")?;
        let credentials = self.storage.find_asr_provider(&selected)?;
        if credentials.provider.kind != AsrProviderKind::FunAsr {
            bail!("说话人识别只能使用 FunASR 类型的 Nota ASR Server");
        }
        Ok(credentials)
    }
}

fn build_speaker_plans(
    segments: &[TranscriptSegment],
    target_samples: u64,
    capabilities: &SpeakerEmbeddingCapabilities,
) -> Result<Vec<SpeakerPlan>> {
    let mut grouped: BTreeMap<String, Vec<&TranscriptSegment>> = BTreeMap::new();
    for segment in segments {
        let Some(speaker) = segment
            .speaker
            .as_deref()
            .map(str::trim)
            .filter(|speaker| !speaker.is_empty())
        else {
            continue;
        };
        if segment.end_ms > segment.start_ms {
            grouped.entry(speaker.to_owned()).or_default().push(segment);
        }
    }
    let target_ms = (target_samples.saturating_mul(1_000) / SAMPLE_RATE).min(
        capabilities
            .analysis_max_total_seconds
            .saturating_mul(1_000),
    );
    let minimum_range_ms = capabilities.analysis_min_clip_seconds.saturating_mul(1_000);
    let maximum_range_ms = capabilities
        .analysis_max_clip_seconds
        .min(MAX_RANGE_SECONDS)
        .saturating_mul(1_000);
    let mut plans = Vec::new();
    for (speaker, mut speaker_segments) in grouped {
        speaker_segments.sort_by_key(|segment| Reverse(segment.end_ms - segment.start_ms));
        let fallback_preview = speaker_segments.first().map(|segment| SampleRange {
            start_ms: segment.start_ms,
            end_ms: segment.start_ms.saturating_add(
                (segment.end_ms - segment.start_ms)
                    .min(FALLBACK_PREVIEW_SECONDS.saturating_mul(1_000)),
            ),
        });
        let mut remaining = target_ms;
        let mut ranges: Vec<SampleRange> = Vec::new();
        for require_spacing in [true, false] {
            for segment in &speaker_segments {
                if remaining < minimum_range_ms || ranges.len() >= capabilities.analysis_max_files {
                    break;
                }
                if ranges
                    .iter()
                    .any(|range| range.start_ms == segment.start_ms)
                {
                    continue;
                }
                let duration = (segment.end_ms - segment.start_ms)
                    .min(maximum_range_ms)
                    .min(remaining);
                if duration < minimum_range_ms {
                    continue;
                }
                let candidate = SampleRange {
                    start_ms: segment.start_ms,
                    end_ms: segment.start_ms + duration,
                };
                if ranges.iter().any(|range| {
                    candidate.start_ms < range.end_ms && candidate.end_ms > range.start_ms
                }) {
                    continue;
                }
                if require_spacing {
                    let midpoint = candidate.start_ms + duration / 2;
                    if ranges.iter().any(|range| {
                        let selected_midpoint =
                            range.start_ms + (range.end_ms - range.start_ms) / 2;
                        midpoint.abs_diff(selected_midpoint) < MIN_CANDIDATE_SPACING_MS
                    }) {
                        continue;
                    }
                }
                remaining -= duration;
                ranges.push(candidate);
            }
        }
        ranges.sort_by_key(|range| range.start_ms);
        plans.push(SpeakerPlan {
            raw_speaker: speaker,
            fallback_preview,
            ranges,
        });
    }
    Ok(plans)
}

fn extract_planned_samples(
    path: &Path,
    duration_ms: u64,
    plans: &[SpeakerPlan],
) -> Result<HashMap<String, Vec<ExtractedRange>>> {
    let mut output = plans
        .iter()
        .map(|plan| {
            (
                plan.raw_speaker.clone(),
                plan.ranges
                    .iter()
                    .map(|range| ExtractedRange {
                        start_ms: range.start_ms,
                        end_ms: range.end_ms,
                        samples: Vec::new(),
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut reader = OggPcm16Reader::open(path, duration_ms)?;
    let mut offset = 0u64;
    while !reader.finished() {
        let block = reader.read_samples(4_096)?;
        if block.is_empty() {
            continue;
        }
        let block_start = offset;
        let block_end = offset + block.len() as u64;
        for plan in plans {
            let destinations = output
                .get_mut(&plan.raw_speaker)
                .context("声纹采样计划失效")?;
            for (range, destination) in plan.ranges.iter().zip(destinations.iter_mut()) {
                let range_start = range.start_ms.saturating_mul(SAMPLE_RATE) / 1_000;
                let range_end = range.end_ms.saturating_mul(SAMPLE_RATE) / 1_000;
                let copy_start = block_start.max(range_start);
                let copy_end = block_end.min(range_end);
                if copy_end > copy_start {
                    let start = (copy_start - block_start) as usize;
                    let end = (copy_end - block_start) as usize;
                    destination.samples.extend_from_slice(&block[start..end]);
                }
            }
        }
        offset = block_end;
    }
    Ok(output)
}

struct MatchSuggestion {
    participant_id: String,
    display_name: String,
    score: f32,
    confident: bool,
}

fn best_match(
    candidate: &[f32],
    fingerprint: &str,
    samples: &[crate::storage::StoredVoiceprintEmbedding],
) -> Option<MatchSuggestion> {
    let mut grouped: HashMap<(&str, &str), Vec<&[f32]>> = HashMap::new();
    for sample in samples {
        if sample.embedding_fingerprint == fingerprint && sample.embedding.len() == candidate.len()
        {
            grouped
                .entry((&sample.participant_id, &sample.display_name))
                .or_default()
                .push(&sample.embedding);
        }
    }
    let mut scores = grouped
        .into_iter()
        .filter_map(|((participant_id, display_name), embeddings)| {
            let prototype = normalized_average(&embeddings)?;
            let score = cosine(candidate, &prototype)?;
            Some((participant_id.to_owned(), display_name.to_owned(), score))
        })
        .collect::<Vec<_>>();
    scores.sort_by(|left, right| right.2.total_cmp(&left.2));
    let (participant_id, display_name, score) = scores.first()?.clone();
    let runner_up = scores.get(1).map(|item| item.2).unwrap_or(-1.0);
    Some(MatchSuggestion {
        participant_id,
        display_name,
        score,
        confident: score >= MIN_MATCH_SIMILARITY && score - runner_up >= MIN_MATCH_MARGIN,
    })
}

fn normalized_average(embeddings: &[&[f32]]) -> Option<Vec<f32>> {
    let dimension = embeddings.first()?.len();
    if dimension == 0
        || embeddings
            .iter()
            .any(|embedding| embedding.len() != dimension)
    {
        return None;
    }
    let mut average = vec![0.0f32; dimension];
    for embedding in embeddings {
        for (sum, value) in average.iter_mut().zip(embedding.iter()) {
            *sum += value;
        }
    }
    normalize(&mut average).then_some(average)
}

fn cosine(left: &[f32], right: &[f32]) -> Option<f32> {
    (left.len() == right.len() && !left.is_empty()).then(|| {
        left.iter()
            .zip(right.iter())
            .map(|(left, right)| left * right)
            .sum()
    })
}

fn normalize(values: &mut [f32]) -> bool {
    let norm = values.iter().map(|value| value * value).sum::<f32>().sqrt();
    if !norm.is_finite() || norm <= f32::EPSILON {
        return false;
    }
    for value in values {
        *value /= norm;
    }
    true
}

fn clean_temp_directory(path: &Path) -> Result<()> {
    for entry in std::fs::read_dir(path)? {
        let path = entry?.path();
        if path.is_file() && path.extension().and_then(|value| value.to_str()) == Some("wav") {
            std::fs::remove_file(path)?;
        }
    }
    Ok(())
}

fn sanitize_file_component(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || *character == '_')
        .take(32)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_multiple_bounded_ranges_for_each_speaker() {
        let segments = vec![
            TranscriptSegment {
                start_ms: 0,
                end_ms: 10_000,
                text: "one".into(),
                speaker: Some("speaker_0".into()),
            },
            TranscriptSegment {
                start_ms: 20_000,
                end_ms: 32_000,
                text: "two".into(),
                speaker: Some("speaker_0".into()),
            },
        ];

        let capabilities = SpeakerEmbeddingCapabilities {
            max_bytes: 2 * 1024 * 1024,
            analysis_max_files: 8,
            analysis_min_clip_seconds: 3,
            analysis_max_clip_seconds: 12,
            analysis_max_total_seconds: 30,
            analysis_min_accepted_seconds: 5,
            analysis_min_purity: 0.70,
        };
        let plans = build_speaker_plans(&segments, SAMPLE_RATE * 20, &capabilities).unwrap();

        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].ranges.len(), 2);
        assert_eq!(
            plans[0]
                .ranges
                .iter()
                .map(|range| range.end_ms - range.start_ms)
                .sum::<u64>(),
            12_000
        );
        let fallback_preview = plans[0].fallback_preview.as_ref().unwrap();
        assert_eq!(fallback_preview.start_ms, 20_000);
        assert_eq!(fallback_preview.end_ms, 28_000);
    }

    #[test]
    fn ignores_short_and_mixed_turn_candidates_for_enrollment_planning() {
        let segments = vec![
            TranscriptSegment {
                start_ms: 0,
                end_ms: 500,
                text: "short".into(),
                speaker: Some("speaker_0".into()),
            },
            TranscriptSegment {
                start_ms: 10_000,
                end_ms: 16_000,
                text: "long".into(),
                speaker: Some("speaker_0".into()),
            },
        ];
        let capabilities = SpeakerEmbeddingCapabilities {
            max_bytes: 2 * 1024 * 1024,
            analysis_max_files: 8,
            analysis_min_clip_seconds: 3,
            analysis_max_clip_seconds: 12,
            analysis_max_total_seconds: 30,
            analysis_min_accepted_seconds: 5,
            analysis_min_purity: 0.70,
        };

        let plans = build_speaker_plans(&segments, SAMPLE_RATE * 20, &capabilities).unwrap();

        assert_eq!(plans[0].ranges.len(), 1);
        assert_eq!(plans[0].ranges[0].start_ms, 10_000);
    }

    #[test]
    fn requires_threshold_and_runner_up_margin_for_confident_match() {
        let samples = vec![
            crate::storage::StoredVoiceprintEmbedding {
                participant_id: "a".into(),
                display_name: "小明".into(),
                embedding_fingerprint: "model".into(),
                dimension: 2,
                embedding: vec![1.0, 0.0],
            },
            crate::storage::StoredVoiceprintEmbedding {
                participant_id: "b".into(),
                display_name: "小红".into(),
                embedding_fingerprint: "model".into(),
                dimension: 2,
                embedding: vec![0.99, 0.1],
            },
        ];

        let close = best_match(&[1.0, 0.0], "model", &samples).unwrap();
        assert!(!close.confident);
        let single = best_match(&[1.0, 0.0], "model", &samples[..1]).unwrap();
        assert!(single.confident);
    }
}
