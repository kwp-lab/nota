use super::ProviderTranscript;
use crate::models::{AsrConnectionLevel, AsrConnectionTest, AsrModel, AsrProviderCredentials};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use reqwest::StatusCode;
use reqwest::blocking::multipart::{Form, Part};
use reqwest::blocking::{Client, Response};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs::File;
use std::io::{Read, Take};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub(super) const API_ROOT: &str = "https://dashscope.aliyuncs.com/api/v1";
pub(super) const MODEL: &str = "qwen-audio-3.0-asr-flash-filetrans";
const MAX_CONTROL_RESPONSE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_RESULT_RESPONSE_BYTES: u64 = 64 * 1024 * 1024;
const TEMPORARY_OBJECT_HOURS: i64 = 48;

pub(super) struct DashScopeAdapter;

impl DashScopeAdapter {
    pub(super) fn validate_credentials(credentials: &AsrProviderCredentials) -> Result<()> {
        validate_credentials(credentials)
    }

    pub(super) fn models() -> Vec<AsrModel> {
        models()
    }

    pub(super) fn test_connection(
        credentials: &AsrProviderCredentials,
    ) -> Result<AsrConnectionTest> {
        test_connection(credentials)
    }

    pub(super) fn upload_audio(
        credentials: &AsrProviderCredentials,
        audio_path: &Path,
        on_progress: Arc<dyn Fn(u64) + Send + Sync>,
        cancelled: Arc<AtomicBool>,
    ) -> Result<UploadedAudio> {
        DashScopeAudioUploader::upload(credentials, audio_path, on_progress, cancelled)
    }

    pub(super) fn submit_task(
        credentials: &AsrProviderCredentials,
        oss_url: &str,
        speaker_count: Option<u32>,
    ) -> Result<String> {
        submit_task(credentials, oss_url, speaker_count)
    }

    pub(super) fn get_task(
        credentials: &AsrProviderCredentials,
        task_id: &str,
    ) -> Result<TaskStatus> {
        get_task(credentials, task_id)
    }

    pub(super) fn cancel_pending_task(
        credentials: &AsrProviderCredentials,
        task_id: &str,
    ) -> Result<()> {
        cancel_pending_task(credentials, task_id)
    }

    pub(super) fn download_result(result_url: &str) -> Result<ProviderTranscript> {
        download_result(result_url)
    }
}

struct DashScopeAudioUploader;

