use crate::audio::{OpusOggWriter, SAMPLE_RATE};
use crate::logging::{self, Field, FieldKey};
use crate::models::{
    AudioImportBatchSnapshot, AudioImportBatchStatus, AudioImportItemSnapshot,
    AudioImportItemStatus, RecordingItem,
};
use crate::storage::{AudioImportJob, Storage, sanitize_path_component};
use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use parking_lot::Mutex;
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

const IMPORT_EVENT: &str = "audio-import://status";
const MAX_IMPORT_FILES: usize = 100;
const HASH_BUFFER_BYTES: usize = 1024 * 1024;
const MINIMUM_FREE_BYTES: u64 = 64 * 1024 * 1024;
const RESAMPLE_CHUNK_FRAMES: usize = 4_096;

struct ImportRuntime {
    cancel: Arc<AtomicBool>,
    worker: JoinHandle<()>,
}

pub struct AudioImportManager {
    storage: Arc<Storage>,
    recording_active: Arc<dyn Fn() -> bool + Send + Sync>,
    snapshot: Arc<Mutex<Option<AudioImportBatchSnapshot>>>,
    runtime: Mutex<Option<ImportRuntime>>,
}

enum ImportOneOutcome {
    Completed(RecordingItem),
    Skipped(RecordingItem),
}

