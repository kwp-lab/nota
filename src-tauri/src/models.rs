use serde::{Deserialize, Serialize};

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
    pub consent_confirmed: bool,
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
    pub consent_template: String,
    pub first_run_complete: bool,
    #[serde(default)]
    pub recording_notice_acknowledged: bool,
    pub shortcuts_enabled: bool,
    pub toggle_shortcut: String,
    pub stop_shortcut: String,
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
            "outputDirectory": "C:\\Recordings",
            "consentConfirmed": true
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
            "outputDirectory": "C:\\Recordings",
            "consentConfirmed": true
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
