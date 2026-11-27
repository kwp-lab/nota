//! `dashscope_filetrans_segmented_v1`: transcribes a recording longer than the
//! DashScope two-hour diarization limit as sequential, independent segments.
//!
//! Speaker labels are not aligned across segments. Each segment's labels are
//! renumbered after the previous segment's, and the user merges a person by
//! naming their labels the same.

use super::{
    Cancellation, DashScopeOutcome, TranscriptionJob, dashscope, dashscope_poll_task,
    dashscope_upload_and_submit, emit_current_status, validate_dashscope_hotwords,
};
use crate::audio::segments::{
    DASHSCOPE_SEGMENT_LIMITS, PlannedSegment, plan_segments, write_segment,
};
use crate::models::{
    AsrProviderCredentials, StoredTranscriptionChunk, TranscriptSegment,
    TranscriptionProgressPhase, TranscriptionProgressUnit, TranscriptionStatus,
};
use crate::storage::Storage;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const CHECKPOINT_VERSION: u32 = 2;
/// Allowed overrun of a provider sentence past its segment's planned length.
const TIMESTAMP_TOLERANCE_MS: u64 = 2_000;

/// Checkpoint version 2. The active segment's `task_id` lives in
/// `transcriptions.remote_job_id`, as it does for the single-file protocol.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct SegmentedCheckpoint {
    version: u32,
    plan: Vec<PlannedSegment>,
    active_index: u32,
    active: dashscope::Checkpoint,
}

impl SegmentedCheckpoint {
    fn from_json(value: &str) -> Result<Option<Self>> {
        if value.trim().is_empty() || value.trim() == "{}" {
            return Ok(None);
        }
        let checkpoint: Self =
            serde_json::from_str(value).context("无法读取 DashScope 分段恢复状态")?;
        if checkpoint.version != CHECKPOINT_VERSION {
            bail!("DashScope 分段恢复状态版本不受支持");
        }
        if checkpoint.plan.is_empty() {
            bail!("DashScope 分段恢复状态缺少分段计划");
        }
        Ok(Some(checkpoint))
    }

    fn to_json(&self) -> Result<String> {
        serde_json::to_string(self).context("无法保存 DashScope 分段恢复状态")
    }
}

fn segment_file_path(storage: &Storage, job: &TranscriptionJob, index: u32) -> PathBuf {
    storage
        .paths()
        .recovery
        .join("TranscriptionTemp")
        .join(format!(
            "{}-g{}-s{}.ogg",
            job.recording_id, job.generation, index
        ))
}

/// Removes a temporary segment file when dropped, whatever the upload outcome.
struct TemporaryFile(PathBuf);

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