impl AudioImportManager {
    pub fn new(
        storage: Arc<Storage>,
        recording_active: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<Self> {
        recover_incomplete_imports(&storage)?;
        Ok(Self {
            storage,
            recording_active,
            snapshot: Arc::new(Mutex::new(None)),
            runtime: Mutex::new(None),
        })
    }

    pub fn start(
        self: &Arc<Self>,
        app: AppHandle,
        paths: Vec<String>,
    ) -> Result<AudioImportBatchSnapshot> {
        self.reap_finished()?;
        if (self.recording_active)() {
            bail!("请先停止并保存当前录音，再导入音频文件");
        }
        let runtime_exists = self.runtime.lock().is_some();
        if runtime_exists || self.has_active() {
            bail!("已有音频正在导入，请等待完成或先停止导入");
        }
        if paths.is_empty() {
            bail!("请选择要导入的音频文件");
        }
        if paths.len() > MAX_IMPORT_FILES {
            bail!("一次最多导入 {MAX_IMPORT_FILES} 个音频文件");
        }

        let batch_id = Uuid::new_v4().to_string();
        let items = paths
            .iter()
            .map(|path| AudioImportItemSnapshot {
                id: Uuid::new_v4().to_string(),
                file_name: Path::new(path)
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("音频文件")
                    .to_owned(),
                status: AudioImportItemStatus::Queued,
                progress_current_ms: 0,
                progress_total_ms: 0,
                error_message: None,
                recording_id: None,
            })
            .collect::<Vec<_>>();
        let initial = AudioImportBatchSnapshot {
            id: batch_id,
            status: AudioImportBatchStatus::Running,
            current_index: 0,
            total: items.len().min(u32::MAX as usize) as u32,
            completed: 0,
            failed: 0,
            skipped: 0,
            items,
        };
        *self.snapshot.lock() = Some(initial.clone());
        emit_snapshot(&app, &initial);
        logging::info(
            "audio_import",
            "batch_started",
            &[
                Field::text(FieldKey::BatchId, initial.id.as_str()),
                Field::number(FieldKey::Count, initial.total as u64),
            ],
        );

        let manager = Arc::clone(self);
        let worker_batch_id = initial.id.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let worker = std::thread::Builder::new()
            .name("nota-audio-import".into())
            .spawn(move || manager.run_batch(app, paths, worker_batch_id, worker_cancel))?;
        *self.runtime.lock() = Some(ImportRuntime { cancel, worker });
        Ok(initial)
    }

    pub fn snapshot(&self) -> Option<AudioImportBatchSnapshot> {
        self.snapshot.lock().clone()
    }

    pub fn has_active(&self) -> bool {
        let snapshot_running = self
            .snapshot
            .lock()
            .as_ref()
            .is_some_and(|snapshot| snapshot.status == AudioImportBatchStatus::Running);
        snapshot_running
            || self
                .runtime
                .lock()
                .as_ref()
                .is_some_and(|runtime| !runtime.worker.is_finished())
    }

    pub fn cancel(&self) -> Result<AudioImportBatchSnapshot> {
        let runtime = self.runtime.lock();
        let Some(runtime) = runtime.as_ref() else {
            return self.snapshot().context("当前没有正在进行的音频导入任务");
        };
        if let Some(snapshot) = self.snapshot() {
            logging::info(
                "audio_import",
                "cancel_requested",
                &[Field::text(FieldKey::BatchId, snapshot.id.as_str())],
            );
        }
        runtime.cancel.store(true, Ordering::Release);
        self.snapshot().context("当前没有正在进行的音频导入任务")
    }

    pub fn interrupt_all(&self) -> Result<()> {
        let runtime = self.runtime.lock().take();
        if let Some(runtime) = runtime {
            runtime.cancel.store(true, Ordering::Release);
            runtime
                .worker
                .join()
                .map_err(|_| anyhow!("音频导入线程异常结束"))?;
        }
        Ok(())
    }

    fn reap_finished(&self) -> Result<()> {
        let finished = self
            .runtime
            .lock()
            .as_ref()
            .is_some_and(|runtime| runtime.worker.is_finished());
        if finished {
            let runtime = self.runtime.lock().take();
            if let Some(runtime) = runtime {
                runtime
                    .worker
                    .join()
                    .map_err(|_| anyhow!("音频导入线程异常结束"))?;
            }
        }
        Ok(())
    }

    fn run_batch(
        &self,
        app: AppHandle,
        paths: Vec<String>,
        batch_id: String,
        cancel: Arc<AtomicBool>,
    ) {
        let started_at = Instant::now();
        for (index, path) in paths.into_iter().enumerate() {
            if cancel.load(Ordering::Acquire) {
                break;
            }
            self.update_snapshot(&app, |snapshot| {
                snapshot.current_index = (index + 1).min(u32::MAX as usize) as u32;
                if let Some(item) = snapshot.items.get_mut(index) {
                    item.status = AudioImportItemStatus::Probing;
                }
            });

            let outcome = import_one(
                &self.storage,
                Path::new(&path),
                &cancel,
                |current_ms, total_ms| {
                    self.update_snapshot(&app, |snapshot| {
                        if let Some(item) = snapshot.items.get_mut(index) {
                            item.status = AudioImportItemStatus::Decoding;
                            item.progress_current_ms = current_ms;
                            item.progress_total_ms = total_ms;
                        }
                    });
                },
                || {
                    self.update_snapshot(&app, |snapshot| {
                        if let Some(item) = snapshot.items.get_mut(index) {
                            item.status = AudioImportItemStatus::Finalizing;
                        }
                    });
                },
            );

            match outcome {
                Ok(ImportOneOutcome::Completed(recording)) => {
                    let recording_id = recording.id.clone();
                    self.update_snapshot(&app, |snapshot| {
                        snapshot.completed = snapshot.completed.saturating_add(1);
                        if let Some(item) = snapshot.items.get_mut(index) {
                            item.status = AudioImportItemStatus::Completed;
                            item.recording_id = Some(recording.id);
                        }
                    });
                    logging::info(
                        "audio_import",
                        "item_completed",
                        &[
                            Field::text(FieldKey::BatchId, batch_id.as_str()),
                            Field::text(FieldKey::RecordingId, recording_id),
                        ],
                    );
                }
                Ok(ImportOneOutcome::Skipped(recording)) => {
                    let recording_id = recording.id.clone();
                    self.update_snapshot(&app, |snapshot| {
                        snapshot.skipped = snapshot.skipped.saturating_add(1);
                        if let Some(item) = snapshot.items.get_mut(index) {
                            item.status = AudioImportItemStatus::Skipped;
                            item.recording_id = Some(recording.id);
                            item.error_message = Some("该文件已经导入过".into());
                        }
                    });
                    logging::info(
                        "audio_import",
                        "item_skipped",
                        &[
                            Field::text(FieldKey::BatchId, batch_id.as_str()),
                            Field::text(FieldKey::RecordingId, recording_id),
                            Field::text(FieldKey::Reason, "duplicate_content"),
                        ],
                    );
                }
                Err(_error) if cancel.load(Ordering::Acquire) => {
                    self.update_snapshot(&app, |snapshot| {
                        if let Some(item) = snapshot.items.get_mut(index) {
                            item.status = AudioImportItemStatus::Cancelled;
                            item.error_message = None;
                        }
                    });
                    break;
                }
                Err(error) => {
                    self.update_snapshot(&app, |snapshot| {
                        snapshot.failed = snapshot.failed.saturating_add(1);
                        if let Some(item) = snapshot.items.get_mut(index) {
                            item.status = AudioImportItemStatus::Failed;
                            item.error_message =
                                Some(import_error_message(&error, Path::new(&path)));
                        }
                    });
                    logging::warn(
                        "audio_import",
                        "item_failed",
                        &[
                            Field::text(FieldKey::BatchId, batch_id.as_str()),
                            Field::number(FieldKey::Current, (index + 1) as u64),
                            Field::text(FieldKey::ErrorCode, "import_failed"),
                        ],
                    );
                }
            }
        }

        self.update_snapshot(&app, |snapshot| {
            if cancel.load(Ordering::Acquire) {
                snapshot.status = AudioImportBatchStatus::Cancelled;
                for item in &mut snapshot.items {
                    if item.status == AudioImportItemStatus::Queued {
                        item.status = AudioImportItemStatus::Cancelled;
                    }
                }
            } else {
                snapshot.status = AudioImportBatchStatus::Completed;
            }
        });
        if let Some(snapshot) = self.snapshot() {
            logging::info(
                "audio_import",
                if snapshot.status == AudioImportBatchStatus::Cancelled {
                    "batch_cancelled"
                } else {
                    "batch_completed"
                },
                &[
                    Field::text(FieldKey::BatchId, batch_id.as_str()),
                    Field::text(
                        FieldKey::Status,
                        match snapshot.status {
                            AudioImportBatchStatus::Running => "running",
                            AudioImportBatchStatus::Completed => "completed",
                            AudioImportBatchStatus::Cancelled => "cancelled",
                        },
                    ),
                    Field::number(
                        FieldKey::DurationMs,
                        started_at.elapsed().as_millis() as u64,
                    ),
                    Field::number(FieldKey::Total, snapshot.total as u64),
                    Field::number(FieldKey::Count, snapshot.completed as u64),
                ],
            );
        }
    }

    fn update_snapshot(&self, app: &AppHandle, update: impl FnOnce(&mut AudioImportBatchSnapshot)) {
        let value = {
            let mut snapshot = self.snapshot.lock();
            let Some(snapshot) = snapshot.as_mut() else {
                return;
            };
            update(snapshot);
            snapshot.clone()
        };
        emit_snapshot(app, &value);
    }
}

fn emit_snapshot(app: &AppHandle, snapshot: &AudioImportBatchSnapshot) {
    let _ = app.emit(IMPORT_EVENT, snapshot.clone());
}

fn import_one(
    storage: &Storage,
    source: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64, u64),
    finalizing: impl FnOnce(),
) -> Result<ImportOneOutcome> {
    ensure_not_cancelled(cancel)?;
    let canonical = std::fs::canonicalize(source).context("无法访问所选音频文件")?;
    if !canonical.is_file() {
        bail!("所选路径不是音频文件");
    }
    let source_format = supported_source_format(&canonical)?;
    let source_file_name = canonical
        .file_name()
        .and_then(|value| value.to_str())
        .context("音频文件名不是有效的 Unicode 文本")?
        .to_owned();
    let source_sha256 = hash_file(&canonical, cancel)?;
    if let Some(recording) = storage.imported_recording_by_hash(&source_sha256)? {
        return Ok(ImportOneOutcome::Skipped(recording));
    }

    let settings = storage.settings()?;
    let output_directory = PathBuf::from(settings.output_directory);
    std::fs::create_dir_all(&output_directory).context("无法创建录音保存目录")?;
    if fs2::available_space(&output_directory)? < MINIMUM_FREE_BYTES {
        bail!("录音保存目录空间不足，至少需要 64 MiB 可用空间");
    }

    let id = Uuid::new_v4().to_string();
    let title = sanitize_path_component(
        canonical
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("导入录音"),
        "导入录音",
    );
    let final_path = unique_import_path(&output_directory, &title);
    let partial_path = output_directory.join(format!(".nota-import-{id}.partial.ogg"));
    let imported_at = Utc::now();
    let created_at = source_created_at(&canonical, imported_at);
    let job = AudioImportJob {
        id: id.clone(),
        source_path: canonical.to_string_lossy().into_owned(),
        source_file_name,
        source_format: None,
        source_sha256: None,
        title,
        created_at: created_at.to_rfc3339(),
        imported_at: imported_at.to_rfc3339(),
        final_path: final_path.to_string_lossy().into_owned(),
        partial_path: partial_path.to_string_lossy().into_owned(),
        duration_ms: None,
        size_bytes: None,
        status: "writing".into(),
    };
    storage.begin_audio_import(&job)?;

    let result = (|| {
        let (duration_ms, output_path) =
            decode_to_nota_ogg(&canonical, &partial_path, cancel, &mut progress)?;
        ensure_not_cancelled(cancel)?;
        let size_bytes = std::fs::metadata(&output_path)?.len();
        storage.prepare_audio_import(
            &id,
            source_format,
            &source_sha256,
            duration_ms,
            size_bytes,
        )?;
        finalizing();
        std::fs::rename(&partial_path, &final_path).context("无法提交导入后的录音文件")?;
        storage.mark_audio_import_file_committed(&id)?;
        storage.complete_audio_import(&id)
    })();

    if result.is_err() && !final_path.exists() {
        if partial_path.exists() {
            let _ = std::fs::remove_file(&partial_path);
        }
        let _ = storage.remove_audio_import_job(&id);
    }
    result.map(ImportOneOutcome::Completed)
}