impl DashScopeAudioUploader {
    fn upload(
        credentials: &AsrProviderCredentials,
        audio_path: &Path,
        on_progress: Arc<dyn Fn(u64) + Send + Sync>,
        cancelled: Arc<AtomicBool>,
    ) -> Result<UploadedAudio> {
        upload_audio(credentials, audio_path, on_progress, cancelled)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct Checkpoint {
    pub version: u32,
    pub stage: Stage,
    pub oss_url: Option<String>,
    pub oss_expires_at: Option<String>,
    pub submit_attempted_at: Option<String>,
}

impl Default for Checkpoint {
    fn default() -> Self {
        Self {
            version: 1,
            stage: Stage::New,
            oss_url: None,
            oss_expires_at: None,
            submit_attempted_at: None,
        }
    }
}

impl Checkpoint {
    pub(super) fn from_json(value: &str) -> Result<Self> {
        if value.trim().is_empty() || value.trim() == "{}" {
            return Ok(Self::default());
        }
        let checkpoint: Self =
            serde_json::from_str(value).context("无法读取 DashScope 恢复状态")?;
        if checkpoint.version != 1 {
            bail!("DashScope 恢复状态版本不受支持");
        }
        Ok(checkpoint)
    }

    pub(super) fn to_json(&self) -> Result<String> {
        serde_json::to_string(self).context("无法保存 DashScope 恢复状态")
    }

    pub(super) fn uploaded_object_is_valid(&self) -> bool {
        self.oss_url
            .as_deref()
            .is_some_and(|value| value.starts_with("oss://"))
            && self
                .oss_expires_at
                .as_deref()
                .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
                .is_some_and(|value| {
                    value.with_timezone(&Utc) > Utc::now() + ChronoDuration::minutes(5)
                })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Stage {
    New,
    Uploading,
    Uploaded,
    Submitting,
    Submitted,
    Pending,
    Running,
    Downloading,
    Normalizing,
}

#[derive(Debug, Clone)]
pub(super) struct UploadedAudio {
    pub oss_url: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TaskState {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Unknown,
}

#[derive(Debug, Clone)]
pub(super) struct TaskStatus {
    pub state: TaskState,
    pub result_url: Option<String>,
    pub error_code: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UploadPolicy {
    policy: String,
    signature: String,
    upload_dir: String,
    upload_host: String,
    oss_access_key_id: String,
    x_oss_object_acl: String,
    x_oss_forbid_overwrite: String,
    #[serde(default)]
    max_file_size_mb: Option<Value>,
}

struct ProgressReader {
    inner: File,
    completed: u64,
    on_progress: Arc<dyn Fn(u64) + Send + Sync>,
    cancelled: Arc<AtomicBool>,
}

impl Read for ProgressReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "DashScope upload cancelled",
            ));
        }
        let read = self.inner.read(buffer)?;
        self.completed = self.completed.saturating_add(read as u64);
        (self.on_progress)(self.completed);
        Ok(read)
    }
}

pub(super) fn validate_credentials(credentials: &AsrProviderCredentials) -> Result<()> {
    if credentials.provider.base_url.trim_end_matches('/') != API_ROOT {
        bail!("DashScope 第一版仅支持中国区公共端点 {API_ROOT}");
    }
    if credentials.provider.model_id != MODEL {
        bail!("DashScope 第一版仅支持模型 {MODEL}");
    }
    if credentials.api_key.trim().is_empty() {
        bail!("请先填写 DashScope API Key");
    }
    Ok(())
}

pub(super) fn models() -> Vec<AsrModel> {
    vec![AsrModel {
        id: MODEL.into(),
        owned_by: Some("Alibaba Cloud".into()),
        ready: Some(true),
    }]
}

pub(super) fn test_connection(credentials: &AsrProviderCredentials) -> Result<AsrConnectionTest> {
    validate_credentials(credentials)?;
    let _ = get_upload_policy(credentials)?;
    Ok(AsrConnectionTest {
        reachable: true,
        level: AsrConnectionLevel::Success,
        message: "服务可用，已成功获取模型绑定的临时上传凭证；未上传音频，也未创建计费任务".into(),
        models: models(),
        device: None,
    })
}

pub(super) fn upload_audio(
    credentials: &AsrProviderCredentials,
    audio_path: &Path,
    on_progress: Arc<dyn Fn(u64) + Send + Sync>,
    cancelled: Arc<AtomicBool>,
) -> Result<UploadedAudio> {
    validate_credentials(credentials)?;
    if audio_path.extension().and_then(|value| value.to_str()) != Some("ogg") {
        bail!("DashScope 文件转写仅上传 Nota 的原始 Ogg 录音");
    }
    let policy = get_upload_policy(credentials)?;
    validate_aliyun_https_url(&policy.upload_host, "临时上传地址")?;
    let size = std::fs::metadata(audio_path)?.len();
    if let Some(max_mb) = policy.max_file_size_mb.as_ref().and_then(value_as_f64) {
        let max_bytes = (max_mb * 1024.0 * 1024.0) as u64;
        if size > max_bytes {
            bail!("录音文件超过 DashScope 临时上传限制");
        }
    }
    let file_name = audio_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("nota-recording.ogg");
    let object_key = format!("{}/{}", policy.upload_dir.trim_end_matches('/'), file_name);
    let reader = ProgressReader {
        inner: File::open(audio_path)?,
        completed: 0,
        on_progress,
        cancelled,
    };
    let file = Part::reader_with_length(reader, size)
        .file_name(file_name.to_owned())
        .mime_str("audio/ogg")?;
    // OSS validates the multipart sequence; keep the audio part last.
    let form = Form::new()
        .text("OSSAccessKeyId", policy.oss_access_key_id)
        .text("Signature", policy.signature)
        .text("policy", policy.policy)
        .text("x-oss-object-acl", policy.x_oss_object_acl)
        .text("x-oss-forbid-overwrite", policy.x_oss_forbid_overwrite)
        .text("key", object_key.clone())
        .text("success_action_status", "200")
        .part("file", file);
    let response = client()?
        .post(&policy.upload_host)
        .timeout(Duration::from_secs(30 * 60))
        .multipart(form)
        .send()
        .map_err(|_| anyhow::anyhow!("无法上传录音至 DashScope 临时存储"))?;
    if response.status() != StatusCode::OK {
        bail!(
            "DashScope 临时上传失败（HTTP {}）",
            response.status().as_u16()
        );
    }
    Ok(UploadedAudio {
        oss_url: format!("oss://{object_key}"),
        expires_at: (Utc::now() + ChronoDuration::hours(TEMPORARY_OBJECT_HOURS)).to_rfc3339(),
    })
}

pub(super) fn submit_task(
    credentials: &AsrProviderCredentials,
    oss_url: &str,
    speaker_count: Option<u32>,
) -> Result<String> {
    validate_credentials(credentials)?;
    if !oss_url.starts_with("oss://") {
        bail!("DashScope 临时对象地址无效");
    }
    let response = build_submit_request(&client()?, credentials, oss_url, speaker_count)
        .send()
        .map_err(|_| {
            anyhow::anyhow!("DashScope 任务提交结果未知；为避免重复计费，Nota 不会自动重试")
        })?;
    let value = read_json(
        response,
        "提交 DashScope 转写任务",
        MAX_CONTROL_RESPONSE_BYTES,
    )?;
    value
        .pointer("/output/task_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .context("DashScope 接受请求但没有返回 task_id；为避免重复计费，Nota 不会自动重试")
}

fn build_submit_request(
    client: &Client,
    credentials: &AsrProviderCredentials,
    oss_url: &str,
    speaker_count: Option<u32>,
) -> reqwest::blocking::RequestBuilder {
    let mut parameters = json!({
        "channel_id": [0],
        "diarization_enabled": true
    });
    if let Some(count) = speaker_count {
        parameters["speaker_count"] = json!(count);
    }
    client
        .post(format!("{API_ROOT}/services/audio/asr/transcription"))
        .timeout(Duration::from_secs(120))
        .bearer_auth(&credentials.api_key)
        .header("X-DashScope-Async", "enable")
        .header("X-DashScope-OssResourceResolve", "enable")
        .json(&json!({
            "model": MODEL,
            "input": { "file_urls": [oss_url] },
            "parameters": parameters
        }))
}

pub(super) fn get_task(credentials: &AsrProviderCredentials, task_id: &str) -> Result<TaskStatus> {
    validate_credentials(credentials)?;
    let response = client()?
        .get(format!("{API_ROOT}/tasks/{task_id}"))
        .timeout(Duration::from_secs(60))
        .bearer_auth(&credentials.api_key)
        .send()
        .map_err(|_| anyhow::anyhow!("无法查询 DashScope 转写任务"))?;
    let value = read_json(
        response,
        "查询 DashScope 转写任务",
        MAX_CONTROL_RESPONSE_BYTES,
    )?;
    let status = value
        .pointer("/output/task_status")
        .and_then(Value::as_str)
        .context("DashScope 任务响应缺少 task_status")?;
    let state = match status.to_ascii_uppercase().as_str() {
        "PENDING" | "WAITING" => TaskState::Pending,
        "RUNNING" => TaskState::Running,
        "SUCCEEDED" => TaskState::Succeeded,
        "FAILED" => TaskState::Failed,
        "CANCELED" | "CANCELLED" => TaskState::Cancelled,
        _ => TaskState::Unknown,
    };
    let result_url = if state == TaskState::Succeeded {
        successful_result_url(&value)?
    } else {
        None
    };
    Ok(TaskStatus {
        state,
        result_url,
        error_code: value
            .pointer("/output/code")
            .and_then(Value::as_str)
            .map(safe_code),
    })
}

pub(super) fn cancel_pending_task(
    credentials: &AsrProviderCredentials,
    task_id: &str,
) -> Result<()> {
    validate_credentials(credentials)?;
    let response = client()?
        .post(format!("{API_ROOT}/tasks/{task_id}/cancel"))
        .timeout(Duration::from_secs(60))
        .bearer_auth(&credentials.api_key)
        .send()
        .map_err(|_| anyhow::anyhow!("无法请求取消 DashScope 排队任务"))?;
    let _ = read_json(
        response,
        "取消 DashScope 排队任务",
        MAX_CONTROL_RESPONSE_BYTES,
    )?;
    Ok(())
}

pub(super) fn download_result(result_url: &str) -> Result<ProviderTranscript> {
    validate_aliyun_https_url(result_url, "转写结果地址")?;
    let response = client()?
        .get(result_url)
        .timeout(Duration::from_secs(5 * 60))
        .send()
        .map_err(|_| anyhow::anyhow!("无法下载 DashScope 转写结果"))?;
    let value = read_json(
        response,
        "下载 DashScope 转写结果",
        MAX_RESULT_RESPONSE_BYTES,
    )?;
    normalize_result(&value)
}

fn get_upload_policy(credentials: &AsrProviderCredentials) -> Result<UploadPolicy> {
    let response = client()?
        .get(format!("{API_ROOT}/uploads"))
        .query(&[("action", "getPolicy"), ("model", MODEL)])
        .timeout(Duration::from_secs(60))
        .bearer_auth(&credentials.api_key)
        .send()
        .map_err(|_| anyhow::anyhow!("无法获取 DashScope 临时上传凭证"))?;
    let value = read_json(
        response,
        "获取 DashScope 临时上传凭证",
        MAX_CONTROL_RESPONSE_BYTES,
    )?;
    let policy: UploadPolicy = serde_json::from_value(
        value
            .get("data")
            .cloned()
            .context("DashScope 上传凭证缺少 data")?,
    )
    .context("DashScope 上传凭证字段不完整")?;
    validate_aliyun_https_url(&policy.upload_host, "临时上传地址")?;
    Ok(policy)
}

fn client() -> Result<Client> {
    Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .build()
        .context("无法初始化 DashScope HTTP 客户端")
}

fn read_json(response: Response, operation: &str, max_bytes: u64) -> Result<Value> {
    let status = response.status();
    if response
        .content_length()
        .is_some_and(|size| size > max_bytes)
    {
        bail!("{operation}响应超过安全大小限制");
    }
    let mut bytes = Vec::new();
    let mut limited: Take<Response> = response.take(max_bytes + 1);
    limited
        .read_to_end(&mut bytes)
        .context("无法读取 DashScope 响应")?;
    if bytes.len() as u64 > max_bytes {
        bail!("{operation}响应超过安全大小限制");
    }
    let value: Value =
        serde_json::from_slice(&bytes).with_context(|| format!("{operation}返回了无效 JSON"))?;
    if !status.is_success() {
        let code = value.get("code").and_then(Value::as_str).map(safe_code);
        if let Some(code) = code {
            bail!("{operation}失败（HTTP {}，错误码 {code}）", status.as_u16());
        }
        bail!("{operation}失败（HTTP {}）", status.as_u16());
    }
    Ok(value)
}

fn validate_aliyun_https_url(value: &str, label: &str) -> Result<()> {
    let url = reqwest::Url::parse(value).with_context(|| format!("{label}格式无效"))?;
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    if url.scheme() != "https" || (!host.ends_with(".aliyuncs.com") && host != "aliyuncs.com") {
        bail!("{label}不是受信任的阿里云 HTTPS 地址");
    }
    Ok(())
}

fn successful_result_url(value: &Value) -> Result<Option<String>> {
    let results = value
        .pointer("/output/results")
        .and_then(Value::as_array)
        .context("DashScope 成功任务缺少结果列表")?;
    for item in results {
        if item
            .get("subtask_status")
            .and_then(Value::as_str)
            .is_some_and(|status| status.eq_ignore_ascii_case("SUCCEEDED"))
            && let Some(url) = item.get("transcription_url").and_then(Value::as_str)
        {
            validate_aliyun_https_url(url, "转写结果地址")?;
            return Ok(Some(url.to_owned()));
        }
    }
    bail!("DashScope 任务成功但没有可下载的转写结果")
}

fn normalize_result(value: &Value) -> Result<ProviderTranscript> {
    let transcripts = value
        .get("transcripts")
        .and_then(Value::as_array)
        .context("DashScope 结果缺少 transcripts")?;
    let mut text_parts = Vec::new();
    let mut language = None;
    let mut segments = Vec::new();
    for transcript in transcripts {
        if let Some(text) = transcript
            .get("text")
            .or_else(|| transcript.get("transcript"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            text_parts.push(text.to_owned());
        }
        if language.is_none() {
            language = transcript
                .get("language")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned);
        }
        let Some(sentences) = transcript.get("sentences").and_then(Value::as_array) else {
            continue;
        };
        for sentence in sentences {
            let Some(text) = sentence
                .get("text")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
            else {
                continue;
            };
            let start_ms = sentence
                .get("begin_time")
                .and_then(value_as_u64)
                .context("DashScope 句子缺少有效的 begin_time")?;
            let end_ms = sentence
                .get("end_time")
                .and_then(value_as_u64)
                .context("DashScope 句子缺少有效的 end_time")?;
            if end_ms < start_ms {
                bail!("DashScope 句子时间范围无效");
            }
            segments.push(crate::models::TranscriptSegment {
                start_ms,
                end_ms,
                text: text.to_owned(),
                speaker: speaker_label(sentence.get("speaker_id")),
            });
        }
    }
    segments.sort_by_key(|segment| (segment.start_ms, segment.end_ms));
    let mut text = text_parts.join("\n");
    if text.trim().is_empty() {
        text = segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect();
    }
    if text.trim().is_empty() {
        bail!("DashScope 转写结果没有正文");
    }
    if segments.is_empty() {
        let duration_ms = value
            .pointer("/properties/original_duration_in_milliseconds")
            .and_then(value_as_u64)
            .unwrap_or(0);
        segments.push(crate::models::TranscriptSegment {
            start_ms: 0,
            end_ms: duration_ms,
            text: text.clone(),
            speaker: None,
        });
    }
    Ok(ProviderTranscript {
        text,
        language,
        segments,
    })
}

fn speaker_label(value: Option<&Value>) -> Option<String> {
    let raw = match value? {
        Value::Number(value) => value.as_i64()?.max(0).to_string(),
        Value::String(value) => value.trim().trim_start_matches("speaker_").to_owned(),
        _ => return None,
    };
    (!raw.is_empty()).then(|| format!("speaker_{raw}"))
}

fn value_as_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_i64().map(|value| value.max(0) as u64))
        .or_else(|| value.as_str()?.parse().ok())
}

