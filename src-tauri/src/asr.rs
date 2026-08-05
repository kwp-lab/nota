use crate::models::{
    AsrConnectionLevel, AsrConnectionTest, AsrModel, AsrProviderCredentials, AsrProviderKind,
    StoredTranscriptionChunk, TranscriptSegment, TranscriptionEvent, TranscriptionProgressPhase,
    TranscriptionProgressUnit, TranscriptionProtocol, TranscriptionStatus, TranscriptionSummary,
};
use crate::storage::Storage;
use anyhow::{Context, Result, bail};
use crossbeam_channel::{Receiver, Sender, unbounded};
use ogg::PacketReader;
use opus::{Channels, Decoder};
use parking_lot::Mutex;
use reqwest::StatusCode;
use reqwest::blocking::Client;
use reqwest::blocking::multipart::{Form, Part};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const ASR_SAMPLE_RATE: u32 = 16_000;
const CHUNK_DURATION_MS: u64 = 10 * 60 * 1_000;
const CHUNK_OVERLAP_MS: u64 = 2_000;
const CHUNK_SAMPLES: usize = ASR_SAMPLE_RATE as usize * 10 * 60;
const OVERLAP_SAMPLES: usize = ASR_SAMPLE_RATE as usize * 2;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const BATCH_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const BATCH_POLL_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub struct SpeakerEmbeddingCapabilities {
    pub max_bytes: u64,
    pub analysis_max_files: usize,
    pub analysis_min_clip_seconds: u64,
    pub analysis_max_clip_seconds: u64,
    pub analysis_max_total_seconds: u64,
    pub analysis_min_accepted_seconds: u64,
    pub analysis_min_purity: f32,
}

#[derive(Debug, Clone)]
pub struct CleanSpeakerSampleRange {
    pub file_index: usize,
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Debug, Clone)]
pub struct SpeakerSampleAnalysisResult {
    pub outcome: SpeakerSampleAnalysisOutcome,
    pub fingerprint: String,
    pub embedding: Option<Vec<f32>>,
    pub accepted_audio_ms: u64,
    pub purity_score: f32,
    pub preview: CleanSpeakerSampleRange,
    pub accepted_ranges: Vec<CleanSpeakerSampleRange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeakerSampleAnalysisOutcome {
    Enrollable,
    PreviewOnly,
}

#[derive(Clone)]
struct TranscriptionJob {
    app: AppHandle,
    recording_id: String,
    generation: u32,
}

#[derive(Debug, Clone)]
struct ProviderTranscript {
    text: String,
    language: Option<String>,
    segments: Vec<TranscriptSegment>,
}

struct Cancellation {
    requested: AtomicBool,
    remote_requested: AtomicBool,
}

impl Cancellation {
    fn new() -> Self {
        Self {
            requested: AtomicBool::new(false),
            remote_requested: AtomicBool::new(false),
        }
    }
}

pub struct AsrManager {
    storage: Arc<Storage>,
    sender: Sender<TranscriptionJob>,
    cancellations: Arc<Mutex<HashMap<String, Arc<Cancellation>>>>,
}

impl AsrManager {
    pub fn new(storage: Arc<Storage>, is_recording: Arc<dyn Fn() -> bool + Send + Sync>) -> Self {
        let (sender, receiver) = unbounded();
        let cancellations = Arc::new(Mutex::new(HashMap::new()));
        let worker_storage = Arc::clone(&storage);
        let worker_cancellations = Arc::clone(&cancellations);
        let worker_recording = Arc::clone(&is_recording);
        std::thread::Builder::new()
            .name("nota-asr-worker".into())
            .spawn(move || {
                worker_loop(
                    receiver,
                    worker_storage,
                    worker_cancellations,
                    worker_recording,
                )
            })
            .expect("无法启动 Nota 转写工作线程");
        Self {
            storage,
            sender,
            cancellations,
        }
    }

    pub fn start(
        &self,
        app: AppHandle,
        recording_id: &str,
        provider_id: &str,
        speaker_count: Option<u32>,
    ) -> Result<TranscriptionSummary> {
        self.reserve(recording_id)?;
        let result = (|| {
            let recording = self.storage.find_recording(recording_id)?;
            if !Path::new(&recording.path).is_file() {
                bail!("录音文件不存在或已被移动");
            }
            let credentials = self.storage.find_asr_provider(provider_id)?;
            if speaker_count.is_some() && credentials.provider.kind != AsrProviderKind::FunAsr {
                bail!("只有 FunASR 服务支持指定说话人数");
            }
            if credentials.provider.kind == AsrProviderKind::FunAsr {
                fetch_batch_capabilities(&credentials)
                    .context("当前 FunASR Server 不支持整场会议转写，请升级 Nota ASR Server")?;
            }
            if cleanup_previous_batch_job_if_present(&self.storage, recording_id).is_err() {
                log::warn!(
                    "unable to clean previous remote ASR job recording={}",
                    recording_id
                );
            }
            remove_temporary_chunks(&self.storage.paths().recovery, recording_id)?;
            let generation = self.storage.begin_transcription(
                recording_id,
                &credentials.provider,
                speaker_count,
            )?;
            self.enqueue(app, recording_id, generation)?;
            self.storage.transcription_summary(recording_id)
        })();
        if result.is_err() {
            self.cancellations.lock().remove(recording_id);
        }
        result
    }

    pub fn resume(&self, app: AppHandle, recording_id: &str) -> Result<TranscriptionSummary> {
        self.reserve(recording_id)?;
        let result = (|| {
            let generation = self.storage.resume_transcription(recording_id)?;
            self.enqueue(app, recording_id, generation)?;
            self.storage.transcription_summary(recording_id)
        })();
        if result.is_err() {
            self.cancellations.lock().remove(recording_id);
        }
        result
    }

    fn reserve(&self, recording_id: &str) -> Result<()> {
        let mut cancellations = self.cancellations.lock();
        if cancellations.contains_key(recording_id) {
            bail!("该录音已有转写任务正在排队或处理");
        }
        cancellations.insert(recording_id.to_owned(), Arc::new(Cancellation::new()));
        Ok(())
    }

    fn enqueue(&self, app: AppHandle, recording_id: &str, generation: u32) -> Result<()> {
        if let Err(error) = self.sender.send(TranscriptionJob {
            app,
            recording_id: recording_id.to_owned(),
            generation,
        }) {
            self.cancellations.lock().remove(recording_id);
            bail!("无法加入转写队列：{error}");
        }
        Ok(())
    }

    pub fn cancel(&self, app: &AppHandle, recording_id: &str) -> Result<TranscriptionSummary> {
        let cancellation = self
            .cancellations
            .lock()
            .get(recording_id)
            .cloned()
            .context("该录音当前没有可取消的转写任务")?;
        cancellation.requested.store(true, Ordering::Release);
        cancellation.remote_requested.store(true, Ordering::Release);
        if let Ok(Some((credentials, remote_job_id))) =
            remote_batch_target(&self.storage, recording_id)
        {
            let cancel_recording_id = recording_id.to_owned();
            if let Err(error) = std::thread::Builder::new()
                .name("nota-asr-remote-cancel".into())
                .spawn(move || {
                    if cancel_batch_job(&credentials, &remote_job_id).is_err() {
                        log::warn!(
                            "unable to cancel remote ASR job recording={}",
                            cancel_recording_id
                        );
                    }
                })
            {
                log::warn!("unable to start remote ASR cancellation: {error}");
            }
        }
        self.storage.set_transcription_error(
            recording_id,
            self.current_generation(recording_id)?,
            TranscriptionStatus::Cancelled,
            "用户已中断转写；可以稍后继续",
        )?;
        let summary = self.storage.transcription_summary(recording_id)?;
        emit_status(app, recording_id, summary.clone());
        Ok(summary)
    }

    pub fn has_active(&self) -> bool {
        !self.cancellations.lock().is_empty()
    }

    pub fn is_active(&self, recording_id: &str) -> bool {
        self.cancellations.lock().contains_key(recording_id)
    }

    pub fn interrupt_all(&self) -> Result<()> {
        for cancellation in self.cancellations.lock().values() {
            cancellation.requested.store(true, Ordering::Release);
        }
        self.storage.interrupt_running_transcriptions()
    }