fn supported_source_format(path: &Path) -> Result<&'static str> {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("mp3") => Ok("MP3"),
        Some("m4a") => Ok("M4A"),
        Some("wav") => Ok("WAV"),
        Some("flac") => Ok("FLAC"),
        _ => bail!("暂不支持该音频格式；请选择 MP3、M4A、WAV 或 FLAC 文件"),
    }
}

fn hash_file(path: &Path, cancel: &AtomicBool) -> Result<String> {
    let mut input = BufReader::with_capacity(HASH_BUFFER_BYTES, File::open(path)?);
    let mut buffer = vec![0u8; HASH_BUFFER_BYTES];
    let mut hasher = Sha256::new();
    loop {
        ensure_not_cancelled(cancel)?;
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn decode_to_nota_ogg(
    source: &Path,
    destination: &Path,
    cancel: &AtomicBool,
    progress: &mut impl FnMut(u64, u64),
) -> Result<(u64, PathBuf)> {
    let input = File::open(source)?;
    let mss = MediaSourceStream::new(Box::new(input), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = source.extension().and_then(|value| value.to_str()) {
        hint.with_extension(extension);
    }
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .context("无法识别音频容器")?;
    let track = format
        .default_track(TrackType::Audio)
        .cloned()
        .context("文件中没有可用的音轨")?;
    let parameters = track
        .codec_params
        .as_ref()
        .and_then(|value| value.audio())
        .cloned()
        .context("音轨缺少解码参数")?;
    let source_rate = parameters.sample_rate.context("音轨缺少采样率")?;
    let total_frames = track.num_frames.unwrap_or(0);
    let total_ms = total_frames.saturating_mul(1_000) / source_rate.max(1) as u64;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&parameters, &AudioDecoderOptions::default())
        .context("不支持该音频的编码方式")?;
    let mut writer = OpusOggWriter::create(destination)?;
    let mut resampler = ImportResampler::new(source_rate)?;
    let track_id = track.id;
    let mut interleaved = Vec::<f32>::new();
    let mut decoded_frames = 0u64;
    let mut consecutive_decode_errors = 0u32;
    let mut last_space_check = Instant::now();

    loop {
        ensure_not_cancelled(cancel)?;
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(SymphoniaError::ResetRequired) => bail!("音频流在文件中途改变，暂不支持导入"),
            Err(error) => return Err(error).context("读取音频数据失败"),
        };
        if packet.track_id != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => {
                consecutive_decode_errors = 0;
                decoded
            }
            Err(SymphoniaError::DecodeError(_)) | Err(SymphoniaError::IoError(_)) => {
                consecutive_decode_errors = consecutive_decode_errors.saturating_add(1);
                if consecutive_decode_errors > 20 {
                    bail!("音频包含过多无法解码的数据");
                }
                continue;
            }
            Err(error) => return Err(error).context("音频解码失败"),
        };
        if decoded.spec().rate() != source_rate {
            bail!("音频采样率在文件中途发生变化，暂不支持导入");
        }
        let channels = decoded.spec().channels().count();
        if channels == 0 {
            continue;
        }
        interleaved.resize(decoded.samples_interleaved(), 0.0);
        decoded.copy_to_slice_interleaved(&mut interleaved);
        let mono = downmix_interleaved(&interleaved, channels);
        decoded_frames = decoded_frames.saturating_add(mono.len() as u64);
        resampler.push(&mono, &mut writer)?;
        progress(
            decoded_frames.saturating_mul(1_000) / source_rate.max(1) as u64,
            total_ms,
        );
        if last_space_check.elapsed() >= Duration::from_secs(2) {
            if fs2::available_space(destination.parent().unwrap_or_else(|| Path::new(".")))?
                < MINIMUM_FREE_BYTES
            {
                bail!("录音保存目录空间不足，导入已停止");
            }
            last_space_check = Instant::now();
        }
    }
    if decoded_frames == 0 {
        bail!("音频中没有可解码的声音");
    }
    resampler.finish(&mut writer)?;
    let output = writer.finish()?;
    let duration_ms = decoded_frames.saturating_mul(1_000) / source_rate.max(1) as u64;
    progress(duration_ms, total_ms.max(duration_ms));
    Ok((duration_ms, output))
}