fn value_as_f64(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.parse::<f64>().ok())
}

fn safe_code(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
        .take(80)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AsrProvider, AsrProviderKind};

    fn credentials() -> AsrProviderCredentials {
        AsrProviderCredentials {
            provider: AsrProvider {
                id: "dashscope".into(),
                name: "DashScope".into(),
                kind: AsrProviderKind::DashScope,
                base_url: API_ROOT.into(),
                model_id: MODEL.into(),
                has_api_key: true,
                capabilities: AsrProviderKind::DashScope.capabilities(),
                created_at: "2026-08-22T00:00:00Z".into(),
                updated_at: "2026-08-22T00:00:00Z".into(),
            },
            api_key: "secret-test-key".into(),
        }
    }

    #[test]
    fn rejects_non_aliyun_result_hosts() {
        assert!(validate_aliyun_https_url("https://example.com/result.json", "结果").is_err());
        assert!(
            validate_aliyun_https_url("http://bucket.oss-cn-hangzhou.aliyuncs.com/a", "结果")
                .is_err()
        );
        assert!(
            validate_aliyun_https_url("https://bucket.oss-cn-hangzhou.aliyuncs.com/a", "结果")
                .is_ok()
        );
    }

    #[test]
    fn normalizes_sentences_and_speakers() {
        let transcript = normalize_result(&json!({
            "transcripts": [{
                "text": "你好。世界。",
                "language": "zh",
                "sentences": [
                    {"begin_time": 0, "end_time": 500, "text": "你好。", "speaker_id": 0},
                    {"begin_time": 510, "end_time": 900, "text": "世界。", "speaker_id": "1"}
                ]
            }]
        }))
        .unwrap();
        assert_eq!(transcript.language.as_deref(), Some("zh"));
        assert_eq!(transcript.segments.len(), 2);
        assert_eq!(transcript.segments[0].speaker.as_deref(), Some("speaker_0"));
        assert_eq!(transcript.segments[1].speaker.as_deref(), Some("speaker_1"));
    }

    #[test]
    fn submission_uses_required_headers_model_and_diarization() {
        let request = build_submit_request(
            &client().unwrap(),
            &credentials(),
            "oss://temporary/audio.ogg",
            Some(4),
        )
        .build()
        .unwrap();
        assert_eq!(request.headers()["X-DashScope-Async"], "enable");
        assert_eq!(
            request.headers()["X-DashScope-OssResourceResolve"],
            "enable"
        );
        let body: Value =
            serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap();
        assert_eq!(body["model"], MODEL);
        assert_eq!(body["input"]["file_urls"][0], "oss://temporary/audio.ogg");
        assert_eq!(body["parameters"]["diarization_enabled"], true);
        assert_eq!(body["parameters"]["speaker_count"], 4);
    }

    #[test]
    fn credentials_reject_mutable_endpoint_model_and_missing_key() {
        let mut value = credentials();
        value.provider.base_url = "https://example.com/api/v1".into();
        assert!(validate_credentials(&value).is_err());
        value = credentials();
        value.provider.model_id = "different-model".into();
        assert!(validate_credentials(&value).is_err());
        value = credentials();
        value.api_key.clear();
        assert!(validate_credentials(&value).is_err());
    }

    #[test]
    fn rejects_reversed_sentence_timestamps() {
        assert!(
            normalize_result(&json!({
                "transcripts": [{
                    "text": "invalid",
                    "sentences": [{"begin_time": 1000, "end_time": 900, "text": "invalid"}]
                }]
            }))
            .is_err()
        );
    }

    #[test]
    fn upload_reader_stops_when_local_cancellation_is_requested() {
        let path = std::env::temp_dir().join(format!(
            "nota-dashscope-cancel-{}.ogg",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&path, b"audio").unwrap();
        let cancelled = Arc::new(AtomicBool::new(true));
        let mut reader = ProgressReader {
            inner: File::open(&path).unwrap(),
            completed: 0,
            on_progress: Arc::new(|_| {}),
            cancelled,
        };
        let error = reader.read(&mut [0u8; 8]).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
        std::fs::remove_file(path).unwrap();
    }
}
