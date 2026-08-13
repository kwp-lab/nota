use anyhow::Result;
use log::{Level, LevelFilter, Log, Metadata, Record};
use parking_lot::Mutex;
use std::borrow::Cow;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const MAX_LOG_BYTES: u64 = 10 * 1024 * 1024;
const LOG_FILES: usize = 3;
const MAX_TEXT_FIELD_CHARS: usize = 160;
const MAX_LEGACY_MESSAGE_CHARS: usize = 2_048;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKey {
    AppVersion,
    SessionId,
    RecordingId,
    VersionId,
    DocumentId,
    BatchId,
    ProviderId,
    ProviderKind,
    ModelId,
    Generation,
    Protocol,
    Mode,
    Status,
    Phase,
    Action,
    Reason,
    ErrorCode,
    HttpStatus,
    DurationMs,
    Count,
    Bytes,
    ProcessId,
    RootProcessId,
    SourceEpoch,
    Executable,
    InputTokens,
    OutputTokens,
    EstimatedInputTokens,
    RetryAttempt,
    Packets,
    NonSilentPackets,
    Underflows,
    Discontinuities,
    ConcealedSamples,
    SampleRateHz,
    Channels,
    BitsPerSample,
    ValidBitsPerSample,
    BlockAlign,
    Current,
    Total,
    Paused,
    Scope,
    Permanent,
    IncludeAiDocuments,
}

impl FieldKey {
    fn as_str(self) -> &'static str {
        match self {
            Self::AppVersion => "app_version",
            Self::SessionId => "session_id",
            Self::RecordingId => "recording_id",
            Self::VersionId => "version_id",
            Self::DocumentId => "document_id",
            Self::BatchId => "batch_id",
            Self::ProviderId => "provider_id",
            Self::ProviderKind => "provider_kind",
            Self::ModelId => "model_id",
            Self::Generation => "generation",
            Self::Protocol => "protocol",
            Self::Mode => "mode",
            Self::Status => "status",
            Self::Phase => "phase",
            Self::Action => "action",
            Self::Reason => "reason",
            Self::ErrorCode => "error_code",
            Self::HttpStatus => "http_status",
            Self::DurationMs => "duration_ms",
            Self::Count => "count",
            Self::Bytes => "bytes",
            Self::ProcessId => "process_id",
            Self::RootProcessId => "root_process_id",
            Self::SourceEpoch => "source_epoch",
            Self::Executable => "executable",
            Self::InputTokens => "input_tokens",
            Self::OutputTokens => "output_tokens",
            Self::EstimatedInputTokens => "estimated_input_tokens",
            Self::RetryAttempt => "retry_attempt",
            Self::Packets => "packets",
            Self::NonSilentPackets => "non_silent_packets",
            Self::Underflows => "underflows",
            Self::Discontinuities => "discontinuities",
            Self::ConcealedSamples => "concealed_samples",
            Self::SampleRateHz => "sample_rate_hz",
            Self::Channels => "channels",
            Self::BitsPerSample => "bits_per_sample",
            Self::ValidBitsPerSample => "valid_bits_per_sample",
            Self::BlockAlign => "block_align",
            Self::Current => "current",
            Self::Total => "total",
            Self::Paused => "paused",
            Self::Scope => "scope",
            Self::Permanent => "permanent",
            Self::IncludeAiDocuments => "include_ai_documents",
        }
    }
}

#[derive(Debug, Clone)]
enum FieldValue<'a> {
    Text(Cow<'a, str>),
    Number(u64),
    Boolean(bool),
}

#[derive(Debug, Clone)]
pub struct Field<'a> {
    key: FieldKey,
    value: FieldValue<'a>,
}

impl<'a> Field<'a> {
    pub fn text(key: FieldKey, value: impl Into<Cow<'a, str>>) -> Self {
        Self {
            key,
            value: FieldValue::Text(value.into()),
        }
    }