struct ImportResampler {
    source_rate: u32,
    pending: Vec<f32>,
    inner: Option<SincFixedIn<f32>>,
    initial_delay: usize,
    source_frames: u64,
    emitted_frames: u64,
}

impl ImportResampler {
    fn new(source_rate: u32) -> Result<Self> {
        let inner = if source_rate == SAMPLE_RATE {
            None
        } else {
            let parameters = SincInterpolationParameters {
                sinc_len: 64,
                f_cutoff: 0.95,
                interpolation: SincInterpolationType::Linear,
                oversampling_factor: 128,
                window: WindowFunction::BlackmanHarris2,
            };
            Some(SincFixedIn::new(
                SAMPLE_RATE as f64 / source_rate.max(1) as f64,
                1.0,
                parameters,
                RESAMPLE_CHUNK_FRAMES,
                1,
            )?)
        };
        let initial_delay = inner.as_ref().map_or(0, Resampler::output_delay);
        Ok(Self {
            source_rate,
            pending: Vec::with_capacity(RESAMPLE_CHUNK_FRAMES * 2),
            inner,
            initial_delay,
            source_frames: 0,
            emitted_frames: 0,
        })
    }

    fn push(&mut self, samples: &[f32], writer: &mut OpusOggWriter) -> Result<()> {
        self.source_frames = self.source_frames.saturating_add(samples.len() as u64);
        if self.inner.is_none() {
            writer.push_frame(samples)?;
            self.emitted_frames = self.emitted_frames.saturating_add(samples.len() as u64);
            return Ok(());
        }
        self.pending.extend_from_slice(samples);
        while self.pending.len() >= RESAMPLE_CHUNK_FRAMES {
            let input = self
                .pending
                .drain(..RESAMPLE_CHUNK_FRAMES)
                .collect::<Vec<_>>();
            let output = self
                .inner
                .as_mut()
                .expect("resampler exists")
                .process(&[input], None)?;
            self.emit_output(&output[0], writer)?;
        }
        Ok(())
    }

