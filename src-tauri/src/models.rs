use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CaptureTarget {
    pub id: String,
    pub kind: String,
    pub display_name: String,
    pub process_id: u32,
    pub executable_path: String,
    #[serde(skip)]
    pub window_handle: Option<isize>,
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
    pub microphone_selection: Option<DeviceSelection>,
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
            microphone_selection: None,
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
    pub origin: RecordingOrigin,
    #[serde(default)]
    pub source_file_name: Option<String>,
    #[serde(default)]
    pub source_format: Option<String>,
    #[serde(default)]
    pub imported_at: Option<String>,
    #[serde(default)]
    pub transcription: Option<TranscriptionSummary>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecordingOrigin {
    #[default]
    Captured,
    Imported,
}

impl RecordingOrigin {
    pub fn from_str(value: &str) -> Self {
        match value {
            "imported" => Self::Imported,
            _ => Self::Captured,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AudioImportBatchStatus {
    Running,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AudioImportItemStatus {
    Queued,
    Probing,
    Decoding,
    Finalizing,
    Completed,
    Failed,
    Skipped,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioImportItemSnapshot {
    pub id: String,
    pub file_name: String,
    pub status: AudioImportItemStatus,
    pub progress_current_ms: u64,
    pub progress_total_ms: u64,
    pub error_message: Option<String>,
    pub recording_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioImportBatchSnapshot {
    pub id: String,
    pub status: AudioImportBatchStatus,
    pub current_index: u32,
    pub total: u32,
    pub completed: u32,
    pub failed: u32,
    pub skipped: u32,
    pub items: Vec<AudioImportItemSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LevelEvent {
    pub system: f32,
    pub microphone: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CapturePromptKind {
    CaptureInterrupted,
    ProlongedSilence,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CapturePrompt {
    pub session_id: String,
    pub target_name: String,
    pub kind: CapturePromptKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub output_directory: String,
    #[serde(default)]
    pub ai_documents_directory: String,
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
    #[serde(default)]
    pub active_llm_provider_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LlmProviderKind {
    OpenAi,
    OpenAiCompatible,
}

impl LlmProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenAi => "open_ai",
            Self::OpenAiCompatible => "open_ai_compatible",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "open_ai" => Self::OpenAi,
            _ => Self::OpenAiCompatible,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LlmProvider {
    pub id: String,
    pub name: String,
    pub kind: LlmProviderKind,
    pub base_url: String,
    pub model_id: String,
    pub input_token_budget: u32,
    pub max_output_tokens: u32,
    pub has_api_key: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveLlmProviderRequest {
    pub id: Option<String>,
    pub name: String,
    pub kind: LlmProviderKind,
    pub base_url: String,
    pub model_id: String,
    pub input_token_budget: u32,
    pub max_output_tokens: u32,
    pub api_key: AsrApiKeyUpdate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmProviderProbeRequest {
    pub id: Option<String>,
    pub kind: LlmProviderKind,
    pub base_url: String,
    pub model_id: String,
    pub input_token_budget: u32,
    pub max_output_tokens: u32,
    pub api_key: AsrApiKeyUpdate,
}

#[derive(Debug, Clone)]
pub struct LlmProviderCredentials {
    pub provider: LlmProvider,
    pub api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LlmModel {
    pub id: String,
    pub owned_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LlmConnectionTest {
    pub reachable: bool,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AsrProviderKind {
    FunAsr,
    OpenAiCompatible,
    DashScope,
}

impl AsrProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FunAsr => "fun_asr",
            Self::OpenAiCompatible => "open_ai_compatible",
            Self::DashScope => "dash_scope",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "fun_asr" => Self::FunAsr,
            "dash_scope" => Self::DashScope,
            _ => Self::OpenAiCompatible,
        }
    }

    pub fn capabilities(self) -> AsrProviderCapabilities {
        match self {
            Self::FunAsr => AsrProviderCapabilities {
                whole_meeting: true,
                diarization: true,
                speaker_count_min: Some(1),
                speaker_count_max: Some(64),
                voiceprint_analysis: true,
                model_discovery: true,
                cloud_upload: false,
                max_reliable_audio_seconds: None,
            },
            Self::OpenAiCompatible => AsrProviderCapabilities {
                whole_meeting: false,
                diarization: false,
                speaker_count_min: None,
                speaker_count_max: None,
                // Preserve the existing optional FunASR voiceprint workflow
                // when a compatible transcript happens to include speakers.
                voiceprint_analysis: true,
                model_discovery: true,
                cloud_upload: false,
                max_reliable_audio_seconds: None,
            },
            Self::DashScope => AsrProviderCapabilities {
                whole_meeting: true,
                diarization: true,
                speaker_count_min: Some(2),
                speaker_count_max: Some(100),
                voiceprint_analysis: false,
                model_discovery: false,
                cloud_upload: true,
                max_reliable_audio_seconds: Some(2 * 60 * 60),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AsrProviderCapabilities {
    pub whole_meeting: bool,
    pub diarization: bool,
    pub speaker_count_min: Option<u32>,
    pub speaker_count_max: Option<u32>,
    pub voiceprint_analysis: bool,
    pub model_discovery: bool,
    pub cloud_upload: bool,
    pub max_reliable_audio_seconds: Option<u64>,
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
    pub capabilities: AsrProviderCapabilities,
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
    DashScopeFileTransV1,
}

impl TranscriptionProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LegacyChunks => "legacy_chunks",
            Self::NotaBatchV1 => "nota_batch_v1",
            Self::DashScopeFileTransV1 => "dashscope_filetrans_v1",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "nota_batch_v1" => Self::NotaBatchV1,
            "dashscope_filetrans_v1" => Self::DashScopeFileTransV1,
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
    pub provider_kind: AsrProviderKind,
    pub model_id: String,
    pub speaker_count: Option<u32>,
    pub error_message: Option<String>,
    pub has_text: bool,
    pub protocol: TranscriptionProtocol,
    pub voiceprint_analysis_supported: bool,
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
    pub provider_kind: AsrProviderKind,
    pub remote_job_id: Option<String>,
    pub idempotency_key: String,
    pub speaker_count: Option<u32>,
    pub provider_state_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptDocument {
    pub recording_id: String,
    pub status: TranscriptionStatus,
    pub provider_name: String,
    pub provider_kind: AsrProviderKind,
    pub model_id: String,
    pub protocol: TranscriptionProtocol,
    pub voiceprint_analysis_supported: bool,
    pub language: Option<String>,
    pub text: String,
    pub segments: Vec<TranscriptSegment>,
    #[serde(default)]
    pub speaker_names: BTreeMap<String, String>,
    #[serde(default)]
    pub speaker_assignments: BTreeMap<String, RecordingSpeakerAssignment>,
    pub completed_chunks: u32,
    pub total_chunks: u32,
    pub error_message: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecordingSpeakerAssignment {
    pub participant_id: String,
    pub display_name: String,
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
    pub sample_status: SpeakerSampleStatus,
    pub status_message: Option<String>,
    pub error_message: Option<String>,
    pub suggested_participant_id: Option<String>,
    pub suggested_participant_name: Option<String>,
    pub match_score: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SpeakerSampleStatus {
    Enrollable,
    PreviewOnly,
    Unavailable,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiTemplate {
    pub id: String,
    pub name: String,
    pub description: String,
    pub builtin_key: Option<String>,
    pub task_instructions: String,
    pub output_requirements: String,
    pub requires_speaker_labels: bool,
    pub revision: u32,
    pub archived: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveAiTemplateRequest {
    pub id: Option<String>,
    pub name: String,
    pub description: String,
    pub task_instructions: String,
    pub output_requirements: String,
    pub requires_speaker_labels: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiMeetingProfile {
    pub recording_id: String,
    pub workspace_path: String,
    pub meeting_context: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AiGenerationMode {
    Create,
    Regenerate,
    Revise,
}

impl AiGenerationMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Regenerate => "regenerate",
            Self::Revise => "revise",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "regenerate" => Self::Regenerate,
            "revise" => Self::Revise,
            _ => Self::Create,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AiGenerationStatus {
    Queued,
    Generating,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
}

impl AiGenerationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Generating => "generating",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "queued" => Self::Queued,
            "generating" => Self::Generating,
            "completed" => Self::Completed,
            "cancelled" => Self::Cancelled,
            "interrupted" => Self::Interrupted,
            _ => Self::Failed,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AiFileState {
    Pending,
    Ready,
    Modified,
    Missing,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiDocumentVersion {
    pub id: String,
    pub document_id: String,
    pub version_number: u32,
    pub mode: AiGenerationMode,
    pub parent_version_id: Option<String>,
    pub status: AiGenerationStatus,
    pub file_path: Option<String>,
    pub file_state: AiFileState,
    pub provider_name: String,
    pub provider_kind: LlmProviderKind,
    pub model_id: String,
    pub template_name: String,
    pub template_revision: u32,
    pub transcription_generation: u32,
    pub estimated_input_tokens: u32,
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiDocument {
    pub id: String,
    pub recording_id: String,
    pub template_id: String,
    pub title: String,
    pub requirements: String,
    pub template_name: String,
    pub template_builtin_key: Option<String>,
    pub latest_version: Option<AiDocumentVersion>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiWorkspace {
    pub profile: AiMeetingProfile,
    pub documents: Vec<AiDocument>,
    pub templates: Vec<AiTemplate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiGenerationRequest {
    pub recording_id: String,
    pub mode: AiGenerationMode,
    pub document_id: Option<String>,
    pub template_id: Option<String>,
    pub title: Option<String>,
    pub meeting_context: String,
    pub document_requirements: String,
    pub run_request: String,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub source_version_id: Option<String>,
    #[serde(default)]
    pub estimated_input_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiGenerationRequestPreview {
    pub provider_kind: LlmProviderKind,
    pub request_body: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiGenerationDetails {
    pub version_id: String,
    pub request_body: Option<Value>,
    pub response_body: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiDocumentContent {
    pub version: AiDocumentVersion,
    pub markdown: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiGenerationEvent {
    pub recording_id: String,
    pub document_id: String,
    pub version: AiDocumentVersion,
}

#[cfg(test)]
mod tests {
    use super::{
        AecMode, CapturePrompt, CapturePromptKind, CaptureSelection, DeviceSelection,
        RecordingSnapshot, StartRecordingRequest,
    };
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

    #[test]
    fn recording_snapshot_exposes_the_authoritative_microphone_selection() {
        let snapshot = RecordingSnapshot {
            microphone_selection: Some(DeviceSelection::Fixed {
                endpoint_id: "capture-endpoint".into(),
            }),
            ..RecordingSnapshot::default()
        };

        let value = serde_json::to_value(snapshot).unwrap();

        assert_eq!(value["microphoneSelection"]["kind"], "fixed");
        assert_eq!(
            value["microphoneSelection"]["endpointId"],
            "capture-endpoint"
        );
    }

    #[test]
    fn capture_prompt_kind_uses_frontend_camel_case_values() {
        let prompt = CapturePrompt {
            session_id: "session-1".into(),
            target_name: "Meeting".into(),
            kind: CapturePromptKind::ProlongedSilence,
        };

        let value = serde_json::to_value(prompt).unwrap();

        assert_eq!(value["sessionId"], "session-1");
        assert_eq!(value["kind"], "prolongedSilence");
    }
}