pub(super) fn process_dashscope_segmented_job(
    job: &TranscriptionJob,
    storage: &Arc<Storage>,
    cancellation: &Cancellation,
    credentials: &AsrProviderCredentials,
    recording_path: &Path,
) -> Result<()> {
    dashscope::DashScopeAdapter::validate_credentials(credentials)?;
    if recording_path.extension().and_then(|value| value.to_str()) != Some("ogg") {
        bail!("百炼云转写当前仅支持 Nota 原始 Ogg 录音");
    }
    let execution =
        storage.transcription_execution_for_generation(&job.recording_id, job.generation)?;
    validate_dashscope_hotwords(&execution.hotwords)?;

    let mut state = match SegmentedCheckpoint::from_json(&execution.provider_state_json)? {
        Some(state) => state,
        None => {
            storage.set_batch_transcription_progress(
                &job.recording_id,
                job.generation,
                TranscriptionStatus::Preparing,
                TranscriptionProgressPhase::Preparing,
                0,
                0,
                TranscriptionProgressUnit::Bytes,
            )?;
            emit_current_status(&job.app, storage, &job.recording_id, job.generation);
            let plan = plan_segments(recording_path, DASHSCOPE_SEGMENT_LIMITS)?;
            let state = SegmentedCheckpoint {
                version: CHECKPOINT_VERSION,
                plan,
                active_index: 0,
                active: dashscope::Checkpoint::default(),
            };
            storage.set_transcription_provider_checkpoint(
                &job.recording_id,
                job.generation,
                None,
                &state.to_json()?,
            )?;
            state
        }
    };
    let segment_count = state.plan.len() as u32;
    let mut task_id = execution.remote_job_id;

    for segment in state.plan.clone() {
        // A stored segment is never uploaded or submitted again, even if the
        // checkpoint had not advanced past it before an interruption.
        let stored: HashSet<u32> = storage
            .completed_transcription_chunks(&job.recording_id, job.generation)?
            .into_iter()
            .collect();
        if stored.contains(&segment.index) {
            continue;
        }
        if state.active_index != segment.index {
            state.active_index = segment.index;
            state.active = dashscope::Checkpoint::default();
            task_id = None;
            storage.set_transcription_provider_checkpoint(
                &job.recording_id,
                job.generation,
                None,
                &state.to_json()?,
            )?;
        }
        storage.set_transcription_segment_progress(
            &job.recording_id,
            job.generation,
            segment.index,
            segment_count,
        )?;
        emit_current_status(&job.app, storage, &job.recording_id, job.generation);
        let transcript = transcribe_segment(
            job,
            storage,
            cancellation,
            credentials,
            recording_path,
            &segment,
            &mut state,
            task_id.take(),
            execution.speaker_count,
            &execution.hotwords,
        )
        .with_context(|| format!("第 {}/{} 段", segment.index + 1, segment_count))?;
        storage.save_transcription_chunk(&job.recording_id, job.generation, &transcript)?;
    }

    let chunks = storage.transcription_chunks(&job.recording_id, job.generation)?;
    let (text, segments, language) = merge_segments(&state.plan, chunks)?;
    storage.complete_transcription(
        &job.recording_id,
        job.generation,
        &text,
        &segments,
        language.as_deref(),
    )?;
    emit_current_status(&job.app, storage, &job.recording_id, job.generation);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn transcribe_segment(
    job: &TranscriptionJob,
    storage: &Arc<Storage>,
    cancellation: &Cancellation,
    credentials: &AsrProviderCredentials,
    recording_path: &Path,
    segment: &PlannedSegment,
    state: &mut SegmentedCheckpoint,
    mut task_id: Option<String>,
    speaker_count: Option<u32>,
    hotwords: &[crate::models::HotwordEntry],
) -> Result<StoredTranscriptionChunk> {
    loop {
        let mut active = state.active.clone();
        let needs_upload = task_id.is_none()
            && active.stage != dashscope::Stage::Submitting
            && !active.uploaded_object_is_valid();
        let upload_path = segment_file_path(storage, job, segment.index);
        let _temporary = if needs_upload {
            if let Some(parent) = upload_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            // A leftover file from an interrupted attempt is regenerated from the plan.
            let _ = std::fs::remove_file(&upload_path);
            write_segment(recording_path, segment, &upload_path)?;
            Some(TemporaryFile(upload_path.clone()))
        } else {
            None
        };
        let mut save = |remote_job_id: Option<&str>, value: &dashscope::Checkpoint| {
            let mut next = state.clone();
            next.active = value.clone();
            storage.set_transcription_provider_checkpoint(
                &job.recording_id,
                job.generation,
                remote_job_id,
                &next.to_json()?,
            )
        };
        let submitted = dashscope_upload_and_submit(
            job,
            storage,
            cancellation,
            credentials,
            &upload_path,
            speaker_count,
            hotwords,
            &mut active,
            task_id.take(),
            &mut save,
        )?;
        drop(_temporary);
        let outcome = dashscope_poll_task(
            job,
            storage,
            cancellation,
            credentials,
            &submitted,
            &mut active,
            &mut save,
        )?;
        state.active = active;
        match outcome {
            DashScopeOutcome::Transcript(transcript) => {
                validate_segment_timestamps(segment, &transcript.segments)?;
                return Ok(StoredTranscriptionChunk {
                    index: segment.index,
                    start_ms: segment.start_ms,
                    end_ms: segment.end_ms,
                    text: transcript.text,
                    segments: transcript.segments,
                    language: transcript.language,
                });
            }
            DashScopeOutcome::RemoteCancelled => {
                // The provider dropped this segment's task; upload and submit it again.
                state.active = dashscope::Checkpoint::default();
                storage.set_transcription_provider_checkpoint(
                    &job.recording_id,
                    job.generation,
                    None,
                    &state.to_json()?,
                )?;
            }
        }
    }
}

fn validate_segment_timestamps(
    segment: &PlannedSegment,
    sentences: &[TranscriptSegment],
) -> Result<()> {
    let limit = segment.duration_ms() + TIMESTAMP_TOLERANCE_MS;
    if sentences.iter().any(|sentence| sentence.end_ms > limit) {
        bail!("DashScope 返回的时间戳超出该段录音长度");
    }
    Ok(())
}

fn speaker_number(label: &str) -> Option<u64> {
    label.strip_prefix("speaker_")?.parse().ok()
}

/// Merges stored segment results into one transcript. Timestamps move onto
/// the recording timeline and speaker labels are renumbered so that labels
/// from different segments never collide.
pub(super) fn merge_segments(
    plan: &[PlannedSegment],
    mut chunks: Vec<StoredTranscriptionChunk>,
) -> Result<(String, Vec<TranscriptSegment>, Option<String>)> {
    chunks.sort_by_key(|chunk| chunk.index);
    let indices: Vec<u32> = chunks.iter().map(|chunk| chunk.index).collect();
    let expected: Vec<u32> = plan.iter().map(|segment| segment.index).collect();
    if indices != expected {
        bail!("分段转写结果不完整，无法合并");
    }
    let mut text = String::new();
    let mut merged = Vec::new();
    let mut language = None;
    let mut base = 0u64;
    for chunk in chunks {
        if language.is_none() {
            language = chunk.language.clone();
        }
        let mut labels: Vec<&str> = chunk
            .segments
            .iter()
            .filter_map(|sentence| sentence.speaker.as_deref())
            .collect();
        labels.sort_by(|left, right| {
            speaker_number(left)
                .cmp(&speaker_number(right))
                .then_with(|| left.cmp(right))
        });
        labels.dedup();
        let mapping: BTreeMap<String, String> = labels
            .iter()
            .enumerate()
            .map(|(offset, label)| {
                (
                    (*label).to_owned(),
                    format!("speaker_{}", base + offset as u64),
                )
            })
            .collect();
        base += mapping.len() as u64;
        for sentence in &chunk.segments {
            merged.push(TranscriptSegment {
                start_ms: sentence.start_ms.saturating_add(chunk.start_ms),
                end_ms: sentence.end_ms.saturating_add(chunk.start_ms),
                text: sentence.text.clone(),
                speaker: sentence
                    .speaker
                    .as_deref()
                    .and_then(|label| mapping.get(label).cloned()),
            });
        }
        let part = chunk.text.trim();
        if !part.is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(part);
        }
    }
    Ok((text, merged, language))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn planned(index: u32, start_ms: u64, end_ms: u64) -> PlannedSegment {
        PlannedSegment {
            index,
            start_sample: start_ms * 48,
            end_sample: end_ms * 48,
            start_ms,
            end_ms,
        }
    }

    fn sentence(
        start_ms: u64,
        end_ms: u64,
        speaker: Option<&str>,
        text: &str,
    ) -> TranscriptSegment {
        TranscriptSegment {
            start_ms,
            end_ms,
            text: text.into(),
            speaker: speaker.map(str::to_owned),
        }
    }

    fn chunk(
        index: u32,
        start_ms: u64,
        end_ms: u64,
        segments: Vec<TranscriptSegment>,
    ) -> StoredTranscriptionChunk {
        StoredTranscriptionChunk {
            index,
            start_ms,
            end_ms,
            text: segments
                .iter()
                .map(|segment| segment.text.as_str())
                .collect::<Vec<_>>()
                .join(""),
            segments,
            language: if index == 0 { None } else { Some("zh".into()) },
        }
    }

    #[test]
    fn merge_offsets_timestamps_and_renumbers_speakers_per_segment() {
        let plan = vec![planned(0, 0, 60_000), planned(1, 60_000, 120_000)];
        let chunks = vec![
            chunk(
                1,
                60_000,
                120_000,
                vec![
                    sentence(0, 1_000, Some("speaker_1"), "乙。"),
                    sentence(2_000, 3_000, Some("speaker_0"), "甲。"),
                    sentence(4_000, 5_000, None, "旁白。"),
                ],
            ),
            chunk(
                0,
                0,
                60_000,
                vec![
                    sentence(0, 1_000, Some("speaker_0"), "一。"),
                    sentence(1_000, 2_000, Some("speaker_10"), "二。"),
                    sentence(2_000, 3_000, Some("speaker_2"), "三。"),
                ],
            ),
        ];
        let (text, segments, language) = merge_segments(&plan, chunks).unwrap();
        assert_eq!(text, "一。二。三。\n乙。甲。旁白。");
        assert_eq!(language.as_deref(), Some("zh"));
        let speakers: Vec<_> = segments
            .iter()
            .map(|segment| segment.speaker.as_deref())
            .collect();
        assert_eq!(
            speakers,
            vec![
                Some("speaker_0"),
                Some("speaker_2"),
                Some("speaker_1"),
                Some("speaker_4"),
                Some("speaker_3"),
                None,
            ]
        );
        assert_eq!(segments[3].start_ms, 60_000);
        assert_eq!(segments[5].end_ms, 65_000);
    }

    #[test]
    fn merge_rejects_missing_segments() {
        let plan = vec![planned(0, 0, 60_000), planned(1, 60_000, 120_000)];
        let chunks = vec![chunk(0, 0, 60_000, vec![sentence(0, 1_000, None, "一。")])];
        assert!(merge_segments(&plan, chunks).is_err());
    }

    #[test]
    fn rejects_sentences_past_the_segment_length() {
        let segment = planned(1, 60_000, 120_000);
        assert!(validate_segment_timestamps(&segment, &[sentence(0, 62_000, None, "")]).is_ok());
        assert!(validate_segment_timestamps(&segment, &[sentence(0, 62_001, None, "")]).is_err());
    }

    #[test]
    fn checkpoint_round_trips_and_rejects_other_versions() {
        assert!(SegmentedCheckpoint::from_json("{}").unwrap().is_none());
        let state = SegmentedCheckpoint {
            version: CHECKPOINT_VERSION,
            plan: vec![planned(0, 0, 60_000)],
            active_index: 0,
            active: dashscope::Checkpoint::default(),
        };
        let json = state.to_json().unwrap();
        assert_eq!(SegmentedCheckpoint::from_json(&json).unwrap(), Some(state));
        assert!(
            SegmentedCheckpoint::from_json(&json.replace("\"version\":2", "\"version\":1"))
                .is_err()
        );
    }
}