    fn finish(&mut self, writer: &mut OpusOggWriter) -> Result<()> {
        let Some(inner) = self.inner.as_mut() else {
            return Ok(());
        };
        let input = [std::mem::take(&mut self.pending)];
        let output = inner.process_partial(Some(&input), None)?;
        self.emit_output(&output[0], writer)?;
        for _ in 0..4 {
            if self.emitted_frames >= self.target_output_frames() {
                break;
            }
            let output = self
                .inner
                .as_mut()
                .expect("resampler exists")
                .process_partial::<Vec<f32>>(None, None)?;
            self.emit_output(&output[0], writer)?;
        }
        Ok(())
    }

    fn emit_output(&mut self, samples: &[f32], writer: &mut OpusOggWriter) -> Result<()> {
        let skip = self.initial_delay.min(samples.len());
        self.initial_delay -= skip;
        let samples = &samples[skip..];
        let remaining = self
            .target_output_frames()
            .saturating_sub(self.emitted_frames)
            .min(samples.len() as u64) as usize;
        if remaining > 0 {
            writer.push_frame(&samples[..remaining])?;
            self.emitted_frames = self.emitted_frames.saturating_add(remaining as u64);
        }
        Ok(())
    }

    fn target_output_frames(&self) -> u64 {
        self.source_frames.saturating_mul(SAMPLE_RATE as u64) / self.source_rate.max(1) as u64
    }
}