    fn current_generation(&self, recording_id: &str) -> Result<u32> {
        self.storage.transcription_generation(recording_id)
    }
}

fn worker_loop(
    receiver: Receiver<TranscriptionJob>,
    storage: Arc<Storage>,
    cancellations: Arc<Mutex<HashMap<String, Arc<Cancellation>>>>,
    is_recording: Arc<dyn Fn() -> bool + Send + Sync>,
) {
    while let Ok(job) = receiver.recv() {
        let cancellation = cancellations
            .lock()
            .get(&job.recording_id)
            .cloned()
            .unwrap_or_else(|| {
                Arc::new(Cancellation {
                    requested: AtomicBool::new(true),
                    remote_requested: AtomicBool::new(false),
                })
            });
        let result = process_job(&job, &storage, cancellation.as_ref(), is_recording.as_ref());
        if let Err(error) = result {
            let cancelled = cancellation.requested.load(Ordering::Acquire);
            let status = if cancelled {
                TranscriptionStatus::Cancelled
            } else {
                TranscriptionStatus::Failed
            };
            let message = if cancelled {
                "转写已中断；已完成进度已保留，可以稍后继续".to_owned()
            } else {
                sanitize_error(&format!("{error:#}"))
            };
            if let Err(storage_error) =
                storage.set_transcription_error(&job.recording_id, job.generation, status, &message)
            {
                log::error!(
                    "failed to persist ASR error recording={} error={storage_error:#}",
                    job.recording_id
                );
            }
            if let Ok(summary) = storage.transcription_summary(&job.recording_id) {
                emit_status(&job.app, &job.recording_id, summary);
            }
            log::error!(
                "ASR job failed recording={} generation={}",
                job.recording_id,
                job.generation
            );
        }
        cancellations.lock().remove(&job.recording_id);
    }
}

fn process_job(
    job: &TranscriptionJob,
    storage: &Storage,
    cancellation: &Cancellation,
    is_recording: &(dyn Fn() -> bool + Send + Sync),
) -> Result<()> {
    while is_recording() {
        ensure_not_cancelled(&cancellation.requested)?;
        std::thread::sleep(Duration::from_millis(250));
    }
    ensure_not_cancelled(&cancellation.requested)?;

    let recording = storage.find_recording(&job.recording_id)?;
    let (provider_id, provider_name, model_id) =
        storage.transcription_provider_snapshot(&job.recording_id)?;
    let mut credentials = storage.find_asr_provider(&provider_id)?;
    credentials.provider.name = provider_name;
    credentials.provider.model_id = model_id;
    let execution = storage.transcription_execution(&job.recording_id)?;
    if execution.protocol == TranscriptionProtocol::NotaBatchV1 {
        return process_batch_job(
            job,
            storage,
            cancellation,
            &credentials,
            Path::new(&recording.path),
            recording.size_bytes,
            recording.duration_ms,
        );
    }
    let declared_total_chunks = chunk_count(recording.duration_ms);
    let completed: HashSet<u32> = storage
        .completed_transcription_chunks(&job.recording_id, job.generation)?
        .into_iter()
        .collect();
    let mut completed_count = completed.len() as u32;
    storage.set_transcription_progress(
        &job.recording_id,
        job.generation,
        TranscriptionStatus::Preparing,
        completed_count,
        declared_total_chunks,
    )?;
    emit_current_status(&job.app, storage, &job.recording_id);

    let temp_directory = storage.paths().recovery.join("TranscriptionTemp");
    std::fs::create_dir_all(&temp_directory)?;
    let mut reader = OggPcm16Reader::open(Path::new(&recording.path), recording.duration_ms)?;
    let mut chunk_index = 0u32;
    let mut overlap = Vec::new();

    loop {
        ensure_not_cancelled(&cancellation.requested)?;
        while is_recording() {
            ensure_not_cancelled(&cancellation.requested)?;
            std::thread::sleep(Duration::from_millis(250));
        }
        let wanted = if chunk_index == 0 {
            CHUNK_SAMPLES
        } else {
            CHUNK_SAMPLES - overlap.len()
        };
        let fresh = reader.read_samples(wanted)?;
        if fresh.is_empty() && overlap.is_empty() {
            break;
        }
        let mut samples = Vec::with_capacity(overlap.len() + fresh.len());
        samples.extend_from_slice(&overlap);
        samples.extend_from_slice(&fresh);
        if samples.is_empty() {
            break;
        }
        while is_recording() {
            ensure_not_cancelled(&cancellation.requested)?;
            std::thread::sleep(Duration::from_millis(250));
        }
        let is_last = reader.finished();
        let start_ms = chunk_start_ms(chunk_index);
        let raw_end_ms =
            start_ms.saturating_add(samples.len() as u64 * 1_000 / ASR_SAMPLE_RATE as u64);
        let end_ms = if recording.duration_ms == 0 {
            raw_end_ms
        } else {
            raw_end_ms.min(recording.duration_ms.max(start_ms))
        };
        let current_total = declared_total_chunks.max(chunk_index.saturating_add(1));

        storage.set_transcription_progress(
            &job.recording_id,
            job.generation,
            TranscriptionStatus::Transcribing,
            completed_count,
            current_total,
        )?;
        emit_current_status(&job.app, storage, &job.recording_id);

        if !completed.contains(&chunk_index) {
            let wav_path = temp_directory.join(format!(
                "{}-{}-{}.wav",
                job.recording_id, job.generation, chunk_index
            ));
            write_pcm16_wav(&wav_path, &samples)?;
            let upload_result = transcribe_file(&credentials, &wav_path).and_then(|transcript| {
                ensure_not_cancelled(&cancellation.requested)?;
                storage.save_transcription_chunk(
                    &job.recording_id,
                    job.generation,
                    &StoredTranscriptionChunk {
                        index: chunk_index,
                        start_ms,
                        end_ms,
                        text: transcript.text,
                        segments: transcript.segments,
                        language: transcript.language,
                    },
                )?;
                Ok(())
            });
            if let Err(error) = std::fs::remove_file(&wav_path) {
                log::warn!("unable to remove ASR temporary chunk: {error}");
            }
            upload_result?;
            completed_count = completed_count.saturating_add(1);
            emit_current_status(&job.app, storage, &job.recording_id);
        }

        overlap = if samples.len() > OVERLAP_SAMPLES && !is_last {
            samples[samples.len() - OVERLAP_SAMPLES..].to_vec()
        } else {
            Vec::new()
        };
        chunk_index += 1;
        if is_last {
            break;
        }
    }

    ensure_not_cancelled(&cancellation.requested)?;
    let chunks = storage.transcription_chunks(&job.recording_id, job.generation)?;
    let actual_total_chunks = chunk_index.max(1);
    storage.set_transcription_progress(
        &job.recording_id,
        job.generation,
        TranscriptionStatus::Transcribing,
        chunks.len().min(u32::MAX as usize) as u32,
        actual_total_chunks,
    )?;
    if chunks.len() != actual_total_chunks as usize {
        bail!(
            "分块转写尚未完成（{}/{}），可以稍后继续",
            chunks.len(),
            actual_total_chunks
        );
    }
    let (text, segments, language) = merge_chunks(chunks);
    storage.complete_transcription(
        &job.recording_id,
        job.generation,
        &text,
        &segments,
        language.as_deref(),
    )?;
    emit_current_status(&job.app, storage, &job.recording_id);
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
struct BatchCapabilities {
    batch_transcription_version: String,
    upload_chunk_bytes: u64,
    max_upload_bytes: u64,
    max_audio_seconds: u64,
    audio_formats: Vec<String>,
    #[serde(default)]
    speaker_embedding_version: Option<String>,
    #[serde(default)]
    speaker_embedding_max_bytes: Option<u64>,
    #[serde(default)]
    speaker_embedding_min_seconds: Option<u64>,
    #[serde(default)]
    speaker_embedding_max_seconds: Option<u64>,
    #[serde(default)]
    speaker_sample_analysis_version: Option<String>,
    #[serde(default)]
    speaker_sample_analysis_max_files: Option<usize>,
    #[serde(default)]
    speaker_sample_analysis_min_clip_seconds: Option<u64>,
    #[serde(default)]
    speaker_sample_analysis_max_clip_seconds: Option<u64>,
    #[serde(default)]
    speaker_sample_analysis_max_total_seconds: Option<u64>,
    #[serde(default)]
    speaker_sample_analysis_min_accepted_seconds: Option<u64>,
    #[serde(default)]
    speaker_sample_analysis_min_purity: Option<f32>,
}

#[derive(Debug, Clone, Deserialize)]
struct BatchJobFailure {
    code: String,
    message: String,
}

#[derive(Debug, Clone, Deserialize)]
struct BatchJobStatus {
    id: String,
    state: String,
    phase: String,
    upload_offset: u64,
    upload_length: u64,
    progress_current: u64,
    progress_total: u64,
    progress_unit: String,
    error: Option<BatchJobFailure>,
}

#[derive(Serialize)]
struct CreateBatchJobRequest {
    file_name: String,
    content_type: &'static str,
    size_bytes: u64,
    model: String,
    language: &'static str,
    response_format: &'static str,
    diarization: bool,
    speaker_count: Option<u32>,
}

fn process_batch_job(
    job: &TranscriptionJob,
    storage: &Storage,
    cancellation: &Cancellation,
    credentials: &AsrProviderCredentials,
    recording_path: &Path,
    recorded_size: u64,
    recorded_duration_ms: u64,
) -> Result<()> {
    let capabilities = fetch_batch_capabilities(credentials)?;
    let actual_size = std::fs::metadata(recording_path)?.len();
    if actual_size != recorded_size {
        log::warn!(
            "recording size changed before ASR upload recording={} indexed={} actual={}",
            job.recording_id,
            recorded_size,
            actual_size
        );
    }
    if actual_size > capabilities.max_upload_bytes {
        bail!("录音文件超过 ASR Server 允许的上传大小");
    }
    if recorded_duration_ms > capabilities.max_audio_seconds.saturating_mul(1_000) {
        bail!("录音时长超过 ASR Server 允许的上限");
    }
    let chunk_bytes = capabilities.upload_chunk_bytes.clamp(1, 16 * 1024 * 1024) as usize;
    let execution = storage.transcription_execution(&job.recording_id)?;
    let had_remote_job = execution.remote_job_id.is_some();
    ensure_batch_not_cancelled(
        cancellation,
        credentials,
        execution.remote_job_id.as_deref(),
    )?;

    let mut remote = match execution.remote_job_id.as_deref() {
        Some(remote_id) => match get_batch_job(credentials, remote_id)? {
            Some(status) => status,
            None => {
                let created = create_batch_job(
                    credentials,
                    recording_path,
                    actual_size,
                    &execution.idempotency_key,
                    execution.speaker_count,
                )?;
                storage.set_remote_transcription_job(
                    &job.recording_id,
                    job.generation,
                    Some(&created.id),
                )?;
                created
            }
        },
        None => {
            let created = create_batch_job(
                credentials,
                recording_path,
                actual_size,
                &execution.idempotency_key,
                execution.speaker_count,
            )?;
            storage.set_remote_transcription_job(
                &job.recording_id,
                job.generation,
                Some(&created.id),
            )?;
            created
        }
    };

    ensure_batch_not_cancelled(cancellation, credentials, Some(&remote.id))?;
    if had_remote_job && matches!(remote.state.as_str(), "cancelled" | "failed") {
        loop {
            match resume_batch_job(credentials, &remote.id) {
                Ok(resumed) => {
                    remote = resumed;
                    break;
                }
                Err(error) if format!("{error:#}").contains("job_still_stopping") => {
                    wait_for_batch_poll(cancellation, credentials, &remote.id)?;
                }
                Err(error) => return Err(error),
            }
        }
    }

    loop {
        ensure_batch_not_cancelled(cancellation, credentials, Some(&remote.id))?;
        persist_batch_status(job, storage, &remote)?;
        match remote.state.as_str() {
            "uploading" => {
                remote = upload_batch_audio(
                    job,
                    storage,
                    cancellation,
                    credentials,
                    recording_path,
                    remote,
                    chunk_bytes,
                )?;
                ensure_batch_not_cancelled(cancellation, credentials, Some(&remote.id))?;
                remote = complete_batch_job(credentials, &remote.id)?;
            }
            "queued" | "processing" => {
                wait_for_batch_poll(cancellation, credentials, &remote.id)?;
                remote = get_batch_job(credentials, &remote.id)?
                    .context("ASR Server 上的转写任务已不存在；请点击继续转写以重新上传")?;
            }
            "succeeded" => {
                storage.set_batch_transcription_progress(
                    &job.recording_id,
                    job.generation,
                    TranscriptionStatus::Transcribing,
                    TranscriptionProgressPhase::Finalizing,
                    0,
                    1,
                    TranscriptionProgressUnit::Steps,
                )?;
                emit_current_status(&job.app, storage, &job.recording_id);
                let transcript = fetch_batch_result(credentials, &remote.id)?;
                storage.complete_transcription(
                    &job.recording_id,
                    job.generation,
                    &transcript.text,
                    &transcript.segments,
                    transcript.language.as_deref(),
                )?;
                emit_current_status(&job.app, storage, &job.recording_id);
                match delete_batch_job(credentials, &remote.id) {
                    Ok(()) => {
                        storage.set_remote_transcription_job(
                            &job.recording_id,
                            job.generation,
                            None,
                        )?;
                    }
                    Err(_) => {
                        log::warn!(
                            "unable to acknowledge remote ASR result recording={}",
                            job.recording_id
                        );
                    }
                }
                return Ok(());
            }
            "cancelled" => bail!("ASR Server 上的转写任务已取消，可以稍后继续"),
            "failed" => {
                let detail = remote
                    .error
                    .as_ref()
                    .map(|error| format!("{} ({})", error.message, error.code))
                    .unwrap_or_else(|| "ASR Server 处理会议录音失败".into());
                bail!("{detail}");
            }
            state => bail!("ASR Server 返回了未知任务状态：{state}"),
        }
    }
}

fn fetch_batch_capabilities(credentials: &AsrProviderCredentials) -> Result<BatchCapabilities> {
    let client = http_client()?;
    let mut request = client
        .get(format!(
            "{}/nota/capabilities",
            normalize_base_url(&credentials.provider.base_url)?
        ))
        .timeout(BATCH_REQUEST_TIMEOUT);
    if !credentials.api_key.is_empty() {
        request = request.bearer_auth(&credentials.api_key);
    }
    let response = request.send().context("无法连接 Nota 批处理能力接口")?;
    let status = response.status();
    let body = response.text().context("无法读取 Nota 批处理能力响应")?;
    if status == StatusCode::NOT_FOUND {
        bail!("FunASR Server 版本过旧，不支持整场会议说话人一致性协议");
    }
    if !status.is_success() {
        bail!(redact_secret(
            &http_error("Nota 批处理能力接口", status, &body),
            &credentials.api_key
        ));
    }
    let capabilities: BatchCapabilities =
        serde_json::from_str(&body).context("Nota 批处理能力接口返回了无效 JSON")?;
    if capabilities.batch_transcription_version != "1"
        || !capabilities
            .audio_formats
            .iter()
            .any(|format| format.eq_ignore_ascii_case("ogg"))
        || capabilities.upload_chunk_bytes == 0
    {
        bail!("FunASR Server 不支持 Nota 所需的批处理协议版本");
    }
    Ok(capabilities)
}

pub fn fetch_speaker_embedding_capabilities(
    credentials: &AsrProviderCredentials,
) -> Result<SpeakerEmbeddingCapabilities> {
    if credentials.provider.kind != AsrProviderKind::FunAsr {
        bail!("说话人识别需要 Nota ASR Server（FunASR 类型）");
    }
    let capabilities = fetch_batch_capabilities(credentials)?;
    if capabilities.speaker_embedding_version.as_deref() != Some("1") {
        bail!("当前 Nota ASR Server 版本不支持说话人声纹提取，请升级服务端");
    }
    if capabilities.speaker_sample_analysis_version.as_deref() != Some("1") {
        bail!("当前 Nota ASR Server 版本不支持 CAM++ 纯净声纹样本筛选，请升级服务端");
    }
    let max_bytes = capabilities
        .speaker_embedding_max_bytes
        .context("服务端未返回声纹样本字节限制")?;
    let min_seconds = capabilities
        .speaker_embedding_min_seconds
        .context("服务端未返回声纹样本最短时长")?;
    let max_seconds = capabilities
        .speaker_embedding_max_seconds
        .context("服务端未返回声纹样本最长时长")?;
    let analysis_max_files = capabilities
        .speaker_sample_analysis_max_files
        .context("服务端未返回纯净样本候选数量限制")?;
    let analysis_min_clip_seconds = capabilities
        .speaker_sample_analysis_min_clip_seconds
        .context("服务端未返回纯净样本最短片段限制")?;
    let analysis_max_clip_seconds = capabilities
        .speaker_sample_analysis_max_clip_seconds
        .context("服务端未返回纯净样本最长片段限制")?;
    let analysis_max_total_seconds = capabilities
        .speaker_sample_analysis_max_total_seconds
        .context("服务端未返回纯净样本累计时长限制")?;
    let analysis_min_accepted_seconds = capabilities
        .speaker_sample_analysis_min_accepted_seconds
        .context("服务端未返回纯净样本最短有效时长")?;
    let analysis_min_purity = capabilities
        .speaker_sample_analysis_min_purity
        .context("服务端未返回纯净样本最低纯度")?;
    if max_bytes == 0
        || min_seconds == 0
        || max_seconds <= min_seconds
        || analysis_max_files == 0
        || analysis_min_clip_seconds == 0
        || analysis_max_clip_seconds < analysis_min_clip_seconds
        || analysis_max_total_seconds < analysis_min_clip_seconds
        || analysis_min_accepted_seconds < analysis_min_clip_seconds
        || analysis_min_accepted_seconds > analysis_max_total_seconds
        || !analysis_min_purity.is_finite()
        || analysis_min_purity <= 0.0
        || analysis_min_purity > 1.0
    {
        bail!("服务端返回了无效的声纹样本限制");
    }
    Ok(SpeakerEmbeddingCapabilities {
        max_bytes,
        analysis_max_files,
        analysis_min_clip_seconds,
        analysis_max_clip_seconds,
        analysis_max_total_seconds,
        analysis_min_accepted_seconds,
        analysis_min_purity,
    })
}

#[derive(Debug, Deserialize)]
struct RawCleanSpeakerSampleRange {
    file_index: usize,
    start: f64,
    end: f64,
}

#[derive(Debug, Deserialize)]
struct RawSpeakerSampleAnalysisResponse {
    schema_version: String,
    outcome: SpeakerSampleAnalysisOutcome,
    embedding_model: String,
    embedding_fingerprint: String,
    dimension: usize,
    accepted_audio_duration: f64,
    purity_score: f32,
    preview: RawCleanSpeakerSampleRange,
    accepted_ranges: Vec<RawCleanSpeakerSampleRange>,
    embedding: Option<Vec<f32>>,
}

pub fn analyze_speaker_samples(
    credentials: &AsrProviderCredentials,
    paths: &[PathBuf],
) -> Result<SpeakerSampleAnalysisResult> {
    if paths.is_empty() {
        bail!("没有可供 CAM++ 分析的候选声音片段");
    }
    let base = normalize_base_url(&credentials.provider.base_url)?;
    let mut form = Form::new();
    for (index, path) in paths.iter().enumerate() {
        let file = File::open(path)?;
        let file_length = std::fs::metadata(path)?.len();
        let part = Part::reader_with_length(file, file_length)
            .file_name(format!("nota-candidate-{index}.wav"))
            .mime_str("audio/wav")?;
        form = form.part("files", part);
    }
    let client = http_client()?;
    let mut request = client
        .post(format!("{base}/nota/speaker-samples/analyze"))
        .timeout(REQUEST_TIMEOUT)
        .multipart(form);
    if !credentials.api_key.is_empty() {
        request = request.bearer_auth(&credentials.api_key);
    }
    let response = request
        .send()
        .context("无法连接 CAM++ 纯净声音样本分析接口")?;
    let status = response.status();
    let body = response.text().context("无法读取纯净声音样本分析响应")?;
    if !status.is_success() {
        bail!(redact_secret(
            &http_error("纯净声音样本分析接口", status, &body),
            &credentials.api_key
        ));
    }
    let raw: RawSpeakerSampleAnalysisResponse =
        serde_json::from_str(&body).context("纯净声音样本分析接口返回了无效 JSON")?;
    if raw.schema_version != "1"
        || raw.embedding_model != "cam++"
        || raw.embedding_fingerprint.trim().is_empty()
        || !raw.accepted_audio_duration.is_finite()
        || raw.accepted_audio_duration < 0.0
        || !raw.purity_score.is_finite()
        || !(0.0..=1.0).contains(&raw.purity_score)
    {
        bail!("纯净声音样本分析接口返回了不兼容的数据");
    }
    match (&raw.outcome, &raw.embedding) {
        (SpeakerSampleAnalysisOutcome::Enrollable, Some(embedding))
            if raw.dimension > 0
                && embedding.len() == raw.dimension
                && embedding.iter().all(|value| value.is_finite())
                && raw.accepted_audio_duration > 0.0
                && !raw.accepted_ranges.is_empty() => {}
        (SpeakerSampleAnalysisOutcome::PreviewOnly, None)
            if raw.dimension == 0 && raw.accepted_ranges.is_empty() => {}
        _ => bail!("纯净声音样本分析接口返回了互相矛盾的分析结果"),
    }
    let parse_range = |value: RawCleanSpeakerSampleRange| -> Result<CleanSpeakerSampleRange> {
        if value.file_index >= paths.len()
            || !value.start.is_finite()
            || !value.end.is_finite()
            || value.start < 0.0
            || value.end <= value.start
        {
            bail!("纯净声音样本分析接口返回了无效的时间范围");
        }
        Ok(CleanSpeakerSampleRange {
            file_index: value.file_index,
            start_ms: seconds_to_ms(value.start),
            end_ms: seconds_to_ms(value.end),
        })
    };
    let preview = parse_range(raw.preview)?;
    let accepted_ranges = raw
        .accepted_ranges
        .into_iter()
        .map(parse_range)
        .collect::<Result<Vec<_>>>()?;
    let embedding = raw
        .embedding
        .map(|embedding| {
            let norm = embedding
                .iter()
                .map(|value| value * value)
                .sum::<f32>()
                .sqrt();
            if !norm.is_finite() || norm <= f32::EPSILON {
                bail!("纯净声音样本分析接口返回了空向量");
            }
            Ok(embedding
                .into_iter()
                .map(|value| value / norm)
                .collect::<Vec<_>>())
        })
        .transpose()?;
    Ok(SpeakerSampleAnalysisResult {
        outcome: raw.outcome,
        fingerprint: raw.embedding_fingerprint,
        embedding,
        accepted_audio_ms: seconds_to_ms(raw.accepted_audio_duration),
        purity_score: raw.purity_score,
        preview,
        accepted_ranges,
    })
}

fn create_batch_job(
    credentials: &AsrProviderCredentials,
    recording_path: &Path,
    size_bytes: u64,
    idempotency_key: &str,
    speaker_count: Option<u32>,
) -> Result<BatchJobStatus> {
    let file_name = recording_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("meeting.ogg")
        .to_owned();
    let payload = CreateBatchJobRequest {
        file_name,
        content_type: "audio/ogg",
        size_bytes,
        model: credentials.provider.model_id.clone(),
        language: "auto",
        response_format: "verbose_json",
        diarization: true,
        speaker_count,
    };
    let client = http_client()?;
    let mut request = client
        .post(format!(
            "{}/nota/transcription-jobs",
            normalize_base_url(&credentials.provider.base_url)?
        ))
        .timeout(BATCH_REQUEST_TIMEOUT)
        .header("Idempotency-Key", idempotency_key)
        .json(&payload);
    if !credentials.api_key.is_empty() {
        request = request.bearer_auth(&credentials.api_key);
    }
    parse_batch_status_response(
        request.send().context("无法创建整场会议转写任务")?,
        "创建整场会议转写任务",
        credentials,
    )
}

fn get_batch_job(
    credentials: &AsrProviderCredentials,
    remote_job_id: &str,
) -> Result<Option<BatchJobStatus>> {
    let client = http_client()?;
    let mut request = client
        .get(batch_job_url(credentials, remote_job_id)?)
        .timeout(BATCH_REQUEST_TIMEOUT);
    if !credentials.api_key.is_empty() {
        request = request.bearer_auth(&credentials.api_key);
    }
    let response = request.send().context("无法查询整场会议转写任务")?;
    if matches!(response.status(), StatusCode::NOT_FOUND | StatusCode::GONE) {
        return Ok(None);
    }
    parse_batch_status_response(response, "查询整场会议转写任务", credentials).map(Some)
}

fn resume_batch_job(
    credentials: &AsrProviderCredentials,
    remote_job_id: &str,
) -> Result<BatchJobStatus> {
    send_batch_status_action(credentials, remote_job_id, "resume", "继续整场会议转写任务")
}

fn complete_batch_job(
    credentials: &AsrProviderCredentials,
    remote_job_id: &str,
) -> Result<BatchJobStatus> {
    send_batch_status_action(credentials, remote_job_id, "complete", "提交完整会议录音")
}

fn cancel_batch_job(
    credentials: &AsrProviderCredentials,
    remote_job_id: &str,
) -> Result<BatchJobStatus> {
    send_batch_status_action(credentials, remote_job_id, "cancel", "取消整场会议转写任务")
}

fn send_batch_status_action(
    credentials: &AsrProviderCredentials,
    remote_job_id: &str,
    action: &str,
    label: &str,
) -> Result<BatchJobStatus> {
    let client = http_client()?;
    let mut request = client
        .post(format!(
            "{}/{action}",
            batch_job_url(credentials, remote_job_id)?
        ))
        .timeout(BATCH_REQUEST_TIMEOUT);
    if !credentials.api_key.is_empty() {
        request = request.bearer_auth(&credentials.api_key);
    }
    parse_batch_status_response(
        request.send().with_context(|| format!("无法{label}"))?,
        label,
        credentials,
    )
}

fn upload_batch_audio(
    job: &TranscriptionJob,
    storage: &Storage,
    cancellation: &Cancellation,
    credentials: &AsrProviderCredentials,
    recording_path: &Path,
    mut remote: BatchJobStatus,
    chunk_bytes: usize,
) -> Result<BatchJobStatus> {
    let size = std::fs::metadata(recording_path)?.len();
    if remote.upload_length != size || remote.upload_offset > size {
        bail!("ASR Server 上的上传任务与本地录音大小不一致");
    }
    let mut file = File::open(recording_path)?;
    let client = http_client()?;
    while remote.upload_offset < size {
        ensure_batch_not_cancelled(cancellation, credentials, Some(&remote.id))?;
        let length = (size - remote.upload_offset).min(chunk_bytes as u64) as usize;
        let mut content = vec![0u8; length];
        file.seek(SeekFrom::Start(remote.upload_offset))?;
        file.read_exact(&mut content)?;
        let checksum = format!("{:x}", Sha256::digest(&content));
        let mut request = client
            .patch(format!("{}/audio", batch_job_url(credentials, &remote.id)?))
            .timeout(BATCH_REQUEST_TIMEOUT)
            .header("Upload-Offset", remote.upload_offset)
            .header("Upload-Checksum", format!("sha256={checksum}"))
            .header("Content-Type", "application/offset+octet-stream")
            .body(content);
        if !credentials.api_key.is_empty() {
            request = request.bearer_auth(&credentials.api_key);
        }
        let response = request.send().context("上传会议录音失败")?;
        let status = response.status();
        let server_offset = response
            .headers()
            .get("Upload-Offset")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok());
        if status == StatusCode::CONFLICT
            && let Some(offset) = server_offset
        {
            if offset > size {
                bail!("ASR Server 返回了无效的上传偏移");
            }
            remote.upload_offset = offset;
            continue;
        }
        let body = response.text().context("无法读取会议录音上传响应")?;
        if !status.is_success() {
            bail!(redact_secret(
                &http_error("会议录音上传", status, &body),
                &credentials.api_key
            ));
        }
        let accepted_offset = server_offset.context("ASR Server 上传响应缺少 Upload-Offset")?;
        if accepted_offset != remote.upload_offset + length as u64 {
            bail!("ASR Server 返回了无效的上传偏移");
        }
        remote.upload_offset = accepted_offset;
        remote.progress_current = remote.upload_offset;
        remote.progress_total = remote.upload_length;
        remote.progress_unit = "bytes".into();
        persist_batch_status(job, storage, &remote)?;
    }
    Ok(remote)
}

fn fetch_batch_result(
    credentials: &AsrProviderCredentials,
    remote_job_id: &str,
) -> Result<ProviderTranscript> {
    let client = http_client()?;
    let mut request = client
        .get(format!(
            "{}/result",
            batch_job_url(credentials, remote_job_id)?
        ))
        .timeout(BATCH_REQUEST_TIMEOUT);
    if !credentials.api_key.is_empty() {
        request = request.bearer_auth(&credentials.api_key);
    }
    let response = request.send().context("无法读取整场会议转写结果")?;
    let status = response.status();
    let body = response.text().context("无法读取整场会议转写响应")?;
    if !status.is_success() {
        bail!(redact_secret(
            &http_error("整场会议转写结果", status, &body),
            &credentials.api_key
        ));
    }
    parse_transcript_response(&body)
}

fn delete_batch_job(credentials: &AsrProviderCredentials, remote_job_id: &str) -> Result<()> {
    let client = http_client()?;
    let mut request = client
        .delete(batch_job_url(credentials, remote_job_id)?)
        .timeout(BATCH_REQUEST_TIMEOUT);
    if !credentials.api_key.is_empty() {
        request = request.bearer_auth(&credentials.api_key);
    }
    let response = request.send().context("无法清理 ASR Server 任务")?;
    if matches!(response.status(), StatusCode::NOT_FOUND | StatusCode::GONE) {
        return Ok(());
    }
    let status = response.status();
    let body = response.text().context("无法读取 ASR Server 清理响应")?;
    if !status.is_success() {
        bail!(redact_secret(
            &http_error("清理 ASR Server 任务", status, &body),
            &credentials.api_key
        ));
    }
    Ok(())
}

fn parse_batch_status_response(
    response: reqwest::blocking::Response,
    label: &str,
    credentials: &AsrProviderCredentials,
) -> Result<BatchJobStatus> {
    let status = response.status();
    let body = response
        .text()
        .with_context(|| format!("无法读取{label}响应"))?;
    if !status.is_success() {
        bail!(redact_secret(
            &http_error(label, status, &body),
            &credentials.api_key
        ));
    }
    serde_json::from_str(&body).with_context(|| format!("{label}响应不是有效 JSON"))
}

fn persist_batch_status(
    job: &TranscriptionJob,
    storage: &Storage,
    remote: &BatchJobStatus,
) -> Result<()> {
    let (status, phase) = match remote.state.as_str() {
        "uploading" => (
            TranscriptionStatus::Preparing,
            TranscriptionProgressPhase::Uploading,
        ),
        "queued" => (
            TranscriptionStatus::Queued,
            TranscriptionProgressPhase::Queued,
        ),
        "processing" => (
            TranscriptionStatus::Transcribing,
            match remote.phase.as_str() {
                "diarizing" => TranscriptionProgressPhase::Diarizing,
                "finalizing" => TranscriptionProgressPhase::Finalizing,
                _ => TranscriptionProgressPhase::Transcribing,
            },
        ),
        "succeeded" => (
            TranscriptionStatus::Transcribing,
            TranscriptionProgressPhase::Finalizing,
        ),
        _ => return Ok(()),
    };
    let unit = match remote.progress_unit.as_str() {
        "bytes" => TranscriptionProgressUnit::Bytes,
        "windows" => TranscriptionProgressUnit::Windows,
        _ => TranscriptionProgressUnit::Steps,
    };
    storage.set_batch_transcription_progress(
        &job.recording_id,
        job.generation,
        status,
        phase,
        remote.progress_current,
        remote.progress_total,
        unit,
    )?;
    emit_current_status(&job.app, storage, &job.recording_id);
    Ok(())
}

fn ensure_batch_not_cancelled(
    cancellation: &Cancellation,
    credentials: &AsrProviderCredentials,
    remote_job_id: Option<&str>,
) -> Result<()> {
    if !cancellation.requested.load(Ordering::Acquire) {
        return Ok(());
    }
    if cancellation.remote_requested.load(Ordering::Acquire)
        && let Some(remote_job_id) = remote_job_id
    {
        let _ = cancel_batch_job(credentials, remote_job_id);
    }
    bail!("转写任务已取消")
}

fn wait_for_batch_poll(
    cancellation: &Cancellation,
    credentials: &AsrProviderCredentials,
    remote_job_id: &str,
) -> Result<()> {
    let slices = BATCH_POLL_INTERVAL.as_millis().div_ceil(100) as usize;
    for _ in 0..slices {
        ensure_batch_not_cancelled(cancellation, credentials, Some(remote_job_id))?;
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

fn batch_job_url(credentials: &AsrProviderCredentials, remote_job_id: &str) -> Result<String> {
    Ok(format!(
        "{}/nota/transcription-jobs/{remote_job_id}",
        normalize_base_url(&credentials.provider.base_url)?
    ))
}

fn remote_batch_target(
    storage: &Storage,
    recording_id: &str,
) -> Result<Option<(AsrProviderCredentials, String)>> {
    let execution = storage.transcription_execution(recording_id)?;
    if execution.protocol != TranscriptionProtocol::NotaBatchV1 {
        return Ok(None);
    }
    let Some(remote_job_id) = execution.remote_job_id else {
        return Ok(None);
    };
    let (provider_id, _, _) = storage.transcription_provider_snapshot(recording_id)?;
    let credentials = storage.find_asr_provider(&provider_id)?;
    Ok(Some((credentials, remote_job_id)))
}

fn cleanup_previous_batch_job_if_present(storage: &Storage, recording_id: &str) -> Result<()> {
    let execution = match storage.transcription_execution(recording_id) {
        Ok(execution) => execution,
        Err(_) => return Ok(()),
    };
    if execution.protocol != TranscriptionProtocol::NotaBatchV1 {
        return Ok(());
    }
    let Some(remote_job_id) = execution.remote_job_id else {
        return Ok(());
    };
    let (provider_id, _, _) = storage.transcription_provider_snapshot(recording_id)?;
    let credentials = storage.find_asr_provider(&provider_id)?;
    let _ = cancel_batch_job(&credentials, &remote_job_id);
    delete_batch_job(&credentials, &remote_job_id)
}

pub fn normalize_base_url(value: &str) -> Result<String> {
    let trimmed = value.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        bail!("API Base URL 不能为空");
    }
    let mut url = reqwest::Url::parse(trimmed).context("API Base URL 格式无效")?;
    if !matches!(url.scheme(), "http" | "https") {
        bail!("API Base URL 仅支持 http 或 https");
    }
    url.set_query(None);
    url.set_fragment(None);
    let path = url.path().trim_end_matches('/');
    let normalized_path = if path.is_empty() {
        "/v1".to_owned()
    } else if path.ends_with("/v1") {
        path.to_owned()
    } else {
        format!("{path}/v1")
    };
    url.set_path(&normalized_path);
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

pub fn list_models(credentials: &AsrProviderCredentials) -> Result<Vec<AsrModel>> {
    let base = normalize_base_url(&credentials.provider.base_url)?;
    let client = http_client()?;
    let mut request = client.get(format!("{base}/models"));
    if !credentials.api_key.is_empty() {
        request = request.bearer_auth(&credentials.api_key);
    }
    let response = request.send().context("无法连接模型接口")?;
    let status = response.status();
    let body = response.text().context("无法读取模型接口响应")?;
    if !status.is_success() {
        bail!(redact_secret(
            &http_error("模型接口", status, &body),
            &credentials.api_key
        ));
    }
    parse_models(&body)
}

pub fn test_connection(credentials: &AsrProviderCredentials) -> Result<AsrConnectionTest> {
    let mut device = None;
    let mut health_message = None;
    let mut batch_warning = None;
    if credentials.provider.kind == AsrProviderKind::FunAsr {
        let base = normalize_base_url(&credentials.provider.base_url)?;
        let root = base.strip_suffix("/v1").unwrap_or(&base);
        let client = http_client()?;
        let mut request = client.get(format!("{root}/health"));
        if !credentials.api_key.is_empty() {
            request = request.bearer_auth(&credentials.api_key);
        }
        let response = request.send().context("无法连接 FunASR 健康检查接口")?;
        let status = response.status();
        let body = response.text().unwrap_or_default();
        if !status.is_success() {
            bail!(redact_secret(
                &http_error("健康检查", status, &body),
                &credentials.api_key
            ));
        }
        if let Ok(value) = serde_json::from_str::<Value>(&body) {
            device = value
                .get("device")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            health_message = value
                .get("status")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
        }
        if let Err(error) = fetch_batch_capabilities(credentials) {
            batch_warning = Some(sanitize_error(&format!("{error:#}")));
        }
    }
    match list_models(credentials) {
        Ok(models) => Ok(AsrConnectionTest {
            reachable: true,
            level: if batch_warning.is_some() {
                AsrConnectionLevel::Warning
            } else {
                AsrConnectionLevel::Success
            },
            message: batch_warning.unwrap_or_else(|| {
                health_message
                    .map(|status| format!("服务可用（{status}），支持整场会议转写"))
                    .unwrap_or_else(|| "服务可用，支持整场会议转写".into())
            }),
            models,
            device,
        }),
        Err(error) if credentials.provider.kind == AsrProviderKind::FunAsr => {
            Ok(AsrConnectionTest {
                reachable: true,
                level: AsrConnectionLevel::Warning,
                message: match batch_warning {
                    Some(batch_warning) => {
                        format!("{batch_warning}；同时模型接口不可用：{error:#}")
                    }
                    None => format!("服务健康检查通过，但模型接口不可用：{error:#}"),
                },
                models: Vec::new(),
                device,
            })
        }
        Err(error) => Err(error),
    }
}

fn transcribe_file(
    credentials: &AsrProviderCredentials,
    path: &Path,
) -> Result<ProviderTranscript> {
    let first = send_transcription(credentials, path, "verbose_json")?;
    if first.0.is_success() {
        return parse_transcript_response(&first.1);
    }
    let response_format_rejected = matches!(
        first.0,
        StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
    ) && first.1.to_ascii_lowercase().contains("response_format");
    if response_format_rejected {
        let fallback = send_transcription(credentials, path, "json")?;
        if fallback.0.is_success() {
            return parse_transcript_response(&fallback.1);
        }
        bail!(redact_secret(
            &http_error("转写接口", fallback.0, &fallback.1),
            &credentials.api_key
        ));
    }
    bail!(redact_secret(
        &http_error("转写接口", first.0, &first.1),
        &credentials.api_key
    ))
}

fn send_transcription(
    credentials: &AsrProviderCredentials,
    path: &Path,
    response_format: &str,
) -> Result<(StatusCode, String)> {
    send_transcription_with_timeout(credentials, path, response_format, REQUEST_TIMEOUT)
}

fn send_transcription_with_timeout(
    credentials: &AsrProviderCredentials,
    path: &Path,
    response_format: &str,
    timeout: Duration,
) -> Result<(StatusCode, String)> {
    let base = normalize_base_url(&credentials.provider.base_url)?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("nota-chunk.wav")
        .to_owned();
    let file = File::open(path)?;
    let part = Part::reader_with_length(file, std::fs::metadata(path)?.len())
        .file_name(file_name)
        .mime_str("audio/wav")?;
    let form = Form::new()
        .part("file", part)
        .text("model", credentials.provider.model_id.clone())
        .text("response_format", response_format.to_owned());
    let client = http_client()?;
    let mut request = client
        .post(format!("{base}/audio/transcriptions"))
        .timeout(timeout)
        .multipart(form);
    if !credentials.api_key.is_empty() {
        request = request.bearer_auth(&credentials.api_key);
    }
    let response = request.send().context("无法连接转写服务")?;
    let status = response.status();
    let body = response.text().context("无法读取转写服务响应")?;
    Ok((status, body))
}

fn http_client() -> Result<Client> {
    Ok(Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .user_agent(format!("Nota/{}", env!("CARGO_PKG_VERSION")))
        .build()?)
}

fn parse_models(body: &str) -> Result<Vec<AsrModel>> {
    let value: Value = serde_json::from_str(body).context("模型接口返回了无效 JSON")?;
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .context("模型接口响应中缺少 data 数组")?;
    Ok(data
        .iter()
        .filter_map(|item| {
            Some(AsrModel {
                id: item.get("id")?.as_str()?.to_owned(),
                owned_by: item
                    .get("owned_by")
                    .or_else(|| item.get("ownedBy"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                ready: item.get("ready").and_then(Value::as_bool),
            })
        })
        .collect())
}

#[derive(Deserialize)]
struct RawTranscript {
    #[serde(default)]
    text: String,
    language: Option<String>,
    #[serde(default)]
    segments: Vec<RawSegment>,
}

#[derive(Deserialize)]
struct RawSegment {
    #[serde(default)]
    start: f64,
    #[serde(default)]
    end: f64,
    #[serde(default)]
    text: String,
    speaker: Option<Value>,
}

fn parse_transcript_response(body: &str) -> Result<ProviderTranscript> {
    if let Ok(raw) = serde_json::from_str::<RawTranscript>(body) {
        if raw.text.trim().is_empty() && raw.segments.is_empty() {
            bail!("转写接口响应中没有文字");
        }
        let segments = raw
            .segments
            .into_iter()
            .filter(|segment| !segment.text.trim().is_empty())
            .map(|segment| TranscriptSegment {
                start_ms: seconds_to_ms(segment.start),
                end_ms: seconds_to_ms(segment.end).max(seconds_to_ms(segment.start)),
                text: segment.text.trim().to_owned(),
                speaker: speaker_label(segment.speaker),
            })
            .collect();
        return Ok(ProviderTranscript {
            text: raw.text.trim().to_owned(),
            language: raw.language,
            segments,
        });
    }
    let text = body.trim().trim_matches('"').trim();
    if text.is_empty() {
        bail!("转写接口返回了空响应");
    }
    Ok(ProviderTranscript {
        text: text.to_owned(),
        language: None,
        segments: Vec::new(),
    })
}

fn speaker_label(value: Option<Value>) -> Option<String> {
    match value? {
        Value::String(value) if !value.trim().is_empty() => Some(value),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn seconds_to_ms(value: f64) -> u64 {
    if value.is_finite() && value > 0.0 {
        (value * 1_000.0).round().min(u64::MAX as f64) as u64
    } else {
        0
    }
}

fn merge_chunks(
    chunks: Vec<StoredTranscriptionChunk>,
) -> (String, Vec<TranscriptSegment>, Option<String>) {
    let mut text = String::new();
    let mut segments = Vec::new();
    let mut language = None;
    let mut previous_end = 0u64;
    for chunk in chunks {
        if language.is_none() {
            language = chunk.language;
        }
        if chunk.segments.is_empty() {
            append_without_overlap(&mut text, chunk.text.trim());
        } else {
            for mut segment in chunk.segments {
                segment.start_ms = segment.start_ms.saturating_add(chunk.start_ms);
                segment.end_ms = segment.end_ms.saturating_add(chunk.start_ms);
                let midpoint = segment.start_ms.saturating_add(segment.end_ms) / 2;
                if !segments.is_empty() && midpoint <= previous_end {
                    continue;
                }
                append_without_overlap(&mut text, segment.text.trim());
                segments.push(segment);
            }
        }
        previous_end = previous_end.max(chunk.end_ms);
    }
    (text.trim().to_owned(), segments, language)
}

fn append_without_overlap(output: &mut String, next: &str) {
    if next.is_empty() {
        return;
    }
    if output.is_empty() {
        output.push_str(next);
        return;
    }
    let max = output.chars().count().min(next.chars().count()).min(240);
    let output_chars: Vec<char> = output.chars().collect();
    let next_chars: Vec<char> = next.chars().collect();
    let overlap = (1..=max)
        .rev()
        .find(|length| output_chars[output_chars.len() - length..] == next_chars[..*length])
        .unwrap_or(0);
    let remainder: String = next_chars[overlap..].iter().collect();
    if !remainder.trim().is_empty() {
        if overlap == 0 {
            output.push('\n');
        }
        output.push_str(remainder.trim_start());
    }
}

fn chunk_count(duration_ms: u64) -> u32 {
    if duration_ms <= CHUNK_DURATION_MS {
        return 1;
    }
    let stride = CHUNK_DURATION_MS - CHUNK_OVERLAP_MS;
    let remaining = duration_ms - CHUNK_DURATION_MS;
    (1 + remaining.div_ceil(stride)).min(u32::MAX as u64) as u32
}

fn chunk_start_ms(index: u32) -> u64 {
    index as u64 * (CHUNK_DURATION_MS - CHUNK_OVERLAP_MS)
}

fn ensure_not_cancelled(cancellation: &AtomicBool) -> Result<()> {
    if cancellation.load(Ordering::Acquire) {
        bail!("转写任务已取消");
    }
    Ok(())
}

fn emit_current_status(app: &AppHandle, storage: &Storage, recording_id: &str) {
    if let Ok(summary) = storage.transcription_summary(recording_id) {
        emit_status(app, recording_id, summary);
    }
}

fn emit_status(app: &AppHandle, recording_id: &str, summary: TranscriptionSummary) {
    let _ = app.emit(
        "asr://status",
        TranscriptionEvent {
            recording_id: recording_id.to_owned(),
            summary,
        },
    );
}

fn http_error(component: &str, status: StatusCode, body: &str) -> String {
    let detail = sanitize_error(body);
    if detail.is_empty() {
        format!("{component}返回 HTTP {}", status.as_u16())
    } else {
        format!("{component}返回 HTTP {}：{detail}", status.as_u16())
    }
}

fn sanitize_error(value: &str) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    compact.chars().take(600).collect()
}

fn redact_secret(message: &str, secret: &str) -> String {
    if secret.is_empty() {
        message.to_owned()
    } else {
        message.replace(secret, "[REDACTED]")
    }
}

pub fn remove_temporary_chunks(recovery_directory: &Path, recording_id: &str) -> Result<()> {
    let directory = recovery_directory.join("TranscriptionTemp");
    let Ok(entries) = std::fs::read_dir(&directory) else {
        return Ok(());
    };
    let prefix = format!("{recording_id}-");
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let is_target = path.is_file()
            && path.extension().and_then(|value| value.to_str()) == Some("wav")
            && path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.starts_with(&prefix));
        if is_target {
            std::fs::remove_file(path)?;
        }
    }
    Ok(())
}

pub fn clean_stale_temporary_chunks(recovery_directory: &Path) -> Result<()> {
    let directory = recovery_directory.join("TranscriptionTemp");
    let Ok(entries) = std::fs::read_dir(&directory) else {
        return Ok(());
    };
    for entry in entries {
        let path = entry?.path();
        if path.is_file() && path.extension().and_then(|value| value.to_str()) == Some("wav") {
            std::fs::remove_file(path)?;
        }
    }
    Ok(())
}

pub(crate) struct OggPcm16Reader {
    packets: PacketReader<BufReader<File>>,
    decoder: Decoder,
    pending: VecDeque<i16>,
    pre_skip: usize,
    emitted: u64,
    maximum_samples: u64,
    reached_end: bool,
}

impl OggPcm16Reader {
    pub(crate) fn open(path: &Path, duration_ms: u64) -> Result<Self> {
        let file =
            File::open(path).with_context(|| format!("无法打开录音文件 {}", path.display()))?;
        Ok(Self {
            packets: PacketReader::new(BufReader::new(file)),
            decoder: Decoder::new(48_000, Channels::Mono)?,
            pending: VecDeque::new(),
            pre_skip: 0,
            emitted: 0,
            maximum_samples: if duration_ms == 0 {
                u64::MAX
            } else {
                duration_ms.saturating_mul(ASR_SAMPLE_RATE as u64) / 1_000
            },
            reached_end: false,
        })
    }

    pub(crate) fn read_samples(&mut self, wanted: usize) -> Result<Vec<i16>> {
        while self.pending.len() < wanted && !self.reached_end {
            self.decode_next_packet()?;
        }
        let remaining = self.maximum_samples.saturating_sub(self.emitted) as usize;
        let count = wanted.min(self.pending.len()).min(remaining);
        let mut output = Vec::with_capacity(count);
        for _ in 0..count {
            if let Some(sample) = self.pending.pop_front() {
                output.push(sample);
            }
        }
        self.emitted = self.emitted.saturating_add(output.len() as u64);
        if self.emitted >= self.maximum_samples {
            self.reached_end = true;
            self.pending.clear();
        }
        Ok(output)
    }

    fn decode_next_packet(&mut self) -> Result<()> {
        let Some(packet) = self.packets.read_packet()? else {
            self.reached_end = true;
            return Ok(());
        };
        if packet.data.starts_with(b"OpusHead") {
            if packet.data.len() >= 12 {
                self.pre_skip = u16::from_le_bytes([packet.data[10], packet.data[11]]) as usize;
            }
            return Ok(());
        }
        if packet.data.starts_with(b"OpusTags") {
            return Ok(());
        }
        let mut decoded = vec![0.0f32; 5_760];
        let count = self
            .decoder
            .decode_float(&packet.data, &mut decoded, false)
            .context("Opus 音频解码失败")?;
        decoded.truncate(count);
        if self.pre_skip > 0 {
            let skip = self.pre_skip.min(decoded.len());
            decoded.drain(..skip);
            self.pre_skip -= skip;
        }
        for source in decoded.chunks_exact(3) {
            let value = ((source[0] + source[1] + source[2]) / 3.0).clamp(-1.0, 1.0);
            self.pending
                .push_back((value * if value < 0.0 { 32_768.0 } else { 32_767.0 }).round() as i16);
        }
        Ok(())
    }

    pub(crate) fn finished(&self) -> bool {
        self.reached_end && self.pending.is_empty()
    }
}

pub(crate) fn write_pcm16_wav(path: &Path, samples: &[i16]) -> Result<()> {
    let data_size = samples
        .len()
        .checked_mul(2)
        .context("WAV 分块过大")?
        .min(u32::MAX as usize) as u32;
    let mut output = BufWriter::new(
        OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)?,
    );
    output.write_all(b"RIFF")?;
    output.write_all(&(36u32.saturating_add(data_size)).to_le_bytes())?;
    output.write_all(b"WAVEfmt ")?;
    output.write_all(&16u32.to_le_bytes())?;
    output.write_all(&1u16.to_le_bytes())?;
    output.write_all(&1u16.to_le_bytes())?;
    output.write_all(&ASR_SAMPLE_RATE.to_le_bytes())?;
    output.write_all(&(ASR_SAMPLE_RATE * 2).to_le_bytes())?;
    output.write_all(&2u16.to_le_bytes())?;
    output.write_all(&16u16.to_le_bytes())?;
    output.write_all(b"data")?;
    output.write_all(&data_size.to_le_bytes())?;
    for sample in samples {
        output.write_all(&sample.to_le_bytes())?;
    }
    output.flush()?;
    output.get_ref().sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::OpusOggWriter;
    use crate::models::AsrProvider;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::sync::Mutex as StdMutex;
    use std::thread::JoinHandle;

    fn credentials(base_url: String, api_key: &str) -> AsrProviderCredentials {
        AsrProviderCredentials {
            provider: AsrProvider {
                id: "provider".into(),
                name: "Mock ASR".into(),
                kind: AsrProviderKind::OpenAiCompatible,
                base_url,
                model_id: "mock-model".into(),
                has_api_key: !api_key.is_empty(),
                created_at: "2026-07-28T00:00:00Z".into(),
                updated_at: "2026-07-28T00:00:00Z".into(),
            },
            api_key: api_key.into(),
        }
    }

    fn funasr_credentials(base_url: String, api_key: &str) -> AsrProviderCredentials {
        let mut credentials = credentials(base_url, api_key);
        credentials.provider.kind = AsrProviderKind::FunAsr;
        credentials
    }

    fn mock_server(
        responses: Vec<(u16, &'static str)>,
    ) -> (String, Arc<StdMutex<Vec<String>>>, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(StdMutex::new(Vec::new()));
        let collected = Arc::clone(&requests);
        let handle = std::thread::spawn(move || {
            for (status, response_body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0u8; 8_192];
                let mut expected_length = None;
                loop {
                    let count = stream.read(&mut buffer).unwrap_or(0);
                    if count == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..count]);
                    if expected_length.is_none()
                        && let Some(header_end) =
                            request.windows(4).position(|value| value == b"\r\n\r\n")
                    {
                        let headers = String::from_utf8_lossy(&request[..header_end]).to_string();
                        let content_length = headers.lines().find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        });
                        expected_length = content_length.map(|length| header_end + 4 + length);
                    }
                    if expected_length.is_some_and(|length| request.len() >= length) {
                        break;
                    }
                }
                collected
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&request).into_owned());
                let reason = match status {
                    200 => "OK",
                    400 => "Bad Request",
                    401 => "Unauthorized",
                    422 => "Unprocessable Entity",
                    429 => "Too Many Requests",
                    _ => "Internal Server Error",
                };
                let response = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response_body}",
                    response_body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        (format!("http://{address}/v1"), requests, handle)
    }

    fn test_wav() -> PathBuf {
        let path = std::env::temp_dir().join(format!("nota-asr-http-{}.wav", uuid::Uuid::new_v4()));
        write_pcm16_wav(&path, &[0; 160]).unwrap();
        path
    }

    #[test]
    fn normalizes_api_root_to_v1() {
        assert_eq!(
            normalize_base_url("http://127.0.0.1:8000").unwrap(),
            "http://127.0.0.1:8000/v1"
        );
        assert_eq!(
            normalize_base_url("https://api.example.test/v1/").unwrap(),
            "https://api.example.test/v1"
        );
    }

    #[test]
    fn validates_nota_batch_capabilities_and_authenticates_the_probe() {
        let (base_url, requests, server) = mock_server(vec![(
            200,
            r#"{"batch_transcription_version":"1","upload_chunk_bytes":8388608,"max_upload_bytes":100000000,"max_audio_seconds":14400,"audio_formats":["ogg"]}"#,
        )]);
        let capabilities =
            fetch_batch_capabilities(&funasr_credentials(base_url, "local-secret")).unwrap();
        server.join().unwrap();

        assert_eq!(capabilities.upload_chunk_bytes, 8 * 1024 * 1024);
        let request = &requests.lock().unwrap()[0];
        assert!(request.starts_with("GET /v1/nota/capabilities "));
        assert!(request.contains("authorization: Bearer local-secret"));
    }

    #[test]
    fn discovers_clean_speaker_sample_capabilities() {
        let (base_url, requests, server) = mock_server(vec![(
            200,
            r#"{"batch_transcription_version":"1","upload_chunk_bytes":8388608,"max_upload_bytes":100000000,"max_audio_seconds":14400,"audio_formats":["ogg"],"speaker_embedding_version":"1","speaker_embedding_max_bytes":2097152,"speaker_embedding_min_seconds":5,"speaker_embedding_max_seconds":30,"speaker_sample_analysis_version":"1","speaker_sample_analysis_max_files":8,"speaker_sample_analysis_min_clip_seconds":3,"speaker_sample_analysis_max_clip_seconds":12,"speaker_sample_analysis_max_total_seconds":30,"speaker_sample_analysis_min_accepted_seconds":5,"speaker_sample_analysis_min_purity":0.7}"#,
        )]);
        let credentials = funasr_credentials(base_url, "local-secret");
        let capabilities = fetch_speaker_embedding_capabilities(&credentials).unwrap();
        server.join().unwrap();

        assert_eq!(capabilities.analysis_max_files, 8);
        assert_eq!(capabilities.analysis_min_accepted_seconds, 5);
        assert_eq!(capabilities.analysis_min_purity, 0.7);
        let requests = requests.lock().unwrap();
        assert!(requests[0].starts_with("GET /v1/nota/capabilities "));
        assert!(requests[0].contains("authorization: Bearer local-secret"));
    }

    #[test]
    fn calls_the_clean_speaker_sample_analysis_extension() {
        let (base_url, requests, server) = mock_server(vec![(
            200,
            r#"{"schema_version":"1","outcome":"enrollable","embedding_model":"cam++","embedding_fingerprint":"cam++:test:v1","dimension":2,"audio_duration":10.0,"accepted_audio_duration":7.5,"purity_score":0.9,"preview":{"file_index":1,"start":0.5,"end":4.0},"accepted_ranges":[{"file_index":0,"start":0.25,"end":4.25},{"file_index":1,"start":0.5,"end":4.0}],"embedding":[3.0,4.0]}"#,
        )]);
        let first = test_wav();
        let second = test_wav();

        let result = analyze_speaker_samples(
            &funasr_credentials(base_url, "local-secret"),
            &[first.clone(), second.clone()],
        )
        .unwrap();
        server.join().unwrap();

        assert_eq!(result.outcome, SpeakerSampleAnalysisOutcome::Enrollable);
        assert_eq!(result.embedding, Some(vec![0.6, 0.8]));
        assert_eq!(result.accepted_audio_ms, 7_500);
        assert_eq!(result.preview.file_index, 1);
        assert_eq!(result.preview.start_ms, 500);
        assert_eq!(result.accepted_ranges.len(), 2);
        let request = &requests.lock().unwrap()[0];
        assert!(request.starts_with("POST /v1/nota/speaker-samples/analyze "));
        assert_eq!(request.matches("name=\"files\"").count(), 2);
        std::fs::remove_file(first).unwrap();
        std::fs::remove_file(second).unwrap();
    }

    #[test]
    fn accepts_preview_only_speaker_sample_analysis() {
        let (base_url, _, server) = mock_server(vec![(
            200,
            r#"{"schema_version":"1","outcome":"preview_only","embedding_model":"cam++","embedding_fingerprint":"cam++:test:v1","dimension":0,"audio_duration":6.0,"accepted_audio_duration":0.0,"purity_score":0.45,"preview":{"file_index":0,"start":0.25,"end":4.25},"accepted_ranges":[],"embedding":null}"#,
        )]);
        let sample = test_wav();

        let result = analyze_speaker_samples(
            &funasr_credentials(base_url, "local-secret"),
            std::slice::from_ref(&sample),
        )
        .unwrap();
        server.join().unwrap();

        assert_eq!(result.outcome, SpeakerSampleAnalysisOutcome::PreviewOnly);
        assert_eq!(result.embedding, None);
        assert_eq!(result.preview.start_ms, 250);
        assert!(result.accepted_ranges.is_empty());
        std::fs::remove_file(sample).unwrap();
    }

    #[test]
    fn rejects_a_server_without_the_speaker_embedding_capability() {
        let (base_url, _, server) = mock_server(vec![(
            200,
            r#"{"batch_transcription_version":"1","upload_chunk_bytes":8388608,"max_upload_bytes":100000000,"max_audio_seconds":14400,"audio_formats":["ogg"]}"#,
        )]);

        let error =
            fetch_speaker_embedding_capabilities(&funasr_credentials(base_url, "")).unwrap_err();
        server.join().unwrap();

        assert!(format!("{error:#}").contains("不支持说话人声纹提取"));
    }

    #[test]
    fn funasr_connection_warns_when_the_server_lacks_batch_v1() {
        let (base_url, requests, server) = mock_server(vec![
            (200, r#"{"status":"ok"}"#),
            (404, r#"{"error":{"message":"not found"}}"#),
            (
                200,
                r#"{"object":"list","data":[{"id":"sensevoice","owned_by":"nota","ready":true}]}"#,
            ),
        ]);
        let result = test_connection(&funasr_credentials(base_url, "")).unwrap();
        server.join().unwrap();

        assert!(result.reachable);
        assert_eq!(result.level, AsrConnectionLevel::Warning);
        assert!(result.message.contains("版本过旧"));
        assert_eq!(requests.lock().unwrap().len(), 3);
    }

    #[test]
    fn creates_a_diarized_whole_meeting_job_with_an_idempotency_key() {
        let response = r#"{"id":"remote-job","state":"uploading","phase":"uploading","upload_offset":0,"upload_length":3,"progress_current":0,"progress_total":3,"progress_unit":"bytes","expires_at":"2026-07-31T00:00:00Z","error":null}"#;
        let (base_url, requests, server) = mock_server(vec![(201, response)]);
        let path =
            std::env::temp_dir().join(format!("nota-asr-batch-{}.ogg", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"ogg").unwrap();
        let created = create_batch_job(
            &funasr_credentials(base_url, "local-secret"),
            &path,
            3,
            "stable-idempotency-key",
            Some(3),
        )
        .unwrap();
        server.join().unwrap();

        assert_eq!(created.id, "remote-job");
        let request = &requests.lock().unwrap()[0];
        assert!(request.starts_with("POST /v1/nota/transcription-jobs "));
        assert!(request.contains("idempotency-key: stable-idempotency-key"));
        assert!(request.contains(r#""diarization":true"#));
        assert!(request.contains(r#""response_format":"verbose_json""#));
        assert!(request.contains(r#""speaker_count":3"#));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn parses_verbose_response_with_numeric_speaker() {
        let transcript = parse_transcript_response(
            r#"{"text":"你好","language":"zh","segments":[{"start":1.2,"end":2.5,"text":"你好","speaker":2}]}"#,
        )
        .unwrap();
        assert_eq!(transcript.text, "你好");
        assert_eq!(transcript.segments[0].start_ms, 1_200);
        assert_eq!(transcript.segments[0].speaker.as_deref(), Some("2"));
    }

    #[test]
    fn chunks_use_two_second_overlap() {
        assert_eq!(chunk_count(1), 1);
        assert_eq!(chunk_count(CHUNK_DURATION_MS), 1);
        assert_eq!(chunk_count(CHUNK_DURATION_MS + 1), 2);
        assert_eq!(chunk_start_ms(1), 598_000);
    }

    #[test]
    fn removes_text_overlap_for_chinese_and_plain_text() {
        let mut text = "这是第一段的结尾".to_owned();
        append_without_overlap(&mut text, "一段的结尾，也是第二段开头");
        assert_eq!(text, "这是第一段的结尾，也是第二段开头");
    }

    #[test]
    fn offsets_segment_timestamps_and_discards_overlap_duplicates() {
        let (text, segments, language) = merge_chunks(vec![
            StoredTranscriptionChunk {
                index: 0,
                start_ms: 0,
                end_ms: 600_000,
                text: "第一段".into(),
                segments: vec![TranscriptSegment {
                    start_ms: 599_000,
                    end_ms: 600_000,
                    text: "第一段".into(),
                    speaker: None,
                }],
                language: Some("zh".into()),
            },
            StoredTranscriptionChunk {
                index: 1,
                start_ms: 598_000,
                end_ms: 700_000,
                text: "重复 新内容".into(),
                segments: vec![
                    TranscriptSegment {
                        start_ms: 0,
                        end_ms: 1_000,
                        text: "重复".into(),
                        speaker: None,
                    },
                    TranscriptSegment {
                        start_ms: 3_000,
                        end_ms: 4_000,
                        text: "新内容".into(),
                        speaker: Some("speaker_2".into()),
                    },
                ],
                language: None,
            },
        ]);
        assert_eq!(text, "第一段\n新内容");
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[1].start_ms, 601_000);
        assert_eq!(segments[1].speaker.as_deref(), Some("speaker_2"));
        assert_eq!(language.as_deref(), Some("zh"));
    }

    #[test]
    fn writes_standard_pcm16_wave_header() {
        let path = std::env::temp_dir().join(format!("nota-asr-wav-{}.wav", uuid::Uuid::new_v4()));
        write_pcm16_wav(&path, &[0, 1, -1, i16::MAX]).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(
            u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
            16_000
        );
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 8);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn decodes_ogg_opus_to_sixteen_kilohertz_pcm() {
        let path =
            std::env::temp_dir().join(format!("nota-asr-decode-{}.ogg", uuid::Uuid::new_v4()));
        let mut writer = OpusOggWriter::create(&path).unwrap();
        for frame_index in 0..100 {
            let frame = (0..480)
                .map(|sample_index| {
                    let sample = frame_index * 480 + sample_index;
                    (sample as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.25
                })
                .collect::<Vec<_>>();
            writer.push_frame(&frame).unwrap();
        }
        writer.finish().unwrap();

        let mut reader = OggPcm16Reader::open(&path, 1_000).unwrap();
        let samples = reader.read_samples(20_000).unwrap();
        assert!((15_000..=16_000).contains(&samples.len()));
        assert!(samples.iter().any(|sample| sample.unsigned_abs() > 1_000));
        assert!(reader.finished());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn sends_bearer_multipart_and_parses_verbose_json() {
        let (base_url, requests, server) = mock_server(vec![(
            200,
            r#"{"text":"hello","segments":[{"start":0,"end":1,"text":"hello","speaker":null}]}"#,
        )]);
        let wav = test_wav();
        let transcript = transcribe_file(&credentials(base_url, "local-secret"), &wav).unwrap();
        server.join().unwrap();
        let request = &requests.lock().unwrap()[0];
        assert!(request.contains("authorization: Bearer local-secret"));
        assert!(request.contains("name=\"file\""));
        assert!(request.contains("name=\"model\""));
        assert!(request.contains("mock-model"));
        assert!(request.contains("name=\"response_format\""));
        assert!(request.contains("verbose_json"));
        assert_eq!(transcript.text, "hello");
        std::fs::remove_file(wav).unwrap();
    }

    #[test]
    fn retries_only_explicit_response_format_rejections() {
        let (base_url, requests, server) = mock_server(vec![
            (
                422,
                r#"{"error":{"message":"response_format is unsupported"}}"#,
            ),
            (200, r#"{"text":"fallback"}"#),
        ]);
        let wav = test_wav();
        let transcript = transcribe_file(&credentials(base_url, ""), &wav).unwrap();
        server.join().unwrap();
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].contains("verbose_json"));
        assert!(requests[1].contains("\r\njson\r\n"));
        assert_eq!(transcript.text, "fallback");
        std::fs::remove_file(wav).unwrap();
    }

    #[test]
    fn does_not_retry_auth_rate_limit_or_server_errors() {
        for status in [401, 429, 500] {
            let (base_url, requests, server) =
                mock_server(vec![(status, r#"{"error":{"message":"failure"}}"#)]);
            let wav = test_wav();
            let error = transcribe_file(&credentials(base_url, ""), &wav).unwrap_err();
            server.join().unwrap();
            assert!(format!("{error:#}").contains(&status.to_string()));
            assert_eq!(requests.lock().unwrap().len(), 1);
            std::fs::remove_file(wav).unwrap();
        }
    }

    #[test]
    fn transcription_request_respects_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (_stream, _) = listener.accept().unwrap();
            std::thread::sleep(Duration::from_millis(120));
        });
        let wav = test_wav();
        let result = send_transcription_with_timeout(
            &credentials(format!("http://{address}/v1"), ""),
            &wav,
            "verbose_json",
            Duration::from_millis(20),
        );
        assert!(result.is_err());
        assert!(
            format!("{:#}", result.unwrap_err())
                .to_ascii_lowercase()
                .contains("timed out")
        );
        server.join().unwrap();
        std::fs::remove_file(wav).unwrap();
    }
}