    pub fn number(key: FieldKey, value: impl Into<u64>) -> Self {
        Self {
            key,
            value: FieldValue::Number(value.into()),
        }
    }

    pub fn boolean(key: FieldKey, value: bool) -> Self {
        Self {
            key,
            value: FieldValue::Boolean(value),
        }
    }
}

struct RollingLogger {
    directory: PathBuf,
    file: Mutex<File>,
}

impl Log for RollingLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let mut file = self.file.lock();
        if file
            .metadata()
            .is_ok_and(|metadata| metadata.len() >= MAX_LOG_BYTES)
            && let Ok(replacement) = rotate(&self.directory)
        {
            *file = replacement;
        }
        let message = single_line(&record.args().to_string(), MAX_LEGACY_MESSAGE_CHARS);
        let component = single_line(record.target(), MAX_TEXT_FIELD_CHARS);
        let _ = writeln!(
            file,
            "{}",
            format_log_line(chrono::Utc::now(), record.level(), &component, &message)
        );
    }

    fn flush(&self) {
        let _ = self.file.lock().flush();
    }
}

pub fn init(directory: &Path) -> Result<()> {
    std::fs::create_dir_all(directory)?;
    let file = open_current(directory)?;
    let logger = RollingLogger {
        directory: directory.to_path_buf(),
        file: Mutex::new(file),
    };
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(LevelFilter::Info);
    }
    Ok(())
}