fn downmix_interleaved(samples: &[f32], channels: usize) -> Vec<f32> {
    samples
        .chunks_exact(channels)
        .map(|frame| (frame.iter().copied().sum::<f32>() / channels as f32).clamp(-1.0, 1.0))
        .collect()
}

fn ensure_not_cancelled(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Acquire) {
        bail!("音频导入已取消");
    }
    Ok(())
}

fn unique_import_path(directory: &Path, title: &str) -> PathBuf {
    let first = directory.join(format!("{title}.ogg"));
    if !first.exists() {
        return first;
    }
    for suffix in 2..10_000 {
        let candidate = directory.join(format!("{title}_{suffix}.ogg"));
        if !candidate.exists() {
            return candidate;
        }
    }
    directory.join(format!("{title}_{}.ogg", Uuid::new_v4()))
}

fn source_created_at(path: &Path, imported_at: DateTime<Utc>) -> DateTime<Utc> {
    let candidate = std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .map(DateTime::<Utc>::from)
        .ok();
    candidate
        .filter(|value| *value <= imported_at + ChronoDuration::days(1))
        .unwrap_or(imported_at)
}

fn import_error_message(error: &anyhow::Error, source: &Path) -> String {
    let message = format!("{error:#}");
    let is_m4a = source
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("m4a"));
    if is_m4a && (message.contains("编码方式") || message.contains("解码")) {
        return "无法解码该音频；M4A 目前支持 AAC-LC 和 ALAC，暂不支持 HE-AAC 或受保护音频".into();
    }
    message.chars().take(500).collect()
}

