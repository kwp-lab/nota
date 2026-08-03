use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CaptureTarget {
    pub id: String,
    pub kind: String,
    pub display_name: String,
    pub process_id: u32,
    pub executable_path: String,
    pub browser: bool,
    pub priority: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub direction: DeviceDirection,
    pub is_default_communications: bool,
    pub form_factor: String,
    pub active: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DeviceDirection {
    Render,
    Capture,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum DeviceSelection {
    FollowDefaultCommunications,
    Fixed { endpoint_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CaptureSelection {
    Process { target_id: String },
    System { device: DeviceSelection },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AecMode {
    Auto,
    On,
    Off,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRecordingRequest {
    pub capture: CaptureSelection,
    pub microphone: Option<DeviceSelection>,
    pub aec_mode: AecMode,
    pub output_directory: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RecordingState {
    Idle,
    Preparing,
    Recording,
    Paused,
    Interrupted,
    Recovering,
    Finalizing,
    Completed,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceStatus {
    pub healthy: bool,
    pub label: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingFault {
    pub component: String,
    pub code: String,
    pub recoverable: bool,
    pub user_message: String,
    pub occurred_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingSnapshot {
    pub session_id: Option<String>,
    pub state: RecordingState,
    pub started_at: Option<String>,
    pub active_duration_ms: u64,
    pub bytes_written: u64,
    pub output_path: Option<String>,
    pub system: SourceStatus,
    pub microphone: SourceStatus,
    pub aec_status: AecStatus,
    pub fault: Option<RecordingFault>,
}

impl Default for RecordingSnapshot {
    fn default() -> Self {
        Self {
            session_id: None,
            state: RecordingState::Idle,
            started_at: None,
            active_duration_ms: 0,
            bytes_written: 0,
            output_path: None,
            system: SourceStatus {
                healthy: false,
                label: "系统声音".into(),
                detail: None,
            },
            microphone: SourceStatus {
                healthy: false,
                label: "麦克风".into(),
                detail: None,
            },
            aec_status: AecStatus::Disabled,
            fault: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AecStatus {
    Disabled,
    Enabled,
    Converging,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingItem {
    pub id: String,
    pub title: String,
    pub path: String,
    pub created_at: String,
    pub duration_ms: u64,
    pub size_bytes: u64,
    pub recovered: bool,
    #[serde(default)]
    pub transcription: Option<TranscriptionSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LevelEvent {
    pub system: f32,
    pub microphone: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub output_directory: String,
    pub aec_mode: AecMode,
    pub microphone_enabled: bool,
    pub first_run_complete: bool,
    pub shortcuts_enabled: bool,
    pub toggle_shortcut: String,
    pub stop_shortcut: String,
    #[serde(default)]
    pub active_asr_provider_id: Option<String>,
    #[serde(default)]
    pub voiceprint_provider_id: Option<String>,
    #[serde(default)]
    pub auto_transcribe: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AsrProviderKind {
    FunAsr,
    OpenAiCompatible,
}

impl AsrProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FunAsr => "fun_asr",
            Self::OpenAiCompatible => "open_ai_compatible",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "fun_asr" => Self::FunAsr,
            _ => Self::OpenAiCompatible,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AsrProvider {
    pub id: String,
    pub name: String,
    pub kind: AsrProviderKind,
    pub base_url: String,
    pub model_id: String,
    pub has_api_key: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveAsrProviderRequest {
    pub id: Option<String>,
    pub name: String,
    pub kind: AsrProviderKind,
    pub base_url: String,
    pub model_id: String,
    pub api_key: AsrApiKeyUpdate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AsrProviderProbeRequest {
    pub id: Option<String>,
    pub kind: AsrProviderKind,
    pub base_url: String,
    pub model_id: String,
    pub api_key: AsrApiKeyUpdate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AsrApiKeyUpdate {
    Keep,
    Replace { value: String },
    Clear,
}

#[derive(Debug, Clone)]
pub struct AsrProviderCredentials {
    pub provider: AsrProvider,
    pub api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AsrModel {
    pub id: String,
    pub owned_by: Option<String>,
    pub ready: Option<bool>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AsrConnectionLevel {
    Success,
    Warning,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AsrConnectionTest {
    pub reachable: bool,
    pub level: AsrConnectionLevel,
    pub message: String,
    pub models: Vec<AsrModel>,
    pub device: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TranscriptionStatus {
    Queued,
    Preparing,
    Transcribing,
    Completed,
    Failed,
    Interrupted,
    Cancelled,
}

impl TranscriptionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Preparing => "preparing",
            Self::Transcribing => "transcribing",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "queued" => Self::Queued,
            "preparing" => Self::Preparing,
            "transcribing" => Self::Transcribing,
            "completed" => Self::Completed,
            "interrupted" => Self::Interrupted,
            "cancelled" => Self::Cancelled,
            _ => Self::Failed,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptionProtocol {
    LegacyChunks,
    NotaBatchV1,
}

impl TranscriptionProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LegacyChunks => "legacy_chunks",
            Self::NotaBatchV1 => "nota_batch_v1",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "nota_batch_v1" => Self::NotaBatchV1,
            _ => Self::LegacyChunks,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptionProgressPhase {
    Preparing,
    Uploading,
    Queued,
    Transcribing,
    Diarizing,
    Finalizing,
}

impl TranscriptionProgressPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preparing => "preparing",
            Self::Uploading => "uploading",
            Self::Queued => "queued",
            Self::Transcribing => "transcribing",
            Self::Diarizing => "diarizing",
            Self::Finalizing => "finalizing",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "preparing" => Some(Self::Preparing),
            "uploading" => Some(Self::Uploading),
            "queued" => Some(Self::Queued),
            "transcribing" => Some(Self::Transcribing),
            "diarizing" => Some(Self::Diarizing),
            "finalizing" => Some(Self::Finalizing),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptionProgressUnit {
    Bytes,
    Windows,
    Steps,
    Chunks,
}

impl TranscriptionProgressUnit {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bytes => "bytes",
            Self::Windows => "windows",
            Self::Steps => "steps",
            Self::Chunks => "chunks",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "bytes" => Some(Self::Bytes),
            "windows" => Some(Self::Windows),
            "steps" => Some(Self::Steps),
            "chunks" => Some(Self::Chunks),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionSummary {
    pub status: TranscriptionStatus,
    pub completed_chunks: u32,
    pub total_chunks: u32,
    pub provider_name: String,
    pub model_id: String,
    pub error_message: Option<String>,
    pub has_text: bool,
    pub protocol: TranscriptionProtocol,
    pub progress_phase: Option<TranscriptionProgressPhase>,
    pub progress_current: u64,
    pub progress_total: u64,
    pub progress_unit: Option<TranscriptionProgressUnit>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub speaker: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoredTranscriptionChunk {
    pub index: u32,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub segments: Vec<TranscriptSegment>,
    pub language: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptionExecution {
    pub protocol: TranscriptionProtocol,
    pub remote_job_id: Option<String>,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptDocument {
    pub recording_id: String,
    pub status: TranscriptionStatus,
    pub provider_name: String,
    pub model_id: String,
    pub language: Option<String>,
    pub text: String,
    pub segments: Vec<TranscriptSegment>,
    #[serde(default)]
    pub speaker_names: BTreeMap<String, String>,
    pub completed_chunks: u32,
    pub total_chunks: u32,
    pub error_message: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VoiceprintSample {
    pub id: String,
    pub participant_id: String,
    pub embedding_fingerprint: String,
    pub source_recording_id: Option<String>,
    pub source_recording_title: Option<String>,
    pub source_speaker: String,
    pub preview_start_ms: u64,
    pub preview_end_ms: u64,
    pub preview_available: bool,
    pub speech_duration_ms: u64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ParticipantProfile {
    pub id: String,
    pub display_name: String,
    pub created_at: String,
    pub updated_at: String,
    pub samples: Vec<VoiceprintSample>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerIdentificationCandidate {
    pub raw_speaker: String,
    pub total_speech_ms: u64,
    pub preview_start_ms: u64,
    pub preview_end_ms: u64,
    pub embedding_extracted: bool,
    pub error_message: Option<String>,
    pub suggested_participant_id: Option<String>,
    pub suggested_participant_name: Option<String>,
    pub match_score: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerIdentificationSession {
    pub id: String,
    pub recording_id: String,
    pub speaker_count: u32,
    pub voiceprint_count: u32,
    pub candidates: Vec<SpeakerIdentificationCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerIdentificationAssignment {
    pub raw_speaker: String,
    pub participant_id: Option<String>,
    pub new_display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionEvent {
    pub recording_id: String,
    pub summary: TranscriptionSummary,
}

#[cfg(test)]
mod tests {
    use super::{AecMode, CaptureSelection, DeviceSelection, StartRecordingRequest};
    use serde_json::json;

    #[test]
    fn process_start_request_accepts_frontend_camel_case_fields() {
        let value = json!({
            "capture": {
                "kind": "process",
                "targetId": "process:4242"
            },
            "microphone": {
                "kind": "followDefaultCommunications"
            },
            "aecMode": "auto",
            "outputDirectory": "C:\\Recordings"
        });

        let request: StartRecordingRequest = serde_json::from_value(value).unwrap();

        assert_eq!(
            request.capture,
            CaptureSelection::Process {
                target_id: "process:4242".into()
            }
        );
        assert_eq!(
            request.microphone,
            Some(DeviceSelection::FollowDefaultCommunications)
        );
        assert_eq!(request.aec_mode, AecMode::Auto);
    }

    #[test]
    fn fixed_device_selections_accept_frontend_camel_case_fields() {
        let value = json!({
            "capture": {
                "kind": "system",
                "device": {
                    "kind": "fixed",
                    "endpointId": "render-endpoint"
                }
            },
            "microphone": {
                "kind": "fixed",
                "endpointId": "capture-endpoint"
            },
            "aecMode": "on",
            "outputDirectory": "C:\\Recordings"
        });

        let request: StartRecordingRequest = serde_json::from_value(value).unwrap();

        assert_eq!(
            request.capture,
            CaptureSelection::System {
                device: DeviceSelection::Fixed {
                    endpoint_id: "render-endpoint".into()
                }
            }
        );
        assert_eq!(
            request.microphone,
            Some(DeviceSelection::Fixed {
                endpoint_id: "capture-endpoint".into()
            })
        );
    }
}