pub fn info(component: &'static str, event: &'static str, fields: &[Field<'_>]) {
    emit(Level::Info, component, event, fields);
}

pub fn warn(component: &'static str, event: &'static str, fields: &[Field<'_>]) {
    emit(Level::Warn, component, event, fields);
}

pub fn error(component: &'static str, event: &'static str, fields: &[Field<'_>]) {
    emit(Level::Error, component, event, fields);
}

fn emit(level: Level, component: &'static str, event: &'static str, fields: &[Field<'_>]) {
    let line = format_event(event, fields);
    log::log!(target: component, level, "{line}");
}

fn format_event(event: &str, fields: &[Field<'_>]) -> String {
    let mut line = format!(
        "event={}",
        encode_text(&single_line(event, MAX_TEXT_FIELD_CHARS))
    );
    for field in fields {
        line.push(' ');
        line.push_str(field.key.as_str());
        line.push('=');
        match &field.value {
            FieldValue::Text(value) => {
                line.push_str(&encode_text(&single_line(value, MAX_TEXT_FIELD_CHARS)));
            }
            FieldValue::Number(value) => line.push_str(&value.to_string()),
            FieldValue::Boolean(value) => line.push_str(if *value { "true" } else { "false" }),
        }
    }
    line
}

fn format_log_line(
    timestamp: chrono::DateTime<chrono::Utc>,
    level: Level,
    component: &str,
    message: &str,
) -> String {
    format!(
        "{} {level:<5} component={} {}",
        timestamp.to_rfc3339(),
        encode_text(component),
        message
    )
}

fn single_line(value: &str, max_chars: usize) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .take(max_chars)
        .collect::<String>()
        .trim()
        .to_owned()
}

fn encode_text(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"invalid\"".into())
}

fn open_current(directory: &Path) -> Result<File> {
    Ok(OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("nota.log"))?)
}

fn rotate(directory: &Path) -> Result<File> {
    let oldest = directory.join(format!("nota.{}.log", LOG_FILES - 1));
    if oldest.exists() {
        std::fs::remove_file(oldest)?;
    }
    for index in (1..LOG_FILES - 1).rev() {
        let source = directory.join(format!("nota.{index}.log"));
        let destination = directory.join(format!("nota.{}.log", index + 1));
        if source.exists() {
            std::fs::rename(source, destination)?;
        }
    }
    let current = directory.join("nota.log");
    if current.exists() {
        std::fs::rename(current, directory.join("nota.1.log"))?;
    }
    open_current(directory)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_events_are_single_line_bounded_and_machine_greppable() {
        let long_model = format!("model\n{}", "x".repeat(300));
        let line = format_event(
            "request\r\ndispatched",
            &[
                Field::text(FieldKey::VersionId, "version-1"),
                Field::text(FieldKey::ModelId, long_model),
                Field::number(FieldKey::EstimatedInputTokens, 42u64),
                Field::boolean(FieldKey::Paused, true),
            ],
        );

        assert!(!line.contains('\n'));
        assert!(!line.contains('\r'));
        assert!(line.starts_with("event=\"request  dispatched\""));
        assert!(line.contains("version_id=\"version-1\""));
        assert!(line.contains("estimated_input_tokens=42"));
        assert!(line.contains("paused=true"));
        assert!(line.len() < 500);
    }

    #[test]
    fn diagnostic_field_keys_cannot_name_sensitive_payloads() {
        let keys = [
            FieldKey::AppVersion,
            FieldKey::SessionId,
            FieldKey::RecordingId,
            FieldKey::VersionId,
            FieldKey::DocumentId,
            FieldKey::BatchId,
            FieldKey::ProviderId,
            FieldKey::ProviderKind,
            FieldKey::ModelId,
            FieldKey::Generation,
            FieldKey::Protocol,
            FieldKey::Mode,
            FieldKey::Status,
            FieldKey::Phase,
            FieldKey::Action,
            FieldKey::Reason,
            FieldKey::ErrorCode,
            FieldKey::HttpStatus,
            FieldKey::DurationMs,
            FieldKey::Count,
            FieldKey::Bytes,
            FieldKey::ProcessId,
            FieldKey::RootProcessId,
            FieldKey::SourceEpoch,
            FieldKey::Executable,
            FieldKey::InputTokens,
            FieldKey::OutputTokens,
            FieldKey::EstimatedInputTokens,
            FieldKey::RetryAttempt,
            FieldKey::Packets,
            FieldKey::NonSilentPackets,
            FieldKey::Underflows,
            FieldKey::Discontinuities,
            FieldKey::ConcealedSamples,
            FieldKey::SampleRateHz,
            FieldKey::Channels,
            FieldKey::BitsPerSample,
            FieldKey::ValidBitsPerSample,
            FieldKey::BlockAlign,
            FieldKey::Current,
            FieldKey::Total,
            FieldKey::Paused,
            FieldKey::Scope,
            FieldKey::Permanent,
            FieldKey::IncludeAiDocuments,
        ];
        let forbidden = [
            "api_key",
            "authorization",
            "audio",
            "transcript",
            "request_body",
            "response_body",
            "path",
            "url",
            "title",
        ];

        for key in keys {
            assert!(!forbidden.contains(&key.as_str()));
        }
    }

    #[test]
    fn log_lines_use_utc_rfc3339_timestamps() {
        let timestamp = chrono::DateTime::parse_from_rfc3339("2026-08-12T08:30:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let line = format_log_line(timestamp, Level::Info, "llm", "event=\"request_sent\"");

        assert!(line.starts_with("2026-08-12T08:30:00+00:00 INFO  component=\"llm\""));
        let parsed =
            chrono::DateTime::parse_from_rfc3339(line.split_whitespace().next().unwrap()).unwrap();
        assert_eq!(parsed.offset().local_minus_utc(), 0);
    }

    #[test]
    fn rotation_keeps_current_and_two_archives() {
        let directory =
            std::env::temp_dir().join(format!("nota-log-rotation-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("nota.log"), "current").unwrap();
        std::fs::write(directory.join("nota.1.log"), "previous").unwrap();
        std::fs::write(directory.join("nota.2.log"), "oldest").unwrap();

        drop(rotate(&directory).unwrap());

        assert_eq!(
            std::fs::read_to_string(directory.join("nota.1.log")).unwrap(),
            "current"
        );
        assert_eq!(
            std::fs::read_to_string(directory.join("nota.2.log")).unwrap(),
            "previous"
        );
        assert!(directory.join("nota.log").is_file());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
