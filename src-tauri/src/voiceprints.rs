use crate::asr::{
    OggPcm16Reader, extract_speaker_embedding, fetch_speaker_embedding_capabilities,
    write_pcm16_wav,
};
use crate::models::{
    AsrProviderCredentials, AsrProviderKind, ParticipantProfile, SpeakerIdentificationAssignment,
    SpeakerIdentificationCandidate, SpeakerIdentificationSession, TranscriptSegment,
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
const TARGET_SAMPLE_SECONDS: u64 = 20;
const MAX_RANGE_SECONDS: u64 = 8;
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
    total_speech_ms: u64,
    preview_start_ms: u64,
    preview_end_ms: u64,
    ranges: Vec<SampleRange>,
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
            .saturating_mul(TARGET_SAMPLE_SECONDS.min(capabilities.max_seconds))
            .min(max_samples_by_bytes);
        let minimum_samples = SAMPLE_RATE.saturating_mul(capabilities.min_seconds);
        if target_samples < minimum_samples {
            bail!("服务端声纹样本限制不足以容纳最短有效音频");
        }
        let plans = build_speaker_plans(&transcript.segments, target_samples)?;
        if plans.is_empty() {
            bail!("当前转写结果中没有可识别的 speaker 标签");
        }
        let samples =
            extract_planned_samples(Path::new(&recording.path), recording.duration_ms, &plans)?;
        let stored = self.storage.voiceprint_embeddings()?;
        let mut pending = Vec::with_capacity(plans.len());
        for plan in plans {
            let speaker_samples = samples.get(&plan.raw_speaker).cloned().unwrap_or_default();
            let mut view = SpeakerIdentificationCandidate {
                raw_speaker: plan.raw_speaker.clone(),
                total_speech_ms: plan.total_speech_ms,
                preview_start_ms: plan.preview_start_ms,
                preview_end_ms: plan.preview_end_ms,
                embedding_extracted: false,
                error_message: None,
                suggested_participant_id: None,
                suggested_participant_name: None,
                match_score: None,
            };
            if speaker_samples.len() < minimum_samples as usize {
                view.error_message = Some(format!(
                    "有效语音不足 {} 秒，未生成声纹",
                    capabilities.min_seconds
                ));
                pending.push(PendingCandidate {
                    view,
                    embedding_fingerprint: String::new(),
                    embedding: None,
                });
                continue;
            }

            let wav_path = self.temp_directory.join(format!(
                "{}-{}.wav",
                Uuid::new_v4(),
                sanitize_file_component(&plan.raw_speaker)
            ));
            let extracted = (|| {
                write_pcm16_wav(&wav_path, &speaker_samples)?;
                extract_speaker_embedding(&credentials, &wav_path)
            })();
            let _ = std::fs::remove_file(&wav_path);
            match extracted {
                Ok(result) => {
                    let suggestion = best_match(&result.embedding, &result.fingerprint, &stored);
                    if let Some(best) = suggestion {
                        view.match_score = Some(best.score);
                        if best.confident {
                            view.suggested_participant_id = Some(best.participant_id);
                            view.suggested_participant_name = Some(best.display_name);
                        }
                    }
                    view.embedding_extracted = true;
                    pending.push(PendingCandidate {
                        view,
                        embedding_fingerprint: result.fingerprint,
                        embedding: Some(result.embedding),
                    });
                }
                Err(error) => {
                    view.error_message = Some(format!("声纹提取失败：{error:#}"));
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
    let target_ms = target_samples.saturating_mul(1_000) / SAMPLE_RATE;
    let mut plans = Vec::new();
    for (speaker, mut speaker_segments) in grouped {
        speaker_segments.sort_by_key(|segment| Reverse(segment.end_ms - segment.start_ms));
        let preview = speaker_segments[0];
        let total_speech_ms = speaker_segments
            .iter()
            .map(|segment| segment.end_ms - segment.start_ms)
            .sum();
        let mut remaining = target_ms;
        let mut ranges = Vec::new();
        for segment in speaker_segments {
            if remaining == 0 {
                break;
            }
            let duration = (segment.end_ms - segment.start_ms)
                .min(MAX_RANGE_SECONDS * 1_000)
                .min(remaining);
            if duration < 250 {
                continue;
            }
            ranges.push(SampleRange {
                start_ms: segment.start_ms,
                end_ms: segment.start_ms + duration,
            });
            remaining -= duration;
        }
        ranges.sort_by_key(|range| range.start_ms);
        plans.push(SpeakerPlan {
            raw_speaker: speaker,
            total_speech_ms,
            preview_start_ms: preview.start_ms,
            preview_end_ms: preview.end_ms,
            ranges,
        });
    }
    Ok(plans)
}

fn extract_planned_samples(
    path: &Path,
    duration_ms: u64,
    plans: &[SpeakerPlan],
) -> Result<HashMap<String, Vec<i16>>> {
    let mut output = plans
        .iter()
        .map(|plan| (plan.raw_speaker.clone(), Vec::new()))
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
            let destination = output
                .get_mut(&plan.raw_speaker)
                .context("声纹采样计划失效")?;
            for range in &plan.ranges {
                let range_start = range.start_ms.saturating_mul(SAMPLE_RATE) / 1_000;
                let range_end = range.end_ms.saturating_mul(SAMPLE_RATE) / 1_000;
                let copy_start = block_start.max(range_start);
                let copy_end = block_end.min(range_end);
                if copy_end > copy_start {
                    let start = (copy_start - block_start) as usize;
                    let end = (copy_end - block_start) as usize;
                    destination.extend_from_slice(&block[start..end]);
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

        let plans = build_speaker_plans(&segments, SAMPLE_RATE * 20).unwrap();

        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].ranges.len(), 2);
        assert_eq!(
            plans[0]
                .ranges
                .iter()
                .map(|range| range.end_ms - range.start_ms)
                .sum::<u64>(),
            16_000
        );
        assert_eq!(plans[0].preview_start_ms, 20_000);
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