fn recover_incomplete_imports(storage: &Storage) -> Result<()> {
    for job in storage.audio_import_jobs()? {
        let partial = PathBuf::from(&job.partial_path);
        let final_path = PathBuf::from(&job.final_path);
        let ready = job.source_format.is_some()
            && job.source_sha256.is_some()
            && job.duration_ms.is_some()
            && job.size_bytes.is_some();
        if ready && !final_path.exists() && partial.is_file() {
            std::fs::rename(&partial, &final_path)?;
            storage.mark_audio_import_file_committed(&job.id)?;
        }
        if ready && final_path.is_file() {
            storage.complete_audio_import(&job.id)?;
            continue;
        }
        if partial.exists() {
            let _ = std::fs::remove_file(&partial);
        }
        storage.remove_audio_import_job(&job.id)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asr::OggPcm16Reader;
    use crate::models::RecordingOrigin;
    use crate::paths::AppPaths;
    use std::fs::OpenOptions;
    use std::io::Write;

    fn write_pcm16_stereo_wave(path: &Path, sample_rate: u32, frames: usize) {
        let channels = 2u16;
        let bits_per_sample = 16u16;
        let block_align = channels * (bits_per_sample / 8);
        let byte_rate = sample_rate * block_align as u32;
        let data_bytes = frames as u32 * block_align as u32;
        let mut file = File::create(path).unwrap();
        file.write_all(b"RIFF").unwrap();
        file.write_all(&(36 + data_bytes).to_le_bytes()).unwrap();
        file.write_all(b"WAVEfmt ").unwrap();
        file.write_all(&16u32.to_le_bytes()).unwrap();
        file.write_all(&1u16.to_le_bytes()).unwrap();
        file.write_all(&channels.to_le_bytes()).unwrap();
        file.write_all(&sample_rate.to_le_bytes()).unwrap();
        file.write_all(&byte_rate.to_le_bytes()).unwrap();
        file.write_all(&block_align.to_le_bytes()).unwrap();
        file.write_all(&bits_per_sample.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&data_bytes.to_le_bytes()).unwrap();
        for index in 0..frames {
            let phase = index as f32 * 440.0 * std::f32::consts::TAU / sample_rate as f32;
            let sample = (phase.sin() * i16::MAX as f32 * 0.15) as i16;
            file.write_all(&sample.to_le_bytes()).unwrap();
            file.write_all(&sample.to_le_bytes()).unwrap();
        }
    }

    fn test_storage(root: &Path) -> Storage {
        let nota = root.join("Nota");
        let recordings = nota.join("Recordings");
        let recovery = nota.join("Recovery");
        std::fs::create_dir_all(&recordings).unwrap();
        std::fs::create_dir_all(&recovery).unwrap();
        Storage::open(AppPaths {
            recovery,
            logs: nota.join("Logs"),
            default_ai_documents: nota.join("AI Documents"),
            default_recordings: recordings,
            legacy_default_recordings: root.join("Meeting Note").join("Recordings"),
            database: nota.join("nota.db"),
        })
        .unwrap()
    }

    #[test]
    fn downmixes_stereo_without_clipping() {
        assert_eq!(
            downmix_interleaved(&[1.0, -1.0, 0.5, 0.5], 2),
            vec![0.0, 0.5]
        );
    }

    #[test]
    fn rejects_unsupported_extensions() {
        let error = supported_source_format(Path::new("meeting.amr")).unwrap_err();
        assert!(error.to_string().contains("暂不支持"));
    }

    #[test]
    fn m4a_codec_guidance_is_not_shown_for_other_formats() {
        let error = anyhow!("音频解码失败");
        assert!(import_error_message(&error, Path::new("meeting.m4a")).contains("AAC-LC"));
        assert!(!import_error_message(&error, Path::new("meeting.mp3")).contains("AAC-LC"));
    }

    #[test]
    fn import_paths_are_unique_without_overwriting() {
        let root = std::env::temp_dir().join(format!("nota-import-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let existing = root.join("会议.ogg");
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&existing)
            .unwrap();
        assert_eq!(unique_import_path(&root, "会议"), root.join("会议_2.ogg"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn wav_is_normalized_to_readable_nota_ogg() {
        let root = std::env::temp_dir().join(format!("nota-import-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("phone.wav");
        let output = root.join("normalized.ogg");
        write_pcm16_stereo_wave(&source, 44_100, 44_100);
        let cancel = AtomicBool::new(false);
        let (duration_ms, actual_output) =
            decode_to_nota_ogg(&source, &output, &cancel, &mut |_, _| {}).unwrap();

        assert_eq!(actual_output, output);
        assert!((990..=1_010).contains(&duration_ms));
        let mut reader = OggPcm16Reader::open(&output, duration_ms).unwrap();
        let samples = reader.read_samples(16_000).unwrap();
        assert!(samples.len() > 15_000);
        assert!(samples.iter().any(|sample| *sample != 0));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn startup_commits_prepared_files_and_removes_unprepared_partials() {
        let root = std::env::temp_dir().join(format!("nota-import-test-{}", Uuid::new_v4()));
        let storage = test_storage(&root);
        let recordings = root.join("Nota").join("Recordings");
        let prepared_partial = recordings.join(".prepared.partial.ogg");
        let prepared_final = recordings.join("prepared.ogg");
        std::fs::write(&prepared_partial, b"complete normalized file").unwrap();
        let prepared = AudioImportJob {
            id: "prepared".into(),
            source_path: "C:\\Phone\\prepared.wav".into(),
            source_file_name: "prepared.wav".into(),
            source_format: None,
            source_sha256: None,
            title: "prepared".into(),
            created_at: "2026-08-11T00:00:00Z".into(),
            imported_at: "2026-08-11T01:00:00Z".into(),
            final_path: prepared_final.to_string_lossy().into_owned(),
            partial_path: prepared_partial.to_string_lossy().into_owned(),
            duration_ms: None,
            size_bytes: None,
            status: "writing".into(),
        };
        storage.begin_audio_import(&prepared).unwrap();
        storage
            .prepare_audio_import("prepared", "WAV", "prepared-hash", 1_000, 24)
            .unwrap();

        let abandoned_partial = recordings.join(".abandoned.partial.ogg");
        let abandoned_final = recordings.join("abandoned.ogg");
        std::fs::write(&abandoned_partial, b"incomplete").unwrap();
        storage
            .begin_audio_import(&AudioImportJob {
                id: "abandoned".into(),
                source_path: "C:\\Phone\\abandoned.wav".into(),
                source_file_name: "abandoned.wav".into(),
                source_format: None,
                source_sha256: None,
                title: "abandoned".into(),
                created_at: "2026-08-11T00:00:00Z".into(),
                imported_at: "2026-08-11T01:00:00Z".into(),
                final_path: abandoned_final.to_string_lossy().into_owned(),
                partial_path: abandoned_partial.to_string_lossy().into_owned(),
                duration_ms: None,
                size_bytes: None,
                status: "writing".into(),
            })
            .unwrap();

        recover_incomplete_imports(&storage).unwrap();

        assert!(prepared_final.is_file());
        assert!(!prepared_partial.exists());
        assert_eq!(
            storage.find_recording("prepared").unwrap().origin,
            RecordingOrigin::Imported
        );
        assert!(!abandoned_partial.exists());
        assert!(!abandoned_final.exists());
        assert!(storage.audio_import_jobs().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }
}
