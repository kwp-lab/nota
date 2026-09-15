use crate::ai::{
    AiManager, document_file_matches_identity, find_moved_document_version,
    list_models as list_llm_provider_models, normalize_provider_base_url,
    preview_generation_request, read_document_content, relink_document_version,
    test_connection as test_llm_connection,
};
use crate::asr::{
    AsrManager, clean_stale_temporary_chunks, list_models as list_provider_models,
    normalize_provider_base_url as normalize_asr_provider_base_url, remove_temporary_chunks,
    test_connection, transcription_options as resolve_transcription_options,
};
use crate::audio::{
    AudioMixer, AudioPacket, CaptureEvent, CaptureHandle, CaptureSource, OpusOggWriter,
    list_audio_devices as enumerate_audio_devices,
    list_capture_targets as enumerate_capture_targets, list_recoverable_files, move_verified,
    recover_ogg_file, start_capture,
};
use crate::importer::AudioImportManager;
use crate::logging::{self, Field, FieldKey};
use crate::models::*;
use crate::paths::AppPaths;
use crate::pdf_export::{export_current_webview_pdf, validate_exported_pdf};
use crate::process_icons::attach_capture_target_icons;
use crate::state_machine::{RecordingEvent, transition};
use crate::storage::Storage;
use crate::voiceprints::VoiceprintManager;
use anyhow::{Context, Result, anyhow, bail};
use chrono::{Local, Utc};
use crossbeam_channel::{Receiver, Sender, TryRecvError, unbounded};
use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, State, Webview, WebviewUrl,
    WebviewWindowBuilder,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use uuid::Uuid;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{
    ILCreateFromPathW, ILFree, SHOpenFolderAndSelectItems, ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::PCWSTR;

const MAX_PACKETS_PER_TICK: usize = 64;
const CAPTURE_PROMPT_WINDOW: &str = "capture-prompt";
const CAPTURE_PROMPT_WIDTH: f64 = 420.0;
const CAPTURE_PROMPT_MIN_HEIGHT: f64 = 170.0;
const CAPTURE_PROMPT_MAX_HEIGHT: f64 = 320.0;

struct AppState {
    storage: Arc<Storage>,
    recorder: Arc<RecordingController>,
    asr: Arc<AsrManager>,
    ai: Arc<AiManager>,
    importer: Arc<AudioImportManager>,
    voiceprints: Arc<VoiceprintManager>,
    media_start_gate: Mutex<()>,
    tray_state: Mutex<Option<(RecordingState, Option<CapturePromptKind>)>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrayControls {
    start_enabled: bool,
    toggle_label: Option<&'static str>,
    toggle_enabled: bool,
    stop_enabled: bool,
}

struct RecordingRuntime {
    session_id: String,
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    system_capture: Option<CaptureHandle>,
    microphone_capture: Option<CaptureHandle>,
    system_sender: Sender<AudioPacket>,
    microphone_sender: Sender<AudioPacket>,
    system_health: Arc<Mutex<Arc<AtomicBool>>>,
    microphone_health: Arc<Mutex<Arc<AtomicBool>>>,
    microphone_enabled: Arc<AtomicBool>,
    microphone_source_epoch: Arc<AtomicU64>,
    capture_selection: CaptureSelection,
    aec_mode: AecMode,
    auto_should_enable_aec: bool,
    worker: Option<JoinHandle<()>>,
    process_event_monitor: Option<JoinHandle<()>>,
}

struct RecordingController {
    snapshot: Arc<Mutex<RecordingSnapshot>>,
    runtime: Mutex<Option<RecordingRuntime>>,
    finalizer: Mutex<Option<JoinHandle<()>>>,
    capture_prompt: Mutex<Option<CapturePrompt>>,
    storage: Arc<Storage>,
}

impl RecordingController {
    fn new(storage: Arc<Storage>) -> Self {
        Self {
            snapshot: Arc::new(Mutex::new(RecordingSnapshot::default())),
            runtime: Mutex::new(None),
            finalizer: Mutex::new(None),
            capture_prompt: Mutex::new(None),
            storage,
        }
    }

    fn snapshot(&self) -> RecordingSnapshot {
        self.snapshot.lock().clone()
    }

    fn reap_finalizer(&self) -> Result<()> {
        let finished = self
            .finalizer
            .lock()
            .as_ref()
            .is_some_and(JoinHandle::is_finished);
        if finished {
            self.wait_for_finalizer()?;
        }
        Ok(())
    }

    fn wait_for_finalizer(&self) -> Result<()> {
        let finalizer = self.finalizer.lock().take();
        if let Some(finalizer) = finalizer {
            finalizer
                .join()
                .map_err(|_| anyhow!("录音收尾线程异常结束"))?;
        }
        Ok(())
    }

    fn is_active(&self) -> bool {
        self.runtime.lock().is_some()
            || self
                .finalizer
                .lock()
                .as_ref()
                .is_some_and(|finalizer| !finalizer.is_finished())
            || matches!(
                self.snapshot.lock().state,
                RecordingState::Preparing
                    | RecordingState::Recording
                    | RecordingState::Paused
                    | RecordingState::Interrupted
                    | RecordingState::Finalizing
                    | RecordingState::Recovering
            )
    }

    fn start(
        self: &Arc<Self>,
        app: AppHandle,
        request: StartRecordingRequest,
    ) -> Result<RecordingSnapshot> {
        self.reap_finalizer()?;
        if self.is_active() {
            return Ok(self.snapshot());
        }
        let output_directory = PathBuf::from(request.output_directory.trim());
        if request.output_directory.trim().is_empty() {
            bail!("请选择录音保存目录");
        }
        std::fs::create_dir_all(&output_directory)
            .with_context(|| format!("无法创建保存目录 {}", output_directory.display()))?;
        let available = fs2::available_space(&output_directory)?;
        let recovery_available = fs2::available_space(&self.storage.paths().recovery)?;
        if available.min(recovery_available) < 50 * 1024 * 1024 {
            bail!("保存磁盘剩余空间低于 50 MB，无法安全开始录音");
        }

        let session_id = Uuid::new_v4().to_string();
        logging::info(
            "recording",
            "prepare_requested",
            &[Field::text(FieldKey::SessionId, session_id.clone())],
        );
        let started_at = Utc::now();
        {
            let mut snapshot = self.snapshot.lock();
            let preparing_state = transition(snapshot.state, RecordingEvent::Prepare)?;
            *snapshot = RecordingSnapshot {
                session_id: Some(session_id.clone()),
                state: preparing_state,
                started_at: Some(started_at.to_rfc3339()),
                active_duration_ms: 0,
                bytes_written: 0,
                output_path: None,
                system: SourceStatus {
                    healthy: false,
                    label: "会议声音".into(),
                    detail: Some("正在建立 Windows 音频回环".into()),
                },
                microphone: SourceStatus {
                    healthy: false,
                    label: "麦克风".into(),
                    detail: request.microphone.as_ref().map(|_| "正在连接".into()),
                },
                microphone_selection: request.microphone.clone(),
                aec_status: AecStatus::Disabled,
                fault: (available.min(recovery_available) < 200 * 1024 * 1024).then(|| {
                    RecordingFault {
                        component: "storage".into(),
                        code: "LOW_DISK_WARNING".into(),
                        recoverable: true,
                        user_message: "保存磁盘剩余空间低于 200 MB".into(),
                        occurred_at: Utc::now().to_rfc3339(),
                    }
                }),
            };
        }
        emit_snapshot(&app, &self.snapshot);
        logging::info(
            "recording",
            "capture_initializing",
            &[Field::text(FieldKey::SessionId, session_id.clone())],
        );

        let partial_path = self
            .storage
            .paths()
            .recovery
            .join(format!("{session_id}.partial.ogg"));
        let writer = match OpusOggWriter::create(&partial_path) {
            Ok(writer) => writer,
            Err(error) => {
                self.set_error(&app, "encoder", "CREATE_PARTIAL", &error);
                return Err(error);
            }
        };

        let process_capture = matches!(request.capture, CaptureSelection::Process { .. });
        let (microphone_tx, microphone_rx) = unbounded::<AudioPacket>();
        let (system_tx, system_rx) = unbounded::<AudioPacket>();
        let initial_microphone_source_epoch = u64::from(request.microphone.is_some());
        let system_source = capture_source_from_selection(&request.capture)?;
        // Attach process loopback before opening a Bluetooth microphone.
        // Switching a headset from A2DP to HFP can make a meeting client move
        // its render stream to a replacement process/session. Endpoint
        // loopback needs the opposite order so it binds to the post-switch HFP
        // endpoint instead of the now-silent A2DP endpoint.
        let mut system_capture = if process_capture {
            Some(
                match start_capture(
                    system_source.clone(),
                    system_tx.clone(),
                    0,
                    false,
                    Some(session_id.clone()),
                ) {
                    Ok(handle) => handle,
                    Err(error) => {
                        drop(writer);
                        let _ = std::fs::remove_file(&partial_path);
                        self.set_error(&app, "system", "CAPTURE_START_FAILED", &error);
                        bail!("无法启动所选录音来源：{error:#}。可切换到“全部系统声音”后重试")
                    }
                },
            )
        } else {
            None
        };
        let (mut microphone_capture, microphone_error) = match request.microphone.clone() {
            Some(selection) => {
                match start_capture(
                    CaptureSource::Microphone(selection),
                    microphone_tx.clone(),
                    initial_microphone_source_epoch,
                    false,
                    Some(session_id.clone()),
                ) {
                    Ok(handle) => (Some(handle), None),
                    Err(error) => (None, Some(format!("{error:#}"))),
                }
            }
            None => (None, None),
        };
        // Opening a Bluetooth microphone switches a unified Windows 11
        // endpoint from A2DP to HFP. Establish that mode before creating an
        // endpoint-loopback client, otherwise the loopback can remain attached
        // to the now-silent A2DP render path.
        if !process_capture && microphone_capture.is_some() {
            std::thread::sleep(Duration::from_millis(250));
        }

        if system_capture.is_none() {
            system_capture = Some(
                match start_capture(
                    system_source,
                    system_tx.clone(),
                    0,
                    false,
                    Some(session_id.clone()),
                ) {
                    Ok(handle) => handle,
                    Err(error) => {
                        if let Some(capture) = microphone_capture.take() {
                            capture.stop();
                        }
                        drop(writer);
                        let _ = std::fs::remove_file(&partial_path);
                        self.set_error(&app, "system", "CAPTURE_START_FAILED", &error);
                        bail!("无法启动所选录音来源：{error:#}。可切换到“全部系统声音”后重试")
                    }
                },
            );
        }
        let system_capture = system_capture.expect("capture source was initialized");
        let process_events = process_capture.then(|| system_capture.event_receiver());
        let microphone_expected = request.microphone.is_some();
        let auto_should_enable_aec = auto_should_enable_aec(&request.capture);
        let aec_enabled = microphone_capture.is_some()
            && should_enable_aec(request.aec_mode, auto_should_enable_aec);
        let system_health = Arc::new(Mutex::new(system_capture.health_flag()));
        let microphone_health = Arc::new(Mutex::new(
            microphone_capture
                .as_ref()
                .map(CaptureHandle::health_flag)
                .unwrap_or_else(|| Arc::new(AtomicBool::new(false))),
        ));
        let microphone_enabled = Arc::new(AtomicBool::new(microphone_expected));
        let microphone_source_epoch = Arc::new(AtomicU64::new(initial_microphone_source_epoch));
        let output_path = unique_recording_path(&output_directory);
        let active_microphone_selection =
            microphone_capture.as_ref().and(request.microphone.clone());

        {
            let mut snapshot = self.snapshot.lock();
            snapshot.state = transition(snapshot.state, RecordingEvent::Started)?;
            snapshot.system = SourceStatus {
                healthy: true,
                label: "会议声音".into(),
                detail: Some(match request.capture {
                    CaptureSelection::Process { .. } => "指定应用（含子进程）".into(),
                    CaptureSelection::System { .. } => "全部系统声音".into(),
                }),
            };
            snapshot.microphone = SourceStatus {
                healthy: microphone_capture.is_some(),
                label: "麦克风".into(),
                detail: microphone_error,
            };
            snapshot.microphone_selection = active_microphone_selection;
            snapshot.aec_status = if aec_enabled {
                AecStatus::Converging
            } else {
                AecStatus::Disabled
            };
        }
        emit_snapshot(&app, &self.snapshot);

        let stop = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker_paused = Arc::clone(&paused);
        let worker_snapshot = Arc::clone(&self.snapshot);
        let worker_storage = Arc::clone(&self.storage);
        let worker_app = app.clone();
        let worker_request = request.clone();
        let worker_session = session_id.clone();
        let worker_system_health = Arc::clone(&system_health);
        let worker_microphone_health = Arc::clone(&microphone_health);
        let worker_microphone_enabled = Arc::clone(&microphone_enabled);
        let worker_microphone_source_epoch = Arc::clone(&microphone_source_epoch);
        let worker = std::thread::Builder::new()
            .name("nota-mixer".into())
            .spawn(move || {
                recording_worker(
                    worker_app,
                    worker_storage,
                    worker_snapshot,
                    writer,
                    partial_path,
                    output_path,
                    worker_session,
                    started_at,
                    worker_request,
                    system_rx,
                    microphone_rx,
                    auto_should_enable_aec,
                    worker_system_health,
                    worker_microphone_health,
                    worker_microphone_enabled,
                    worker_microphone_source_epoch,
                    worker_stop,
                    worker_paused,
                );
            })?;
        let process_event_monitor =
            process_events.map(|events| self.spawn_process_event_monitor(app.clone(), events));
        *self.runtime.lock() = Some(RecordingRuntime {
            session_id,
            stop,
            paused,
            system_capture: Some(system_capture),
            microphone_capture,
            system_sender: system_tx,
            microphone_sender: microphone_tx,
            system_health,
            microphone_health,
            microphone_enabled,
            microphone_source_epoch,
            capture_selection: request.capture,
            aec_mode: request.aec_mode,
            auto_should_enable_aec,
            worker: Some(worker),
            process_event_monitor,
        });
        Ok(self.snapshot())
    }

    fn spawn_process_event_monitor(
        self: &Arc<Self>,
        app: AppHandle,
        events: Receiver<CaptureEvent>,
    ) -> JoinHandle<()> {
        let recorder = Arc::clone(self);
        std::thread::spawn(move || {
            while let Ok(event) = events.recv() {
                match event {
                    CaptureEvent::CaptureInterrupted { display_name } => {
                        if let Some(session_id) = recorder.snapshot().session_id {
                            logging::warn(
                                "capture_health",
                                "interruption_prompt_requested",
                                &[
                                    Field::text(FieldKey::SessionId, session_id),
                                    Field::text(FieldKey::Reason, "continuous_rebuild_failure"),
                                ],
                            );
                        }
                        recorder.show_capture_prompt(
                            &app,
                            display_name,
                            CapturePromptKind::CaptureInterrupted,
                        );
                    }
                    CaptureEvent::CaptureRecovered => {
                        if recorder.dismiss_capture_prompt_for_kind(
                            &app,
                            CapturePromptKind::CaptureInterrupted,
                        ) && let Some(session_id) = recorder.snapshot().session_id
                        {
                            logging::info(
                                "capture_health",
                                "interruption_prompt_dismissed_after_recovery",
                                &[Field::text(FieldKey::SessionId, session_id)],
                            );
                        }
                    }
                    CaptureEvent::ProlongedSilence { display_name } => {
                        if let Some(session_id) = recorder.snapshot().session_id {
                            logging::info(
                                "capture_health",
                                "silence_prompt_requested",
                                &[
                                    Field::text(FieldKey::SessionId, session_id),
                                    Field::text(FieldKey::Reason, "prolonged_silence"),
                                ],
                            );
                        }
                        recorder.show_capture_prompt(
                            &app,
                            display_name,
                            CapturePromptKind::ProlongedSilence,
                        );
                    }
                    CaptureEvent::AudioResumed => {
                        if recorder.dismiss_capture_prompt_for_kind(
                            &app,
                            CapturePromptKind::ProlongedSilence,
                        ) && let Some(session_id) = recorder.snapshot().session_id
                        {
                            logging::info(
                                "capture_health",
                                "silence_prompt_dismissed_after_audio_resumed",
                                &[Field::text(FieldKey::SessionId, session_id)],
                            );
                        }
                    }
                }
            }
        })
    }

    fn show_capture_prompt(&self, app: &AppHandle, target_name: String, kind: CapturePromptKind) {
        let Some(session_id) = self.snapshot.lock().session_id.clone() else {
            return;
        };
        let mut pending = self.capture_prompt.lock();
        if !should_replace_capture_prompt(pending.as_ref(), &session_id, kind) {
            return;
        }
        let prompt = CapturePrompt {
            session_id,
            target_name,
            kind,
        };
        *pending = Some(prompt.clone());
        drop(pending);
        let _ = app.emit("capture://prompt-updated", prompt);
        show_capture_prompt_window(app);
        schedule_tray_update(app, self.snapshot().state);
    }

    fn dismiss_capture_prompt(&self, app: &AppHandle) {
        if self.capture_prompt.lock().take().is_none() {
            return;
        }
        close_capture_prompt_window(app);
        schedule_tray_update(app, self.snapshot().state);
    }

    fn dismiss_capture_prompt_for_kind(&self, app: &AppHandle, kind: CapturePromptKind) -> bool {
        let mut pending = self.capture_prompt.lock();
        if !pending.as_ref().is_some_and(|prompt| prompt.kind == kind) {
            return false;
        }
        pending.take();
        drop(pending);
        close_capture_prompt_window(app);
        schedule_tray_update(app, self.snapshot().state);
        true
    }

    fn pending_capture_prompt(&self) -> Option<CapturePrompt> {
        self.capture_prompt.lock().clone()
    }

    fn pause(&self, app: &AppHandle) -> Result<RecordingSnapshot> {
        let mut runtime = self.runtime.lock();
        let Some(runtime) = runtime.as_mut() else {
            return Ok(self.snapshot());
        };
        {
            let mut snapshot = self.snapshot.lock();
            if snapshot.state == RecordingState::Paused {
                return Ok(snapshot.clone());
            }
            if !matches!(
                snapshot.state,
                RecordingState::Recording | RecordingState::Interrupted
            ) {
                bail!("当前状态不能暂停");
            }
            snapshot.state = transition(snapshot.state, RecordingEvent::Pause)?;
        }
        runtime.paused.store(true, Ordering::Release);
        if let Some(capture) = runtime.system_capture.as_ref() {
            capture.pause();
        }
        if let Some(capture) = runtime.microphone_capture.as_ref() {
            capture.pause();
        }
        logging::info(
            "recording",
            "paused",
            &recording_session_fields(&self.snapshot),
        );
        emit_snapshot(app, &self.snapshot);
        Ok(self.snapshot())
    }

    fn resume(&self, app: &AppHandle) -> Result<RecordingSnapshot> {
        let mut runtime = self.runtime.lock();
        let Some(runtime) = runtime.as_mut() else {
            return Ok(self.snapshot());
        };
        {
            let mut snapshot = self.snapshot.lock();
            if snapshot.state == RecordingState::Recording {
                return Ok(snapshot.clone());
            }
            if snapshot.state != RecordingState::Paused {
                bail!("当前状态不能继续");
            }
            snapshot.state = transition(snapshot.state, RecordingEvent::Resume)?;
            snapshot.fault = None;
        }
        runtime.paused.store(false, Ordering::Release);
        if let Some(capture) = runtime.system_capture.as_ref() {
            capture.resume();
        }
        if let Some(capture) = runtime.microphone_capture.as_ref() {
            capture.resume();
        }
        logging::info(
            "recording",
            "resumed",
            &recording_session_fields(&self.snapshot),
        );
        emit_snapshot(app, &self.snapshot);
        Ok(self.snapshot())
    }

    fn stop(&self, app: &AppHandle) -> Result<RecordingSnapshot> {
        self.reap_finalizer()?;
        if self.finalizer.lock().is_some() {
            return Ok(self.snapshot());
        }
        let mut runtime = match self.runtime.lock().take() {
            Some(runtime) => runtime,
            None => return Ok(self.snapshot()),
        };
        {
            let mut snapshot = self.snapshot.lock();
            snapshot.state = transition(snapshot.state, RecordingEvent::Stop)?;
        }
        self.capture_prompt.lock().take();
        close_capture_prompt_window(app);
        emit_snapshot(app, &self.snapshot);
        runtime.stop.store(true, Ordering::Release);
        logging::info(
            "recording",
            "finalizing",
            &recording_session_fields(&self.snapshot),
        );
        if let Some(capture) = runtime.system_capture.take() {
            logging::info(
                "audio_capture",
                "stopping",
                &[
                    Field::text(FieldKey::SessionId, runtime.session_id.clone()),
                    Field::text(FieldKey::Scope, "system"),
                ],
            );
            capture.stop();
            logging::info(
                "audio_capture",
                "stopped",
                &[
                    Field::text(FieldKey::SessionId, runtime.session_id.clone()),
                    Field::text(FieldKey::Scope, "system"),
                ],
            );
        }
        if let Some(capture) = runtime.microphone_capture.take() {
            logging::info(
                "audio_capture",
                "stopping",
                &[
                    Field::text(FieldKey::SessionId, runtime.session_id.clone()),
                    Field::text(FieldKey::Scope, "microphone"),
                ],
            );
            capture.stop();
            logging::info(
                "audio_capture",
                "stopped",
                &[
                    Field::text(FieldKey::SessionId, runtime.session_id.clone()),
                    Field::text(FieldKey::Scope, "microphone"),
                ],
            );
        }
        if let Some(monitor) = runtime.process_event_monitor.take() {
            let _ = monitor.join();
        }
        if let Some(worker) = runtime.worker.take() {
            let finalizer_app = app.clone();
            let finalizer_snapshot = Arc::clone(&self.snapshot);
            let finalizer = std::thread::Builder::new()
                .name("nota-finalizer".into())
                .spawn(move || {
                    logging::info(
                        "recording",
                        "waiting_for_worker",
                        &[Field::text(FieldKey::SessionId, runtime.session_id.clone())],
                    );
                    if worker.join().is_err() {
                        {
                            let mut value = finalizer_snapshot.lock();
                            value.state = RecordingState::Error;
                            value.fault = Some(RecordingFault {
                                component: "encoder".into(),
                                code: "FINALIZER_PANIC".into(),
                                recoverable: true,
                                user_message:
                                    "录音收尾线程异常结束，恢复文件已保留，可在重启后恢复".into(),
                                occurred_at: Utc::now().to_rfc3339(),
                            });
                        }
                        emit_snapshot(&finalizer_app, &finalizer_snapshot);
                        logging::error(
                            "recording",
                            "worker_failed",
                            &[
                                Field::text(FieldKey::SessionId, runtime.session_id.clone()),
                                Field::text(FieldKey::ErrorCode, "worker_panicked"),
                            ],
                        );
                    } else {
                        logging::info(
                            "recording",
                            "worker_stopped",
                            &[Field::text(FieldKey::SessionId, runtime.session_id.clone())],
                        );
                    }
                })
                .context("无法启动录音收尾线程")?;
            *self.finalizer.lock() = Some(finalizer);
        }
        Ok(self.snapshot())
    }

    fn stop_and_wait(&self, app: &AppHandle) -> Result<RecordingSnapshot> {
        self.stop(app)?;
        self.wait_for_finalizer()?;
        Ok(self.snapshot())
    }

    fn set_error(&self, app: &AppHandle, component: &str, code: &str, error: &anyhow::Error) {
        let mut snapshot = self.snapshot.lock();
        snapshot.state =
            transition(snapshot.state, RecordingEvent::Fail).unwrap_or(RecordingState::Error);
        snapshot.fault = Some(RecordingFault {
            component: component.into(),
            code: code.into(),
            recoverable: true,
            user_message: format!("{error:#}"),
            occurred_at: Utc::now().to_rfc3339(),
        });
        let value = snapshot.clone();
        drop(snapshot);
        let _ = app.emit("recording://snapshot", value.clone());
        schedule_tray_update(app, value.state);
    }

    fn switch_capture(
        self: &Arc<Self>,
        app: &AppHandle,
        selection: CaptureSelection,
    ) -> Result<RecordingSnapshot> {
        let mut runtime = self.runtime.lock();
        let Some(runtime) = runtime.as_mut() else {
            return Ok(self.snapshot());
        };
        let source = capture_source_from_selection(&selection)?;
        let replacement = start_capture(
            source,
            runtime.system_sender.clone(),
            0,
            runtime.paused.load(Ordering::Acquire),
            Some(runtime.session_id.clone()),
        )?;
        let replacement_events = matches!(selection, CaptureSelection::Process { .. })
            .then(|| replacement.event_receiver());
        let replacement_health = replacement.health_flag();
        *runtime.system_health.lock() = replacement_health;
        if let Some(previous) = runtime.system_capture.replace(replacement) {
            previous.stop();
        }
        if let Some(previous) = runtime.process_event_monitor.take() {
            let _ = previous.join();
        }
        runtime.process_event_monitor =
            replacement_events.map(|events| self.spawn_process_event_monitor(app.clone(), events));
        runtime.capture_selection = selection.clone();
        self.capture_prompt.lock().take();
        close_capture_prompt_window(app);
        let mut snapshot = self.snapshot.lock();
        snapshot.system = SourceStatus {
            healthy: true,
            label: "会议声音".into(),
            detail: Some(match selection {
                CaptureSelection::Process { .. } => "已切换到指定应用".into(),
                CaptureSelection::System { .. } => "已切换到全部系统声音".into(),
            }),
        };
        let value = snapshot.clone();
        drop(snapshot);
        let _ = app.emit("recording://snapshot", value.clone());
        schedule_tray_update(app, value.state);
        Ok(value)
    }

    fn set_microphone(
        &self,
        app: &AppHandle,
        selection: Option<DeviceSelection>,
    ) -> Result<RecordingSnapshot> {
        let mut runtime = self.runtime.lock();
        let Some(runtime) = runtime.as_mut() else {
            return Ok(self.snapshot());
        };
        match selection {
            Some(selection) => {
                let next_epoch =
                    next_source_epoch(runtime.microphone_source_epoch.load(Ordering::Acquire));
                let replacement = start_capture(
                    CaptureSource::Microphone(selection.clone()),
                    runtime.microphone_sender.clone(),
                    next_epoch,
                    runtime.paused.load(Ordering::Acquire),
                    Some(runtime.session_id.clone()),
                )?;
                *runtime.microphone_health.lock() = replacement.health_flag();
                runtime.microphone_enabled.store(true, Ordering::Release);
                runtime
                    .microphone_source_epoch
                    .store(next_epoch, Ordering::Release);
                if let Some(previous) = runtime.microphone_capture.replace(replacement) {
                    previous.stop();
                }
                logging::info(
                    "microphone",
                    "source_committed",
                    &[
                        Field::text(FieldKey::SessionId, runtime.session_id.clone()),
                        Field::number(FieldKey::SourceEpoch, next_epoch),
                        Field::boolean(FieldKey::Paused, runtime.paused.load(Ordering::Acquire)),
                    ],
                );
                let mut snapshot = self.snapshot.lock();
                snapshot.microphone.healthy = true;
                snapshot.microphone.detail = Some("已切换".into());
                snapshot.microphone_selection = Some(selection);
                snapshot.aec_status =
                    if should_enable_aec(runtime.aec_mode, runtime.auto_should_enable_aec) {
                        AecStatus::Converging
                    } else {
                        AecStatus::Disabled
                    };
            }
            None => {
                runtime.microphone_enabled.store(false, Ordering::Release);
                let next_epoch =
                    next_source_epoch(runtime.microphone_source_epoch.load(Ordering::Acquire));
                runtime
                    .microphone_source_epoch
                    .store(next_epoch, Ordering::Release);
                runtime
                    .microphone_health
                    .lock()
                    .store(false, Ordering::Release);
                if let Some(previous) = runtime.microphone_capture.take() {
                    previous.stop();
                }
                logging::info(
                    "microphone",
                    "disabled",
                    &[
                        Field::text(FieldKey::SessionId, runtime.session_id.clone()),
                        Field::number(FieldKey::SourceEpoch, next_epoch),
                    ],
                );
                let mut snapshot = self.snapshot.lock();
                snapshot.microphone.healthy = false;
                snapshot.microphone.detail = Some("已关闭".into());
                snapshot.microphone_selection = None;
                snapshot.aec_status = AecStatus::Disabled;
            }
        }
        restart_system_loopback_after_device_mode_change(runtime)?;
        emit_snapshot(app, &self.snapshot);
        Ok(self.snapshot())
    }
}

fn should_replace_capture_prompt(
    current: Option<&CapturePrompt>,
    session_id: &str,
    incoming_kind: CapturePromptKind,
) -> bool {
    let Some(current) = current.filter(|prompt| prompt.session_id == session_id) else {
        return true;
    };
    current.kind != incoming_kind && current.kind != CapturePromptKind::CaptureInterrupted
}

fn restart_system_loopback_after_device_mode_change(runtime: &mut RecordingRuntime) -> Result<()> {
    if !matches!(runtime.capture_selection, CaptureSelection::System { .. }) {
        return Ok(());
    }
    std::thread::sleep(Duration::from_millis(250));
    let source = capture_source_from_selection(&runtime.capture_selection)?;
    let replacement = start_capture(
        source,
        runtime.system_sender.clone(),
        0,
        runtime.paused.load(Ordering::Acquire),
        Some(runtime.session_id.clone()),
    )?;
    *runtime.system_health.lock() = replacement.health_flag();
    if let Some(previous) = runtime.system_capture.replace(replacement) {
        previous.stop();
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn recording_worker(
    app: AppHandle,
    storage: Arc<Storage>,
    snapshot: Arc<Mutex<RecordingSnapshot>>,
    mut writer: OpusOggWriter,
    partial_path: PathBuf,
    output_path: PathBuf,
    session_id: String,
    started_at: chrono::DateTime<Utc>,
    request: StartRecordingRequest,
    system_rx: Receiver<AudioPacket>,
    microphone_rx: Receiver<AudioPacket>,
    auto_should_enable_aec: bool,
    system_health: Arc<Mutex<Arc<AtomicBool>>>,
    microphone_health: Arc<Mutex<Arc<AtomicBool>>>,
    microphone_enabled: Arc<AtomicBool>,
    microphone_source_epoch: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
) {
    let mut mixer = AudioMixer::new(request.aec_mode, auto_should_enable_aec);
    let mut system_connected;
    let mut microphone_connected;
    let mut last_emit = Instant::now();
    let mut last_disk_check = Instant::now();
    let mut was_paused = false;
    let mut microphone_epoch = microphone_source_epoch.load(Ordering::Acquire);
    let mut aec_converging_ms = 0u64;
    let tick = crossbeam_channel::tick(Duration::from_millis(10));
    let mut failure: Option<anyhow::Error> = None;

    while !stop.load(Ordering::Acquire) {
        let _ = tick.recv();
        if paused.load(Ordering::Acquire) {
            let _ = drain_packets(&system_rx, |_| {});
            let _ = drain_packets(&microphone_rx, |_| {});
            was_paused = true;
            continue;
        }
        if was_paused {
            mixer.reset(request.aec_mode, auto_should_enable_aec);
            let microphone_is_enabled = microphone_enabled.load(Ordering::Acquire);
            let aec_enabled = microphone_is_enabled
                && should_enable_aec(request.aec_mode, auto_should_enable_aec);
            aec_converging_ms = 0;
            snapshot.lock().aec_status = if aec_enabled {
                AecStatus::Converging
            } else {
                AecStatus::Disabled
            };
            was_paused = false;
        }
        let _ = drain_packets(&system_rx, |packet| mixer.push_system(packet));
        system_connected = system_health.lock().load(Ordering::Acquire);
        let microphone_is_enabled = microphone_enabled.load(Ordering::Acquire);
        let current_microphone_epoch = microphone_source_epoch.load(Ordering::Acquire);
        if current_microphone_epoch != microphone_epoch {
            mixer.switch_microphone_source(
                request.aec_mode,
                auto_should_enable_aec,
                microphone_is_enabled,
                microphone_is_enabled && system_connected,
            );
            microphone_epoch = current_microphone_epoch;
            logging::info(
                "microphone",
                "mixer_source_accepted",
                &[
                    Field::text(FieldKey::SessionId, session_id.clone()),
                    Field::number(FieldKey::SourceEpoch, microphone_epoch),
                ],
            );
            let aec_enabled = microphone_is_enabled
                && should_enable_aec(request.aec_mode, auto_should_enable_aec);
            aec_converging_ms = 0;
            snapshot.lock().aec_status = if aec_enabled {
                AecStatus::Converging
            } else {
                AecStatus::Disabled
            };
        }
        let _ = drain_current_source_packets(&microphone_rx, microphone_epoch, |packet| {
            mixer.push_microphone(packet)
        });
        microphone_connected =
            microphone_is_enabled && microphone_health.lock().load(Ordering::Acquire);
        if !system_connected && !microphone_connected {
            let mut value = snapshot.lock();
            value.state = transition(
                value.state,
                RecordingEvent::SourcesChanged {
                    system: false,
                    microphone: false,
                },
            )
            .unwrap_or(RecordingState::Interrupted);
            value.system.healthy = false;
            value.microphone.healthy = false;
            value.fault = Some(RecordingFault {
                component: "audio".into(),
                code: "ALL_SOURCES_INTERRUPTED".into(),
                recoverable: true,
                user_message: "会议声音和麦克风均已中断，正在等待恢复".into(),
                occurred_at: Utc::now().to_rfc3339(),
            });
            continue;
        }

        let (mixed, system_level, microphone_level) =
            mixer.next_frame(system_connected, microphone_connected);
        if let Err(error) = writer.push_frame(&mixed) {
            failure = Some(error);
            break;
        }
        {
            let mut value = snapshot.lock();
            value.active_duration_ms += 10;
            value.bytes_written = writer.bytes_written();
            value.system.healthy = system_connected;
            value.microphone.healthy = microphone_connected;
            if value.aec_status == AecStatus::Converging && system_connected && microphone_connected
            {
                aec_converging_ms = aec_converging_ms.saturating_add(10);
                if aec_converging_ms >= 2_000 {
                    value.aec_status = AecStatus::Enabled;
                }
            }
        }
        if last_emit.elapsed() >= Duration::from_millis(100) {
            let _ = app.emit(
                "recording://levels",
                LevelEvent {
                    system: system_level,
                    microphone: microphone_level,
                },
            );
            emit_snapshot(&app, &snapshot);
            last_emit = Instant::now();
        }
        if last_disk_check.elapsed() >= Duration::from_secs(1) {
            let output_space = fs2::available_space(output_path.parent().unwrap_or(Path::new(".")));
            let recovery_space =
                fs2::available_space(partial_path.parent().unwrap_or(Path::new(".")));
            match output_space
                .and_then(|output| recovery_space.map(|recovery| output.min(recovery)))
            {
                Ok(space) if space < 50 * 1024 * 1024 => {
                    failure = Some(anyhow!("保存磁盘剩余空间低于 50 MB，录音已安全停止"));
                    break;
                }
                Ok(space) if space < 200 * 1024 * 1024 => {
                    snapshot.lock().fault = Some(RecordingFault {
                        component: "storage".into(),
                        code: "LOW_DISK_WARNING".into(),
                        recoverable: true,
                        user_message: "保存磁盘剩余空间低于 200 MB".into(),
                        occurred_at: Utc::now().to_rfc3339(),
                    });
                }
                Err(error) => {
                    failure = Some(error.into());
                    break;
                }
                _ => {}
            }
            last_disk_check = Instant::now();
        }
    }

    let diagnostics = mixer.diagnostics();
    logging::info(
        "audio_diagnostics",
        "capture_summary",
        &[
            Field::text(FieldKey::SessionId, session_id.clone()),
            Field::text(FieldKey::Scope, "system"),
            Field::number(FieldKey::Underflows, diagnostics.system_underflows),
            Field::number(
                FieldKey::Discontinuities,
                diagnostics.system_discontinuities,
            ),
            Field::number(
                FieldKey::ConcealedSamples,
                diagnostics.system_concealed_gap_samples,
            ),
            Field::number(FieldKey::Packets, diagnostics.system_packets),
            Field::number(
                FieldKey::NonSilentPackets,
                diagnostics.system_non_silent_packets,
            ),
        ],
    );
    logging::info(
        "audio_diagnostics",
        "capture_summary",
        &[
            Field::text(FieldKey::SessionId, session_id.clone()),
            Field::text(FieldKey::Scope, "microphone"),
            Field::number(FieldKey::Underflows, diagnostics.microphone_underflows),
            Field::number(
                FieldKey::Discontinuities,
                diagnostics.microphone_discontinuities,
            ),
            Field::number(
                FieldKey::ConcealedSamples,
                diagnostics.microphone_concealed_gap_samples,
            ),
            Field::number(FieldKey::Packets, diagnostics.microphone_packets),
            Field::number(
                FieldKey::NonSilentPackets,
                diagnostics.microphone_non_silent_packets,
            ),
        ],
    );
    snapshot.lock().state = RecordingState::Finalizing;
    emit_snapshot(&app, &snapshot);
    logging::info(
        "recording",
        "ogg_finalizing",
        &[Field::text(FieldKey::SessionId, session_id.clone())],
    );
    let finalization = writer.finish().and_then(|_| {
        logging::info(
            "recording",
            "ogg_finalized",
            &[Field::text(FieldKey::SessionId, session_id.clone())],
        );
        move_verified(&partial_path, &output_path)?;
        logging::info(
            "recording",
            "output_committed",
            &[Field::text(FieldKey::SessionId, session_id.clone())],
        );
        Ok(())
    });
    match finalization {
        Ok(()) => {
            let metadata = std::fs::metadata(&output_path);
            let duration = snapshot.lock().active_duration_ms;
            let title = output_path
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("会议录音")
                .to_owned();
            let item = RecordingItem {
                id: session_id,
                title,
                path: output_path.to_string_lossy().into_owned(),
                created_at: started_at.to_rfc3339(),
                duration_ms: duration,
                size_bytes: metadata.map(|value| value.len()).unwrap_or(0),
                recovered: false,
                origin: RecordingOrigin::Captured,
                source_file_name: None,
                source_format: None,
                imported_at: None,
                transcription: None,
            };
            if let Err(error) = storage.insert_recording(&item) {
                failure = Some(error);
            }
            let mut value = snapshot.lock();
            value.state = transition(value.state, RecordingEvent::Finalized)
                .unwrap_or(RecordingState::Completed);
            value.output_path = Some(item.path);
            value.bytes_written = item.size_bytes;
            if let Some(error) = failure {
                value.fault = Some(RecordingFault {
                    component: "storage".into(),
                    code: "SAFE_STOP".into(),
                    recoverable: true,
                    user_message: format!("{error:#}"),
                    occurred_at: Utc::now().to_rfc3339(),
                });
            }
            logging::info(
                "recording",
                "completed",
                &[
                    Field::text(FieldKey::SessionId, item.id.clone()),
                    Field::number(FieldKey::Bytes, item.size_bytes),
                    Field::number(FieldKey::DurationMs, item.duration_ms),
                ],
            );
        }
        Err(error) => {
            let mut value = snapshot.lock();
            value.state = RecordingState::Error;
            value.fault = Some(RecordingFault {
                component: "encoder".into(),
                code: "FINALIZE_FAILED".into(),
                recoverable: true,
                user_message: format!(
                    "录音封装失败，恢复文件仍保留在 {}：{error:#}",
                    partial_path.display()
                ),
                occurred_at: Utc::now().to_rfc3339(),
            });
            logging::error(
                "recording",
                "finalization_failed",
                &[
                    Field::text(FieldKey::SessionId, session_id.clone()),
                    Field::text(FieldKey::ErrorCode, "finalize_failed"),
                ],
            );
        }
    }
    emit_snapshot(&app, &snapshot);
}

fn drain_packets(receiver: &Receiver<AudioPacket>, mut consume: impl FnMut(AudioPacket)) -> bool {
    for _ in 0..MAX_PACKETS_PER_TICK {
        match receiver.try_recv() {
            Ok(packet) => consume(packet),
            Err(TryRecvError::Empty) => return true,
            Err(TryRecvError::Disconnected) => return false,
        }
    }
    true
}

fn drain_current_source_packets(
    receiver: &Receiver<AudioPacket>,
    source_epoch: u64,
    mut consume: impl FnMut(AudioPacket),
) -> bool {
    drain_packets(receiver, |packet| {
        if packet.source_epoch == source_epoch {
            consume(packet);
        }
    })
}

fn next_source_epoch(current: u64) -> u64 {
    current.checked_add(1).unwrap_or(1)
}

fn unique_recording_path(directory: &Path) -> PathBuf {
    let stem = Local::now().format("%Y-%m-%d_%H-%M_会议录音").to_string();
    let first = directory.join(format!("{stem}.ogg"));
    if recording_destination_is_available(&first) {
        return first;
    }
    for suffix in 2..10_000 {
        let candidate = directory.join(format!("{stem}_{suffix}.ogg"));
        if recording_destination_is_available(&candidate) {
            return candidate;
        }
    }
    directory.join(format!("{stem}_{}.ogg", Uuid::new_v4()))
}

fn recording_destination_is_available(destination: &Path) -> bool {
    !destination.exists() && !destination.with_extension("ogg.copying").exists()
}

fn capture_source_from_selection(selection: &CaptureSelection) -> Result<CaptureSource> {
    match selection {
        CaptureSelection::Process { target_id } => {
            let target = enumerate_capture_targets()?
                .into_iter()
                .find(|target| target.id == *target_id)
                .context("所选应用已退出，请刷新应用列表后重试")?;
            Ok(CaptureSource::Process {
                process_id: target.process_id,
                display_name: target.display_name,
                executable_path: target.executable_path,
            })
        }
        CaptureSelection::System { device } => Ok(CaptureSource::System(device.clone())),
    }
}

fn auto_should_enable_aec(selection: &CaptureSelection) -> bool {
    let Ok(devices) = enumerate_audio_devices() else {
        return true;
    };
    let endpoint_id = match selection {
        CaptureSelection::System {
            device: DeviceSelection::Fixed { endpoint_id },
        } => Some(endpoint_id.as_str()),
        _ => None,
    };
    let device = devices.into_iter().find(|device| {
        device.direction == DeviceDirection::Render
            && endpoint_id
                .map(|id| device.id == id)
                .unwrap_or(device.is_default_communications)
    });
    !device.is_some_and(|device| {
        matches!(
            device.form_factor.as_str(),
            "Headphones" | "Headset" | "Handset"
        )
    })
}

fn should_enable_aec(mode: AecMode, auto_should_enable: bool) -> bool {
    matches!(mode, AecMode::On) || (matches!(mode, AecMode::Auto) && auto_should_enable)
}

fn emit_snapshot(app: &AppHandle, snapshot: &Arc<Mutex<RecordingSnapshot>>) {
    let value = snapshot.lock().clone();
    let _ = app.emit("recording://snapshot", value.clone());
    schedule_tray_update(app, value.state);
}

fn recording_session_fields(snapshot: &Arc<Mutex<RecordingSnapshot>>) -> Vec<Field<'static>> {
    snapshot
        .lock()
        .session_id
        .clone()
        .map(|session_id| vec![Field::text(FieldKey::SessionId, session_id)])
        .unwrap_or_default()
}

fn command_result<T>(result: Result<T>) -> std::result::Result<T, String> {
    result.map_err(|error| format!("{error:#}"))
}

fn stop_in_background(app: &AppHandle, recorder: Arc<RecordingController>) {
    let stop_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(error) = recorder.stop(&stop_app) {
            logging::error(
                "recording",
                "background_stop_failed",
                &[Field::text(FieldKey::ErrorCode, "stop_failed")],
            );
            recorder.set_error(&stop_app, "controller", "STOP_FAILED", &error);
        }
    });
}

#[tauri::command]
async fn list_capture_targets() -> std::result::Result<Vec<CaptureTarget>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let mut targets = command_result(enumerate_capture_targets())?;
        attach_capture_target_icons(&mut targets);
        Ok(targets)
    })
    .await
    .map_err(|error| format!("无法刷新应用列表：{error}"))?
}

#[tauri::command]
fn list_audio_devices() -> std::result::Result<Vec<AudioDevice>, String> {
    command_result(enumerate_audio_devices())
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> std::result::Result<AppSettings, String> {
    command_result(state.storage.settings())
}

fn shortcut_settings_changed(previous: &AppSettings, next: &AppSettings) -> bool {
    previous.shortcuts_enabled != next.shortcuts_enabled
        || previous.toggle_shortcut != next.toggle_shortcut
        || previous.stop_shortcut != next.stop_shortcut
}

#[tauri::command]
fn save_settings(
    app: AppHandle,
    state: State<AppState>,
    settings: AppSettings,
) -> std::result::Result<(), String> {
    command_result((|| {
        let previous = state.storage.settings()?;
        let shortcuts_changed = shortcut_settings_changed(&previous, &settings);
        if shortcuts_changed && let Err(error) = register_shortcuts(&app, &settings) {
            let _ = register_shortcuts(&app, &previous);
            bail!("快捷键注册失败，可能与其他应用冲突：{error}");
        }
        if let Err(error) = state.storage.save_settings(&settings) {
            if shortcuts_changed {
                let _ = register_shortcuts(&app, &previous);
            }
            return Err(error);
        }
        logging::info("settings", "saved", &[]);
        Ok(())
    })())
}

#[tauri::command]
fn list_hotword_lists(
    state: State<AppState>,
) -> std::result::Result<Vec<HotwordListSummary>, String> {
    command_result(state.storage.list_hotword_lists())
}

#[tauri::command]
fn get_hotword_list(
    state: State<AppState>,
    id: String,
) -> std::result::Result<HotwordListDocument, String> {
    command_result(state.storage.get_hotword_list(&id))
}

#[tauri::command]
fn save_hotword_list(
    state: State<AppState>,
    request: SaveHotwordListRequest,
) -> std::result::Result<SaveHotwordListResult, String> {
    command_result(state.storage.save_hotword_list(&request))
}

#[tauri::command]
fn delete_hotword_list(state: State<AppState>, id: String) -> std::result::Result<(), String> {
    command_result(state.storage.delete_hotword_list(&id))
}

#[tauri::command]
fn get_recording_snapshot(state: State<AppState>) -> RecordingSnapshot {
    state.recorder.snapshot()
}

#[tauri::command]
fn get_capture_prompt(state: State<AppState>) -> Option<CapturePrompt> {
    state.recorder.pending_capture_prompt()
}

#[tauri::command]
async fn respond_capture_prompt(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    stop_and_save: bool,
) -> std::result::Result<RecordingSnapshot, String> {
    let recorder = Arc::clone(&state.recorder);
    let Some(prompt) = recorder
        .pending_capture_prompt()
        .filter(|prompt| prompt.session_id == session_id)
    else {
        return Ok(recorder.snapshot());
    };
    logging::info(
        "user_action",
        "capture_prompt_responded",
        &[
            Field::text(FieldKey::SessionId, session_id.clone()),
            Field::text(
                FieldKey::Reason,
                match prompt.kind {
                    CapturePromptKind::CaptureInterrupted => "capture_interrupted",
                    CapturePromptKind::ProlongedSilence => "prolonged_silence",
                },
            ),
            Field::text(
                FieldKey::Action,
                if stop_and_save {
                    "stop_and_save"
                } else {
                    "continue_recording"
                },
            ),
        ],
    );
    recorder.dismiss_capture_prompt(&app);
    if !stop_and_save {
        return Ok(recorder.snapshot());
    }
    let result = tauri::async_runtime::spawn_blocking(move || recorder.stop(&app))
        .await
        .map_err(|error| format!("停止录音任务异常结束：{error}"))?;
    command_result(result)
}

#[tauri::command]
fn resize_capture_prompt(app: AppHandle, height: f64) -> std::result::Result<(), String> {
    let height = normalize_capture_prompt_height(height).map_err(|error| error.to_string())?;
    let main_thread_app = app.clone();
    app.run_on_main_thread(move || {
        let Some(window) = main_thread_app.get_webview_window(CAPTURE_PROMPT_WINDOW) else {
            return;
        };
        if window
            .set_size(LogicalSize::new(CAPTURE_PROMPT_WIDTH, height))
            .is_err()
        {
            logging::warn(
                "capture_prompt",
                "resize_failed",
                &[Field::text(FieldKey::ErrorCode, "window_resize_failed")],
            );
            return;
        }
        position_capture_prompt_window_on_main_thread(&main_thread_app);
    })
    .map_err(|error| format!("无法调整会议状态提醒窗口：{error}"))
}

fn normalize_capture_prompt_height(height: f64) -> Result<f64> {
    if !height.is_finite() {
        bail!("会议状态提醒窗口高度无效");
    }
    Ok(height.clamp(CAPTURE_PROMPT_MIN_HEIGHT, CAPTURE_PROMPT_MAX_HEIGHT))
}

#[tauri::command]
fn start_recording(
    app: AppHandle,
    state: State<AppState>,
    request: StartRecordingRequest,
) -> std::result::Result<RecordingSnapshot, String> {
    let _gate = state.media_start_gate.lock();
    if state.importer.has_active() {
        return Err("音频正在导入，请等待完成或停止导入后再开始录音".into());
    }
    command_result(state.recorder.start(app, request))
}

#[tauri::command]
fn pause_recording(
    app: AppHandle,
    state: State<AppState>,
) -> std::result::Result<RecordingSnapshot, String> {
    command_result(state.recorder.pause(&app))
}

#[tauri::command]
fn resume_recording(
    app: AppHandle,
    state: State<AppState>,
) -> std::result::Result<RecordingSnapshot, String> {
    command_result(state.recorder.resume(&app))
}

#[tauri::command]
async fn stop_recording(
    app: AppHandle,
    state: State<'_, AppState>,
) -> std::result::Result<RecordingSnapshot, String> {
    let recorder = Arc::clone(&state.recorder);
    let result = tauri::async_runtime::spawn_blocking(move || recorder.stop(&app))
        .await
        .map_err(|error| format!("停止录音任务异常结束：{error}"))?;
    command_result(result)
}

#[tauri::command]
fn switch_capture_source(
    app: AppHandle,
    state: State<AppState>,
    capture: CaptureSelection,
) -> std::result::Result<RecordingSnapshot, String> {
    command_result(state.recorder.switch_capture(&app, capture))
}

#[tauri::command]
fn set_microphone_enabled(
    app: AppHandle,
    state: State<AppState>,
    microphone: Option<DeviceSelection>,
) -> std::result::Result<RecordingSnapshot, String> {
    command_result(state.recorder.set_microphone(&app, microphone))
}

impl AppState {
    fn runtime_active(&self) -> bool {
        self.recorder.is_active()
            || self.asr.has_active()
            || self.ai.has_active()
            || self.importer.has_active()
    }
}

#[tauri::command]
fn start_audio_import(
    app: AppHandle,
    state: State<AppState>,
    paths: Vec<String>,
) -> std::result::Result<AudioImportBatchSnapshot, String> {
    let _gate = state.media_start_gate.lock();
    command_result(state.importer.start(app, paths))
}

#[tauri::command]
fn get_audio_import_snapshot(state: State<AppState>) -> Option<AudioImportBatchSnapshot> {
    state.importer.snapshot()
}

#[tauri::command]
fn cancel_audio_import(
    state: State<AppState>,
) -> std::result::Result<AudioImportBatchSnapshot, String> {
    command_result(state.importer.cancel())
}

#[tauri::command]
fn list_recordings(state: State<AppState>) -> std::result::Result<Vec<RecordingItem>, String> {
    command_result(state.storage.list_recordings())
}

#[tauri::command]
fn prepare_recording_playback(
    app: AppHandle,
    state: State<AppState>,
    id: String,
) -> std::result::Result<String, String> {
    command_result((|| {
        let item = state.storage.find_recording(&id)?;
        let path = PathBuf::from(&item.path);
        if !path.is_file() {
            bail!("录音文件不存在或已被移动");
        }
        let canonical = std::fs::canonicalize(&path)
            .with_context(|| format!("无法访问录音文件 {}", path.display()))?;
        app.asset_protocol_scope().allow_file(&canonical)?;
        Ok(canonical.to_string_lossy().into_owned())
    })())
}

#[tauri::command]
fn list_recoverable_recordings(
    state: State<AppState>,
) -> std::result::Result<Vec<RecordingItem>, String> {
    command_result((|| {
        let files = list_recoverable_files(&state.storage.paths().recovery)?;
        Ok(files
            .into_iter()
            .map(|file| RecordingItem {
                id: file.id,
                title: format!(
                    "未完成录音 {}",
                    file.created_at.with_timezone(&Local).format("%m-%d %H:%M")
                ),
                path: file.path.to_string_lossy().into_owned(),
                created_at: file.created_at.to_rfc3339(),
                duration_ms: 0,
                size_bytes: file.size_bytes,
                recovered: true,
                origin: RecordingOrigin::Captured,
                source_file_name: None,
                source_format: None,
                imported_at: None,
                transcription: None,
            })
            .collect())
    })())
}

#[tauri::command]
fn recover_recording(
    app: AppHandle,
    state: State<AppState>,
    id: String,
) -> std::result::Result<RecordingItem, String> {
    if state.recorder.is_active() {
        return Err("录音进行中不能同时恢复旧文件".into());
    }
    {
        let mut snapshot = state.recorder.snapshot.lock();
        snapshot.state = command_result(transition(snapshot.state, RecordingEvent::Recover))?;
        snapshot.fault = None;
    }
    emit_snapshot(&app, &state.recorder.snapshot);
    let result = (|| {
        let file = list_recoverable_files(&state.storage.paths().recovery)?
            .into_iter()
            .find(|file| file.id == id)
            .context("找不到该恢复文件")?;
        let destination =
            unique_recording_path(&PathBuf::from(state.storage.settings()?.output_directory));
        let size = recover_ogg_file(&file.path, &destination)?;
        let item = RecordingItem {
            id: Uuid::new_v4().to_string(),
            title: destination
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("已恢复录音")
                .into(),
            path: destination.to_string_lossy().into_owned(),
            created_at: Utc::now().to_rfc3339(),
            duration_ms: 0,
            size_bytes: size,
            recovered: true,
            origin: RecordingOrigin::Captured,
            source_file_name: None,
            source_format: None,
            imported_at: None,
            transcription: None,
        };
        state.storage.insert_recording(&item)?;
        Ok(item)
    })();
    match result {
        Ok(item) => {
            {
                let mut snapshot = state.recorder.snapshot.lock();
                snapshot.state = transition(snapshot.state, RecordingEvent::Finalized)
                    .unwrap_or(RecordingState::Completed);
                snapshot.output_path = Some(item.path.clone());
            }
            emit_snapshot(&app, &state.recorder.snapshot);
            Ok(item)
        }
        Err(error) => {
            state
                .recorder
                .set_error(&app, "recovery", "RECOVERY_FAILED", &error);
            command_result(Err(error))
        }
    }
}

#[tauri::command]
fn rename_recording(
    state: State<AppState>,
    id: String,
    title: String,
) -> std::result::Result<RecordingItem, String> {
    command_result(state.storage.rename_recording(&id, &title))
}

#[tauri::command]
fn reveal_recording(state: State<AppState>, id: String) -> std::result::Result<(), String> {
    command_result((|| {
        let item = state.storage.find_recording(&id)?;
        std::process::Command::new("explorer.exe")
            .arg(format!("/select,{}", item.path))
            .spawn()?;
        Ok(())
    })())
}

#[tauri::command]
fn delete_recording(
    state: State<AppState>,
    id: String,
    permanent: bool,
    delete_ai_documents: Option<bool>,
) -> std::result::Result<(), String> {
    command_result((|| {
        if state.asr.is_active(&id) {
            bail!("该录音正在转写，请先中断任务后再删除");
        }
        if state.storage.has_active_ai_generation(&id)? {
            bail!("该录音仍有 AI 文档正在生成，请先取消任务");
        }
        let item = state.storage.find_recording(&id)?;
        remove_temporary_chunks(&state.storage.paths().recovery, &id)?;
        if delete_ai_documents.unwrap_or(false) {
            for (path, document_id, version_id) in state.storage.ai_document_file_links(&id)? {
                let path = PathBuf::from(path);
                if !path.is_file()
                    || !document_file_matches_identity(&path, &document_id, &version_id)
                {
                    continue;
                }
                if permanent {
                    std::fs::remove_file(&path)?;
                } else {
                    trash::delete(&path)?;
                }
            }
        }
        if permanent {
            std::fs::remove_file(&item.path)?;
        } else {
            trash::delete(&item.path)?;
        }
        state.storage.remove_recording(&id)?;
        logging::info(
            "recording_library",
            "recording_deleted",
            &[
                Field::text(FieldKey::RecordingId, id.as_str()),
                Field::boolean(FieldKey::Permanent, permanent),
                Field::boolean(
                    FieldKey::IncludeAiDocuments,
                    delete_ai_documents.unwrap_or(false),
                ),
            ],
        );
        Ok(())
    })())
}

#[tauri::command]
fn open_microphone_settings() -> std::result::Result<(), String> {
    command_result((|| {
        let operation = wide("open");
        let target = wide("ms-settings:privacy-microphone");
        let result = unsafe {
            ShellExecuteW(
                Some(HWND::default()),
                PCWSTR(operation.as_ptr()),
                PCWSTR(target.as_ptr()),
                None,
                None,
                SW_SHOWNORMAL,
            )
        };
        if result.0 as isize <= 32 {
            bail!("无法打开 Windows 麦克风隐私设置");
        }
        Ok(())
    })())
}

#[tauri::command]
fn open_log_directory(state: State<AppState>) -> std::result::Result<(), String> {
    command_result((|| {
        logging::info("diagnostics", "log_directory_open_requested", &[]);
        let operation = wide("open");
        let target = wide(state.storage.paths().logs.to_string_lossy().as_ref());
        let result = unsafe {
            ShellExecuteW(
                Some(HWND::default()),
                PCWSTR(operation.as_ptr()),
                PCWSTR(target.as_ptr()),
                None,
                None,
                SW_SHOWNORMAL,
            )
        };
        if result.0 as isize <= 32 {
            bail!("无法在资源管理器中打开日志目录");
        }
        Ok(())
    })())
}

#[tauri::command]
fn delete_recoverable_recording(
    state: State<AppState>,
    id: String,
) -> std::result::Result<(), String> {
    command_result((|| {
        let file = list_recoverable_files(&state.storage.paths().recovery)?
            .into_iter()
            .find(|file| file.id == id)
            .context("找不到该恢复文件")?;
        std::fs::remove_file(file.path)?;
        Ok(())
    })())
}

#[tauri::command]
async fn quit_application(
    app: AppHandle,
    state: State<'_, AppState>,
    stop_and_save: bool,
) -> std::result::Result<(), String> {
    let recorder = Arc::clone(&state.recorder);
    if recorder.is_active() {
        if !stop_and_save {
            return Err("录音仍在进行".into());
        }
        let stop_app = app.clone();
        let result =
            tauri::async_runtime::spawn_blocking(move || recorder.stop_and_wait(&stop_app))
                .await
                .map_err(|error| format!("退出前保存任务异常结束：{error}"))?;
        command_result(result)?;
    }
    if state.asr.has_active() {
        if !stop_and_save {
            return Err("仍有语音转写任务正在进行".into());
        }
        command_result(state.asr.interrupt_all())?;
    }
    if state.ai.has_active() {
        if !stop_and_save {
            return Err("仍有 AI 文档正在生成".into());
        }
        command_result(state.ai.interrupt_all())?;
    }
    if state.importer.has_active() {
        if !stop_and_save {
            return Err("仍有音频正在导入".into());
        }
        command_result(state.importer.interrupt_all())?;
    }
    app.exit(0);
    Ok(())
}

#[tauri::command]
fn list_asr_providers(state: State<AppState>) -> std::result::Result<Vec<AsrProvider>, String> {
    command_result(state.storage.list_asr_providers())
}

#[tauri::command]
fn save_asr_provider(
    state: State<AppState>,
    mut request: SaveAsrProviderRequest,
) -> std::result::Result<AsrProvider, String> {
    command_result((|| {
        if request.name.trim().is_empty() {
            bail!("服务名称不能为空");
        }
        if request.model_id.trim().is_empty() {
            bail!("模型 ID 不能为空");
        }
        request.name = request.name.trim().to_owned();
        request.model_id = request.model_id.trim().to_owned();
        request.base_url = normalize_asr_provider_base_url(request.kind, &request.base_url)?;
        if request.kind == AsrProviderKind::DashScope
            && request.model_id != "qwen-audio-3.0-asr-flash-filetrans"
        {
            bail!("DashScope 第一版仅支持 qwen-audio-3.0-asr-flash-filetrans 模型");
        }
        let provider = state.storage.save_asr_provider(request)?;
        logging::info(
            "settings",
            "asr_provider_saved",
            &[
                Field::text(FieldKey::ProviderId, provider.id.as_str()),
                Field::text(FieldKey::ProviderKind, provider.kind.as_str()),
                Field::text(FieldKey::ModelId, provider.model_id.as_str()),
            ],
        );
        Ok(provider)
    })())
}

#[tauri::command]
fn delete_asr_provider(state: State<AppState>, id: String) -> std::result::Result<(), String> {
    command_result((|| {
        state.storage.delete_asr_provider(&id)?;
        logging::info(
            "settings",
            "asr_provider_deleted",
            &[Field::text(FieldKey::ProviderId, id)],
        );
        Ok(())
    })())
}

#[tauri::command]
fn set_active_asr_provider(
    state: State<AppState>,
    id: Option<String>,
) -> std::result::Result<AppSettings, String> {
    command_result((|| {
        if let Some(provider_id) = id.as_deref() {
            state.storage.find_asr_provider(provider_id)?;
        }
        let mut settings = state.storage.settings()?;
        settings.active_asr_provider_id = id.filter(|value| !value.trim().is_empty());
        if settings.active_asr_provider_id.is_none() {
            settings.auto_transcribe = false;
        }
        state.storage.save_settings(&settings)?;
        Ok(settings)
    })())
}

#[tauri::command]
async fn test_asr_provider(
    state: State<'_, AppState>,
    mut request: AsrProviderProbeRequest,
) -> std::result::Result<AsrConnectionTest, String> {
    let storage = Arc::clone(&state.storage);
    let result = tauri::async_runtime::spawn_blocking(move || {
        request.base_url = normalize_asr_provider_base_url(request.kind, &request.base_url)?;
        let credentials = storage.asr_probe_credentials(request)?;
        test_connection(&credentials)
    })
    .await
    .map_err(|error| format!("连接测试任务异常结束：{error}"))?;
    command_result(result)
}

#[tauri::command]
async fn list_asr_models(
    state: State<'_, AppState>,
    mut request: AsrProviderProbeRequest,
) -> std::result::Result<Vec<AsrModel>, String> {
    let storage = Arc::clone(&state.storage);
    let result = tauri::async_runtime::spawn_blocking(move || {
        request.base_url = normalize_asr_provider_base_url(request.kind, &request.base_url)?;
        let credentials = storage.asr_probe_credentials(request)?;
        list_provider_models(&credentials)
    })
    .await
    .map_err(|error| format!("读取模型任务异常结束：{error}"))?;
    command_result(result)
}

#[tauri::command]
async fn get_transcription_options(
    state: State<'_, AppState>,
    provider_id: String,
) -> std::result::Result<TranscriptionOptions, String> {
    let storage = Arc::clone(&state.storage);
    let result = tauri::async_runtime::spawn_blocking(move || {
        let credentials = storage.find_asr_provider(&provider_id)?;
        resolve_transcription_options(&credentials)
    })
    .await
    .map_err(|error| format!("读取转写设置异常结束：{error}"))?;
    command_result(result)
}

#[tauri::command]
fn list_llm_providers(state: State<AppState>) -> std::result::Result<Vec<LlmProvider>, String> {
    command_result(state.storage.list_llm_providers())
}

#[tauri::command]
fn save_llm_provider(
    state: State<AppState>,
    mut request: SaveLlmProviderRequest,
) -> std::result::Result<LlmProvider, String> {
    command_result((|| {
        request.base_url = normalize_provider_base_url(request.kind, &request.base_url)?;
        let provider = state.storage.save_llm_provider(request)?;
        logging::info(
            "settings",
            "llm_provider_saved",
            &[
                Field::text(FieldKey::ProviderId, provider.id.as_str()),
                Field::text(FieldKey::ProviderKind, provider.kind.as_str()),
                Field::text(FieldKey::ModelId, provider.model_id.as_str()),
            ],
        );
        Ok(provider)
    })())
}

#[tauri::command]
fn delete_llm_provider(state: State<AppState>, id: String) -> std::result::Result<(), String> {
    command_result((|| {
        state.storage.delete_llm_provider(&id)?;
        logging::info(
            "settings",
            "llm_provider_deleted",
            &[Field::text(FieldKey::ProviderId, id)],
        );
        Ok(())
    })())
}

#[tauri::command]
fn set_active_llm_provider(
    state: State<AppState>,
    id: Option<String>,
) -> std::result::Result<AppSettings, String> {
    command_result((|| {
        if let Some(provider_id) = id.as_deref() {
            state.storage.find_llm_provider(provider_id)?;
        }
        let mut settings = state.storage.settings()?;
        settings.active_llm_provider_id = id.filter(|value| !value.trim().is_empty());
        state.storage.save_settings(&settings)?;
        Ok(settings)
    })())
}

#[tauri::command]
async fn test_llm_provider(
    state: State<'_, AppState>,
    mut request: LlmProviderProbeRequest,
) -> std::result::Result<LlmConnectionTest, String> {
    let storage = Arc::clone(&state.storage);
    let result = tauri::async_runtime::spawn_blocking(move || {
        request.base_url = normalize_provider_base_url(request.kind, &request.base_url)?;
        let credentials = storage.llm_probe_credentials(request)?;
        test_llm_connection(&credentials)
    })
    .await
    .map_err(|error| format!("AI 连接测试任务异常结束：{error}"))?;
    command_result(result)
}

#[tauri::command]
async fn list_llm_models(
    state: State<'_, AppState>,
    mut request: LlmProviderProbeRequest,
) -> std::result::Result<Vec<LlmModel>, String> {
    let storage = Arc::clone(&state.storage);
    let result = tauri::async_runtime::spawn_blocking(move || {
        request.base_url = normalize_provider_base_url(request.kind, &request.base_url)?;
        let credentials = storage.llm_probe_credentials(request)?;
        list_llm_provider_models(&credentials)
    })
    .await
    .map_err(|error| format!("读取 AI 模型任务异常结束：{error}"))?;
    command_result(result)
}

#[tauri::command]
fn list_ai_templates(state: State<AppState>) -> std::result::Result<Vec<AiTemplate>, String> {
    command_result(state.storage.list_ai_templates())
}

#[tauri::command]
fn save_ai_template(
    state: State<AppState>,
    request: SaveAiTemplateRequest,
) -> std::result::Result<AiTemplate, String> {
    command_result(state.storage.save_ai_template(request))
}

#[tauri::command]
fn clone_ai_template(
    state: State<AppState>,
    id: String,
    name: String,
) -> std::result::Result<AiTemplate, String> {
    command_result(state.storage.clone_ai_template(&id, &name))
}

#[tauri::command]
fn archive_ai_template(state: State<AppState>, id: String) -> std::result::Result<(), String> {
    command_result(state.storage.archive_ai_template(&id))
}

#[tauri::command]
fn get_ai_workspace(
    state: State<AppState>,
    recording_id: String,
) -> std::result::Result<AiWorkspace, String> {
    command_result(state.storage.ai_workspace(&recording_id))
}

#[tauri::command]
fn preview_ai_generation_request(
    state: State<AppState>,
    request: AiGenerationRequest,
) -> std::result::Result<AiGenerationRequestPreview, String> {
    command_result(preview_generation_request(&state.storage, &request))
}

#[tauri::command]
fn copy_ai_request_body(request_body: String) -> std::result::Result<(), String> {
    command_result((|| {
        if request_body.trim().is_empty() {
            bail!("当前没有可复制的 AI Request Body");
        }
        arboard::Clipboard::new()?.set_text(request_body)?;
        Ok(())
    })())
}

#[tauri::command]
fn copy_ai_generation_json(json: String) -> std::result::Result<(), String> {
    command_result((|| {
        if json.trim().is_empty() {
            bail!("当前没有可复制的生成详情 JSON");
        }
        arboard::Clipboard::new()?.set_text(json)?;
        Ok(())
    })())
}

#[tauri::command]
fn list_ai_document_versions(
    state: State<AppState>,
    document_id: String,
) -> std::result::Result<Vec<AiDocumentVersion>, String> {
    command_result(state.storage.list_ai_document_versions(&document_id))
}

#[tauri::command]
fn generate_ai_document(
    app: AppHandle,
    state: State<AppState>,
    request: AiGenerationRequest,
) -> std::result::Result<AiDocumentVersion, String> {
    command_result(state.ai.start(app, request))
}

#[tauri::command]
fn cancel_ai_generation(
    state: State<AppState>,
    version_id: String,
) -> std::result::Result<AiDocumentVersion, String> {
    command_result(state.ai.cancel(&version_id))
}

#[tauri::command]
fn read_ai_document_version(
    state: State<AppState>,
    version_id: String,
) -> std::result::Result<AiDocumentContent, String> {
    command_result(read_document_content(&state.storage, &version_id))
}

#[tauri::command]
fn read_ai_generation_details(
    state: State<AppState>,
    version_id: String,
) -> std::result::Result<AiGenerationDetails, String> {
    command_result(state.storage.find_ai_generation_details(&version_id))
}

#[tauri::command]
fn relink_ai_document_version(
    state: State<AppState>,
    version_id: String,
    path: String,
) -> std::result::Result<AiDocumentVersion, String> {
    command_result(relink_document_version(&state.storage, &version_id, &path))
}

#[tauri::command]
fn find_ai_document_version(
    state: State<AppState>,
    version_id: String,
    workspace_path: String,
) -> std::result::Result<AiDocumentVersion, String> {
    command_result(find_moved_document_version(
        &state.storage,
        &version_id,
        &workspace_path,
    ))
}

fn ai_version_path(storage: &Storage, version_id: &str) -> Result<PathBuf> {
    let version = storage.find_ai_document_version(version_id)?;
    let path = PathBuf::from(version.file_path.context("该版本没有文件路径")?);
    if !path.is_file() {
        bail!("AI Markdown 文件不存在或已被移动");
    }
    Ok(path)
}

fn reveal_path_in_explorer(path: &Path) -> Result<()> {
    let target = wide(path.to_string_lossy().as_ref());
    let pidl = unsafe { ILCreateFromPathW(PCWSTR(target.as_ptr())) };
    if pidl.is_null() {
        bail!("无法解析要在资源管理器中显示的文件路径");
    }
    let result = unsafe { SHOpenFolderAndSelectItems(pidl, None, 0) };
    unsafe { ILFree(Some(pidl)) };
    result.context("无法在资源管理器中显示文件")
}

#[tauri::command]
fn open_ai_document_version(
    state: State<AppState>,
    version_id: String,
) -> std::result::Result<(), String> {
    command_result((|| {
        let path = ai_version_path(&state.storage, &version_id)?;
        let operation = wide("open");
        let target = wide(path.to_string_lossy().as_ref());
        let result = unsafe {
            ShellExecuteW(
                Some(HWND::default()),
                PCWSTR(operation.as_ptr()),
                PCWSTR(target.as_ptr()),
                None,
                None,
                SW_SHOWNORMAL,
            )
        };
        if result.0 as isize <= 32 {
            bail!("无法使用默认应用打开 Markdown 文件");
        }
        Ok(())
    })())
}

#[tauri::command]
fn reveal_ai_document_version(
    state: State<AppState>,
    version_id: String,
) -> std::result::Result<(), String> {
    command_result((|| {
        let path = ai_version_path(&state.storage, &version_id)?;
        reveal_path_in_explorer(&path)
    })())
}

#[tauri::command]
fn copy_ai_document_version(
    state: State<AppState>,
    version_id: String,
) -> std::result::Result<(), String> {
    command_result((|| {
        let content = read_document_content(&state.storage, &version_id)?;
        arboard::Clipboard::new()?.set_text(content.markdown)?;
        Ok(())
    })())
}

#[tauri::command]
fn copy_ai_document_path(
    state: State<AppState>,
    version_id: String,
) -> std::result::Result<(), String> {
    command_result((|| {
        let path = ai_version_path(&state.storage, &version_id)?;
        arboard::Clipboard::new()?.set_text(path.to_string_lossy().into_owned())?;
        Ok(())
    })())
}

#[tauri::command]
async fn export_ai_document_pdf(
    webview: Webview,
    state: State<'_, AppState>,
    version_id: String,
    path: String,
) -> std::result::Result<(), String> {
    command_result(read_document_content(&state.storage, &version_id).map(|_| ()))?;
    command_result(export_current_webview_pdf(webview, &path).await)
}

#[tauri::command]
fn reveal_ai_document_pdf(path: String) -> std::result::Result<(), String> {
    command_result((|| {
        let path = validate_exported_pdf(&path)?;
        reveal_path_in_explorer(&path)
    })())
}

#[tauri::command]
fn start_transcription(
    app: AppHandle,
    state: State<AppState>,
    recording_id: String,
    provider_id: Option<String>,
    speaker_count: Option<u32>,
    hotword_list_id: Option<String>,
) -> std::result::Result<TranscriptionSummary, String> {
    command_result((|| {
        let provider_id = match provider_id.filter(|value| !value.trim().is_empty()) {
            Some(value) => value,
            None => state
                .storage
                .settings()?
                .active_asr_provider_id
                .context("请先在设置中选择默认语音转写服务")?,
        };
        state.asr.start(
            app,
            &recording_id,
            &provider_id,
            speaker_count,
            hotword_list_id.as_deref(),
        )
    })())
}

#[tauri::command]
fn cancel_transcription(
    app: AppHandle,
    state: State<AppState>,
    recording_id: String,
) -> std::result::Result<TranscriptionSummary, String> {
    command_result(state.asr.cancel(&app, &recording_id))
}

#[tauri::command]
fn resume_transcription(
    app: AppHandle,
    state: State<AppState>,
    recording_id: String,
) -> std::result::Result<TranscriptionSummary, String> {
    command_result(state.asr.resume(app, &recording_id))
}

#[tauri::command]
fn get_transcript(
    state: State<AppState>,
    recording_id: String,
) -> std::result::Result<TranscriptDocument, String> {
    command_result(state.storage.transcript(&recording_id))
}

#[tauri::command]
fn list_transcription_versions(
    state: State<AppState>,
    recording_id: String,
) -> std::result::Result<Vec<TranscriptionVersionSummary>, String> {
    command_result(state.storage.list_transcription_versions(&recording_id))
}

#[tauri::command]
fn select_transcription_version(
    state: State<AppState>,
    recording_id: String,
    generation: u32,
) -> std::result::Result<TranscriptDocument, String> {
    command_result(
        state
            .storage
            .select_transcription_generation(&recording_id, generation),
    )
}

#[tauri::command]
fn copy_transcript(
    state: State<AppState>,
    recording_id: String,
) -> std::result::Result<(), String> {
    command_result((|| {
        let transcript = state.storage.transcript(&recording_id)?;
        let text = format_transcript_text(&transcript);
        if text.is_empty() {
            bail!("当前没有可复制的转写文字");
        }
        arboard::Clipboard::new()?.set_text(text)?;
        logging::info(
            "transcript",
            "copied",
            &[Field::text(FieldKey::RecordingId, recording_id.as_str())],
        );
        Ok(())
    })())
}

#[tauri::command]
fn export_transcript(
    state: State<AppState>,
    recording_id: String,
    path: String,
) -> std::result::Result<(), String> {
    command_result((|| {
        let transcript = state.storage.transcript(&recording_id)?;
        let text = format_transcript_text(&transcript);
        if text.is_empty() {
            bail!("当前没有可导出的转写文字");
        }
        let destination = PathBuf::from(path);
        if !destination
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("txt"))
        {
            bail!("转写结果仅支持导出为 .txt 文件");
        }
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(destination, text.as_bytes())?;
        logging::info(
            "transcript",
            "exported",
            &[Field::text(FieldKey::RecordingId, recording_id.as_str())],
        );
        Ok(())
    })())
}

fn format_transcript_text(transcript: &TranscriptDocument) -> String {
    let has_speaker = transcript.segments.iter().any(|segment| {
        segment
            .speaker
            .as_deref()
            .is_some_and(|speaker| !speaker.trim().is_empty())
    });
    if !has_speaker {
        return transcript.text.trim().to_owned();
    }

    let lines = transcript
        .segments
        .iter()
        .filter_map(|segment| {
            let text = segment.text.trim();
            if text.is_empty() {
                return None;
            }
            let speaker = segment
                .speaker
                .as_deref()
                .map(str::trim)
                .filter(|speaker| !speaker.is_empty());
            Some(match speaker {
                Some(speaker) => {
                    let display = transcript
                        .speaker_names
                        .get(speaker)
                        .map(String::as_str)
                        .unwrap_or(speaker);
                    format!("{display}：{text}")
                }
                None => text.to_owned(),
            })
        })
        .collect::<Vec<_>>();
    if lines.is_empty() {
        transcript.text.trim().to_owned()
    } else {
        lines.join("\r\n")
    }
}

#[tauri::command]
async fn identify_recording_speakers(
    state: State<'_, AppState>,
    recording_id: String,
    provider_id: Option<String>,
) -> std::result::Result<SpeakerIdentificationSession, String> {
    let manager = Arc::clone(&state.voiceprints);
    let result = tauri::async_runtime::spawn_blocking(move || {
        manager.identify(&recording_id, provider_id.as_deref())
    })
    .await
    .map_err(|error| format!("说话人识别任务异常结束：{error}"))?;
    command_result(result)
}

#[tauri::command]
fn save_speaker_identification(
    state: State<AppState>,
    session_id: String,
    assignments: Vec<SpeakerIdentificationAssignment>,
) -> std::result::Result<TranscriptDocument, String> {
    command_result((|| {
        let recording_id = state.voiceprints.save(&session_id, assignments)?;
        state.storage.transcript(&recording_id)
    })())
}

#[tauri::command]
fn update_recording_speaker_assignments(
    state: State<AppState>,
    recording_id: String,
    assignments: Vec<SpeakerIdentificationAssignment>,
) -> std::result::Result<TranscriptDocument, String> {
    command_result((|| {
        state
            .storage
            .update_recording_speaker_assignments(&recording_id, &assignments)?;
        state.storage.transcript(&recording_id)
    })())
}

#[tauri::command]
fn discard_speaker_identification(state: State<AppState>, session_id: String) {
    state.voiceprints.discard(&session_id);
}

#[tauri::command]
fn list_participants(
    state: State<AppState>,
) -> std::result::Result<Vec<ParticipantProfile>, String> {
    command_result(state.voiceprints.list_participants())
}

#[tauri::command]
fn rename_participant(
    state: State<AppState>,
    id: String,
    display_name: String,
) -> std::result::Result<Vec<ParticipantProfile>, String> {
    command_result((|| {
        state.storage.rename_participant(&id, &display_name)?;
        state.voiceprints.list_participants()
    })())
}

#[tauri::command]
fn delete_participant(
    state: State<AppState>,
    id: String,
) -> std::result::Result<Vec<ParticipantProfile>, String> {
    command_result((|| {
        state.storage.delete_participant(&id)?;
        state.voiceprints.list_participants()
    })())
}

#[tauri::command]
fn delete_voiceprint(
    state: State<AppState>,
    id: String,
) -> std::result::Result<Vec<ParticipantProfile>, String> {
    command_result((|| {
        state.storage.delete_voiceprint(&id)?;
        state.voiceprints.list_participants()
    })())
}

#[tauri::command]
fn reveal_transcript_export(path: String) -> std::result::Result<(), String> {
    command_result((|| {
        let destination = PathBuf::from(path);
        if !destination.is_file() {
            bail!("找不到已导出的转写文件");
        }
        if !destination
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("txt"))
        {
            bail!("只能打开 TXT 转写文件所在的文件夹");
        }
        let directory = destination.parent().context("无法确定导出文件夹")?;
        std::process::Command::new("explorer.exe")
            .arg(directory)
            .spawn()?;
        Ok(())
    })())
}

#[tauri::command]
fn has_active_transcription(state: State<AppState>) -> bool {
    state.asr.has_active()
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn register_shortcuts(app: &AppHandle, settings: &AppSettings) -> Result<()> {
    let shortcuts = app.global_shortcut();
    shortcuts.unregister_all()?;
    if !settings.shortcuts_enabled {
        return Ok(());
    }
    shortcuts.on_shortcut(settings.toggle_shortcut.as_str(), |app, _, event| {
        if event.state != ShortcutState::Pressed {
            return;
        }
        let state = app.state::<AppState>();
        match state.recorder.snapshot().state {
            RecordingState::Recording | RecordingState::Interrupted => {
                let _ = state.recorder.pause(app);
            }
            RecordingState::Paused => {
                let _ = state.recorder.resume(app);
            }
            _ => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
                let _ = app.emit("recording://request-start", "current");
            }
        }
    })?;
    shortcuts.on_shortcut(settings.stop_shortcut.as_str(), |app, _, event| {
        if event.state == ShortcutState::Pressed {
            let state = app.state::<AppState>();
            stop_in_background(app, Arc::clone(&state.recorder));
        }
    })?;
    Ok(())
}

fn create_tray(app: &tauri::App) -> Result<()> {
    let menu = build_tray_menu(app.handle(), RecordingState::Idle, None)?;
    TrayIconBuilder::with_id("main-tray")
        .icon(status_icon(RecordingState::Idle, false))
        .tooltip("Nota · 空闲")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button,
                    button_state,
                    ..
                } if is_window_reveal_click(button, button_state)
            ) {
                show_main_window(tray.app_handle());
            }
        })
        .on_menu_event(|app, event| {
            let state = app.state::<AppState>();
            match event.id().as_ref() {
                "show" => show_main_window(app),
                "capture_prompt" => show_capture_prompt_window(app),
                "start_process" => {
                    show_main_window(app);
                    let _ = app.emit("recording://request-start", "process");
                }
                "start_system" => {
                    show_main_window(app);
                    let _ = app.emit("recording://request-start", "system");
                }
                "toggle" => match state.recorder.snapshot().state {
                    RecordingState::Recording | RecordingState::Interrupted => {
                        let _ = state.recorder.pause(app);
                    }
                    RecordingState::Paused => {
                        let _ = state.recorder.resume(app);
                    }
                    _ => {}
                },
                "stop" => {
                    stop_in_background(app, Arc::clone(&state.recorder));
                }
                "quit" => {
                    if state.runtime_active() {
                        show_main_window(app);
                        let _ = app.emit("recording://request-exit", ());
                    } else {
                        app.exit(0);
                    }
                }
                _ => {}
            }
        })
        .build(app)?;
    Ok(())
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn show_capture_prompt_window(app: &AppHandle) {
    let main_thread_app = app.clone();
    if app
        .run_on_main_thread(move || {
            if let Some(window) = main_thread_app.get_webview_window(CAPTURE_PROMPT_WINDOW) {
                let _ = window.show();
                return;
            }
            if WebviewWindowBuilder::new(
                &main_thread_app,
                CAPTURE_PROMPT_WINDOW,
                WebviewUrl::App("index.html?view=capture-prompt".into()),
            )
            .title("Nota · 音频捕获提醒")
            .inner_size(CAPTURE_PROMPT_WIDTH, CAPTURE_PROMPT_MIN_HEIGHT)
            .resizable(false)
            .decorations(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .focused(false)
            .build()
            .is_err()
            {
                logging::warn(
                    "capture_prompt",
                    "create_failed",
                    &[Field::text(FieldKey::ErrorCode, "window_create_failed")],
                );
                return;
            }
            position_capture_prompt_window_on_main_thread(&main_thread_app);
        })
        .is_err()
    {
        logging::warn(
            "capture_prompt",
            "schedule_failed",
            &[Field::text(
                FieldKey::ErrorCode,
                "main_thread_schedule_failed",
            )],
        );
    }
}

fn position_capture_prompt_window_on_main_thread(app: &AppHandle) {
    let Some(window) = app.get_webview_window(CAPTURE_PROMPT_WINDOW) else {
        return;
    };
    let monitor = app
        .get_webview_window("main")
        .and_then(|main| main.current_monitor().ok().flatten())
        .or_else(|| window.current_monitor().ok().flatten());
    if let Some(monitor) = monitor
        && let Ok(size) = window.outer_size()
    {
        let monitor_position = monitor.position();
        let monitor_size = monitor.size();
        let x = monitor_position.x + monitor_size.width.saturating_sub(size.width + 24) as i32;
        let y = monitor_position.y + monitor_size.height.saturating_sub(size.height + 72) as i32;
        let _ = window.set_position(PhysicalPosition::new(x, y));
    }
}

fn close_capture_prompt_window(app: &AppHandle) {
    let main_thread_app = app.clone();
    if app
        .run_on_main_thread(move || {
            if let Some(window) = main_thread_app.get_webview_window(CAPTURE_PROMPT_WINDOW) {
                let _ = window.close();
            }
        })
        .is_err()
    {
        logging::warn(
            "capture_prompt",
            "close_failed",
            &[Field::text(
                FieldKey::ErrorCode,
                "main_thread_schedule_failed",
            )],
        );
    }
}

fn is_window_reveal_click(button: MouseButton, button_state: MouseButtonState) -> bool {
    button == MouseButton::Left && button_state == MouseButtonState::Up
}

fn build_tray_menu(
    app: &AppHandle,
    state: RecordingState,
    capture_prompt_kind: Option<CapturePromptKind>,
) -> Result<Menu<tauri::Wry>> {
    let show = MenuItem::with_id(app, "show", "显示窗口", true, None::<&str>)?;
    let controls = tray_controls(state);
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    if controls.start_enabled {
        let process = MenuItem::with_id(app, "start_process", "开始应用录音", true, None::<&str>)?;
        let system = MenuItem::with_id(app, "start_system", "开始系统录音", true, None::<&str>)?;
        return Ok(Menu::with_items(
            app,
            &[&show, &process, &system, &separator, &quit],
        )?);
    }

    let toggle = MenuItem::with_id(
        app,
        "toggle",
        controls.toggle_label.unwrap_or("录音处理中…"),
        controls.toggle_enabled,
        None::<&str>,
    )?;
    let stop = MenuItem::with_id(
        app,
        "stop",
        "停止并保存",
        controls.stop_enabled,
        None::<&str>,
    )?;
    if let Some(kind) = capture_prompt_kind {
        let prompt_label = match kind {
            CapturePromptKind::CaptureInterrupted => "应用音频捕获已中断，请确认…",
            CapturePromptKind::ProlongedSilence => "应用已持续 3 分钟没有声音，请确认…",
        };
        let prompt = MenuItem::with_id(app, "capture_prompt", prompt_label, true, None::<&str>)?;
        return Ok(Menu::with_items(
            app,
            &[&show, &prompt, &toggle, &stop, &separator, &quit],
        )?);
    }
    Ok(Menu::with_items(
        app,
        &[&show, &toggle, &stop, &separator, &quit],
    )?)
}

fn schedule_tray_update(app: &AppHandle, state: RecordingState) {
    let capture_prompt_kind = app
        .state::<AppState>()
        .recorder
        .pending_capture_prompt()
        .map(|prompt| prompt.kind);
    {
        let app_state = app.state::<AppState>();
        let mut previous = app_state.tray_state.lock();
        if *previous == Some((state, capture_prompt_kind)) {
            return;
        }
        *previous = Some((state, capture_prompt_kind));
    }
    let main_thread_app = app.clone();
    if app
        .run_on_main_thread(move || {
            update_tray_on_main_thread(&main_thread_app, state, capture_prompt_kind);
        })
        .is_err()
    {
        app.state::<AppState>().tray_state.lock().take();
        logging::warn(
            "tray",
            "update_schedule_failed",
            &[Field::text(
                FieldKey::ErrorCode,
                "main_thread_schedule_failed",
            )],
        );
    }
}

fn update_tray_on_main_thread(
    app: &AppHandle,
    state: RecordingState,
    capture_prompt_kind: Option<CapturePromptKind>,
) {
    let Some(tray) = app.tray_by_id("main-tray") else {
        return;
    };
    let label = match capture_prompt_kind {
        Some(CapturePromptKind::CaptureInterrupted) => "Nota · 应用音频捕获已中断",
        Some(CapturePromptKind::ProlongedSilence) => "Nota · 应用长时间没有声音",
        None => match state {
            RecordingState::Recording | RecordingState::Preparing | RecordingState::Finalizing => {
                "Nota · 正在录音"
            }
            RecordingState::Paused => "Nota · 已暂停",
            RecordingState::Interrupted | RecordingState::Error => "Nota · 音源中断",
            _ => "Nota · 空闲",
        },
    };
    let _ = tray.set_icon(Some(status_icon(state, capture_prompt_kind.is_some())));
    let _ = tray.set_tooltip(Some(label));
    match build_tray_menu(app, state, capture_prompt_kind) {
        Ok(menu) => {
            let _ = tray.set_menu(Some(menu));
        }
        Err(_) => {
            logging::warn(
                "tray",
                "menu_rebuild_failed",
                &[Field::text(FieldKey::ErrorCode, "menu_rebuild_failed")],
            );
        }
    }
}

fn tray_controls(state: RecordingState) -> TrayControls {
    match state {
        RecordingState::Idle | RecordingState::Completed | RecordingState::Error => TrayControls {
            start_enabled: true,
            toggle_label: None,
            toggle_enabled: false,
            stop_enabled: false,
        },
        RecordingState::Recording | RecordingState::Interrupted => TrayControls {
            start_enabled: false,
            toggle_label: Some("暂停录音"),
            toggle_enabled: true,
            stop_enabled: true,
        },
        RecordingState::Paused => TrayControls {
            start_enabled: false,
            toggle_label: Some("继续录音"),
            toggle_enabled: true,
            stop_enabled: true,
        },
        RecordingState::Preparing => TrayControls {
            start_enabled: false,
            toggle_label: Some("正在准备…"),
            toggle_enabled: false,
            stop_enabled: false,
        },
        RecordingState::Finalizing => TrayControls {
            start_enabled: false,
            toggle_label: Some("正在保存…"),
            toggle_enabled: false,
            stop_enabled: false,
        },
        RecordingState::Recovering => TrayControls {
            start_enabled: false,
            toggle_label: Some("正在恢复…"),
            toggle_enabled: false,
            stop_enabled: false,
        },
    }
}

fn status_icon(state: RecordingState, capture_prompt_pending: bool) -> Image<'static> {
    // Compile-time decoding reuses the installer's icon family without a runtime
    // image decoder or filesystem access. Keep the full-resolution app mark intact.
    let base = tauri::include_image!("icons/32x32.png");
    let Some(color) = tray_badge_color(state, capture_prompt_pending) else {
        return base;
    };
    let (width, height) = (base.width(), base.height());
    let mut pixels = base.rgba().to_vec();
    let radius = width.min(height) as f32 * 0.1875;
    let center_x = width as f32 - radius - 1.0;
    let center_y = height as f32 - radius - 1.0;
    // A light separator keeps the small badge readable over the app mark and
    // both taskbar themes. Source-over coverage anti-aliases the circle edges.
    for (radius, color) in [(radius, [255, 255, 255]), (radius * 0.75, color)] {
        for y in 0..height {
            for x in 0..width {
                let distance = ((x as f32 + 0.5 - center_x).powi(2)
                    + (y as f32 + 0.5 - center_y).powi(2))
                .sqrt();
                let coverage = (radius + 0.5 - distance).clamp(0.0, 1.0);
                if coverage == 0.0 {
                    continue;
                }
                let offset = ((y * width + x) * 4) as usize;
                let base_alpha = pixels[offset + 3] as f32 / 255.0;
                let alpha = coverage + base_alpha * (1.0 - coverage);
                for channel in 0..3 {
                    pixels[offset + channel] = ((color[channel] as f32 * coverage
                        + pixels[offset + channel] as f32 * base_alpha * (1.0 - coverage))
                        / alpha)
                        .round() as u8;
                }
                pixels[offset + 3] = (alpha * 255.0).round() as u8;
            }
        }
    }
    Image::new_owned(pixels, width, height)
}

fn tray_badge_color(state: RecordingState, capture_prompt_pending: bool) -> Option<[u8; 3]> {
    // Native counterparts of --color-status-{recording,paused,warning-accent}
    // in design-tokens.css. Pending capture decisions retain highest priority.
    if capture_prompt_pending {
        return Some([217, 154, 66]);
    }
    match state {
        RecordingState::Recording | RecordingState::Preparing | RecordingState::Finalizing => {
            Some([200, 79, 69])
        }
        RecordingState::Paused => Some([189, 139, 53]),
        RecordingState::Interrupted | RecordingState::Error => Some([217, 154, 66]),
        RecordingState::Idle | RecordingState::Completed | RecordingState::Recovering => None,
    }
}

pub fn run_app() {
    let paths = AppPaths::discover().expect("无法初始化 Nota 数据目录");
    crate::logging::init(&paths.logs).expect("无法初始化 Nota 技术日志");
    logging::info(
        "application",
        "started",
        &[
            Field::text(FieldKey::AppVersion, env!("CARGO_PKG_VERSION")),
            Field::number(FieldKey::ProcessId, u64::from(std::process::id())),
        ],
    );
    let storage = Arc::new(Storage::open(paths).expect("无法初始化 Nota 数据库"));
    clean_stale_temporary_chunks(&storage.paths().recovery)
        .expect("无法清理上次遗留的转写临时文件");
    storage
        .interrupt_running_transcriptions()
        .expect("无法恢复上次中断的转写任务状态");
    storage
        .interrupt_running_ai_generations()
        .expect("无法恢复上次中断的 AI 文档任务状态");
    let recorder = Arc::new(RecordingController::new(Arc::clone(&storage)));
    let weak_recorder = Arc::downgrade(&recorder);
    let asr = Arc::new(AsrManager::new(
        Arc::clone(&storage),
        Arc::new(move || {
            weak_recorder
                .upgrade()
                .is_some_and(|recorder| recorder.is_active())
        }),
    ));
    let ai = Arc::new(AiManager::new(Arc::clone(&storage)));
    let weak_recorder_for_import = Arc::downgrade(&recorder);
    let importer = Arc::new(
        AudioImportManager::new(
            Arc::clone(&storage),
            Arc::new(move || {
                weak_recorder_for_import
                    .upgrade()
                    .is_some_and(|recorder| recorder.is_active())
            }),
        )
        .expect("无法初始化 Nota 音频导入管理器"),
    );
    let voiceprints =
        Arc::new(VoiceprintManager::new(Arc::clone(&storage)).expect("无法初始化 Nota 声纹管理"));
    let application = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .manage(AppState {
            storage,
            recorder,
            asr,
            ai,
            importer,
            voiceprints,
            media_start_gate: Mutex::new(()),
            tray_state: Mutex::new(Some((RecordingState::Idle, None))),
        })
        .setup(|app| {
            create_tray(app).map_err(|error| anyhow!(error))?;
            let settings = app.state::<AppState>().storage.settings()?;
            if let Err(error) = register_shortcuts(app.handle(), &settings) {
                let state = app.state::<AppState>();
                state.recorder.snapshot.lock().fault = Some(RecordingFault {
                    component: "shortcut".into(),
                    code: "SHORTCUT_CONFLICT".into(),
                    recoverable: true,
                    user_message: format!("全局快捷键注册失败：{error}"),
                    occurred_at: Utc::now().to_rfc3339(),
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main"
                && let tauri::WindowEvent::CloseRequested { api, .. } = event
            {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            list_capture_targets,
            list_audio_devices,
            get_settings,
            save_settings,
            list_hotword_lists,
            get_hotword_list,
            save_hotword_list,
            delete_hotword_list,
            get_recording_snapshot,
            get_capture_prompt,
            respond_capture_prompt,
            resize_capture_prompt,
            start_recording,
            pause_recording,
            resume_recording,
            stop_recording,
            switch_capture_source,
            set_microphone_enabled,
            list_recordings,
            start_audio_import,
            get_audio_import_snapshot,
            cancel_audio_import,
            prepare_recording_playback,
            list_recoverable_recordings,
            recover_recording,
            rename_recording,
            reveal_recording,
            delete_recording,
            delete_recoverable_recording,
            open_microphone_settings,
            open_log_directory,
            list_asr_providers,
            save_asr_provider,
            delete_asr_provider,
            set_active_asr_provider,
            test_asr_provider,
            list_asr_models,
            get_transcription_options,
            list_llm_providers,
            save_llm_provider,
            delete_llm_provider,
            set_active_llm_provider,
            test_llm_provider,
            list_llm_models,
            list_ai_templates,
            save_ai_template,
            clone_ai_template,
            archive_ai_template,
            get_ai_workspace,
            preview_ai_generation_request,
            copy_ai_request_body,
            copy_ai_generation_json,
            list_ai_document_versions,
            generate_ai_document,
            cancel_ai_generation,
            read_ai_document_version,
            read_ai_generation_details,
            relink_ai_document_version,
            find_ai_document_version,
            open_ai_document_version,
            reveal_ai_document_version,
            copy_ai_document_version,
            copy_ai_document_path,
            export_ai_document_pdf,
            reveal_ai_document_pdf,
            start_transcription,
            cancel_transcription,
            resume_transcription,
            get_transcript,
            list_transcription_versions,
            select_transcription_version,
            copy_transcript,
            export_transcript,
            reveal_transcript_export,
            identify_recording_speakers,
            save_speaker_identification,
            update_recording_speaker_assignments,
            discard_speaker_identification,
            list_participants,
            rename_participant,
            delete_participant,
            delete_voiceprint,
            has_active_transcription,
            quit_application,
        ])
        .build(tauri::generate_context!())
        .expect("Nota 构建失败");
    application.run(|app, event| {
        if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
            let state = app.state::<AppState>();
            if state.recorder.stop_and_wait(app).is_err() {
                logging::error(
                    "application",
                    "shutdown_finalization_failed",
                    &[Field::text(
                        FieldKey::ErrorCode,
                        "recording_finalize_failed",
                    )],
                );
            }
            if state.asr.interrupt_all().is_err() {
                logging::error(
                    "application",
                    "asr_interrupt_failed",
                    &[Field::text(FieldKey::ErrorCode, "asr_interrupt_failed")],
                );
            }
            if state.ai.interrupt_all().is_err() {
                logging::error(
                    "application",
                    "ai_interrupt_failed",
                    &[Field::text(FieldKey::ErrorCode, "ai_interrupt_failed")],
                );
            }
            if state.importer.interrupt_all().is_err() {
                logging::error(
                    "application",
                    "import_interrupt_failed",
                    &[Field::text(FieldKey::ErrorCode, "import_interrupt_failed")],
                );
            }
        }
    });
}

#[cfg(test)]
mod settings_tests {
    use super::*;

    fn settings() -> AppSettings {
        AppSettings {
            output_directory: "C:\\Recordings".into(),
            ai_documents_directory: "C:\\Documents".into(),
            aec_mode: AecMode::Auto,
            microphone_enabled: true,
            first_run_complete: true,
            shortcuts_enabled: true,
            toggle_shortcut: "Ctrl+Alt+F9".into(),
            stop_shortcut: "Ctrl+Alt+F10".into(),
            active_asr_provider_id: None,
            voiceprint_provider_id: None,
            auto_transcribe: false,
            auto_transcribe_hotword_list_id: None,
            active_llm_provider_id: None,
        }
    }

    #[test]
    fn unrelated_settings_do_not_require_shortcut_registration() {
        let previous = settings();
        let mut next = previous.clone();
        next.output_directory = "D:\\Meetings".into();
        next.aec_mode = AecMode::On;
        next.auto_transcribe = true;

        assert!(!shortcut_settings_changed(&previous, &next));
    }

    #[test]
    fn every_shortcut_field_requires_registration() {
        let previous = settings();

        let mut disabled = previous.clone();
        disabled.shortcuts_enabled = false;
        assert!(shortcut_settings_changed(&previous, &disabled));

        let mut toggle = previous.clone();
        toggle.toggle_shortcut = "Ctrl+Alt+F7".into();
        assert!(shortcut_settings_changed(&previous, &toggle));

        let mut stop = previous.clone();
        stop.stop_shortcut = "Ctrl+Alt+F8".into();
        assert!(shortcut_settings_changed(&previous, &stop));
    }
}

#[cfg(test)]
mod transcript_export_tests {
    use super::*;

    fn transcript(text: &str, segments: Vec<TranscriptSegment>) -> TranscriptDocument {
        TranscriptDocument {
            recording_id: "recording".into(),
            generation: 1,
            status: TranscriptionStatus::Completed,
            provider_name: "FunASR".into(),
            provider_kind: AsrProviderKind::FunAsr,
            model_id: "sensevoice".into(),
            speaker_count: None,
            protocol: TranscriptionProtocol::NotaBatchV1,
            voiceprint_analysis_supported: true,
            language: Some("zh".into()),
            text: text.into(),
            segments,
            speaker_names: std::collections::BTreeMap::new(),
            speaker_assignments: std::collections::BTreeMap::new(),
            completed_chunks: 1,
            total_chunks: 1,
            error_message: None,
            updated_at: "2026-08-02T00:00:00Z".into(),
            completed_at: Some("2026-08-02T00:00:00Z".into()),
            hotword_list_name: None,
            hotword_count: 0,
        }
    }

    #[test]
    fn formats_speaker_segments_for_copy_and_txt_export() {
        let value = transcript(
            "大家好。收到。继续。",
            vec![
                TranscriptSegment {
                    start_ms: 0,
                    end_ms: 1_000,
                    text: " 大家好。 ".into(),
                    speaker: Some("speaker_0".into()),
                },
                TranscriptSegment {
                    start_ms: 1_000,
                    end_ms: 2_000,
                    text: "收到。".into(),
                    speaker: Some("speaker_1".into()),
                },
                TranscriptSegment {
                    start_ms: 2_000,
                    end_ms: 3_000,
                    text: "继续。".into(),
                    speaker: None,
                },
            ],
        );

        assert_eq!(
            format_transcript_text(&value),
            "speaker_0：大家好。\r\nspeaker_1：收到。\r\n继续。"
        );
    }

    #[test]
    fn preserves_plain_transcript_when_no_speaker_labels_exist() {
        let value = transcript(
            " 完整的纯文本转写。 ",
            vec![TranscriptSegment {
                start_ms: 0,
                end_ms: 1_000,
                text: "分段文本".into(),
                speaker: None,
            }],
        );

        assert_eq!(format_transcript_text(&value), "完整的纯文本转写。");
    }

    #[test]
    fn resolved_participant_names_are_used_for_copy_and_export() {
        let mut value = transcript(
            "大家好。",
            vec![TranscriptSegment {
                start_ms: 0,
                end_ms: 1_000,
                text: "大家好。".into(),
                speaker: Some("speaker_0".into()),
            }],
        );
        value
            .speaker_names
            .insert("speaker_0".into(), "小明".into());

        assert_eq!(format_transcript_text(&value), "小明：大家好。");
        assert_eq!(value.segments[0].speaker.as_deref(), Some("speaker_0"));
    }
}

#[cfg(test)]
mod tray_tests {
    use super::*;

    #[test]
    fn idle_tray_uses_unmodified_packaged_app_icon() {
        let base = tauri::include_image!("icons/32x32.png");
        for state in [
            RecordingState::Idle,
            RecordingState::Completed,
            RecordingState::Recovering,
        ] {
            let icon = status_icon(state, false);
            assert_eq!((icon.width(), icon.height()), (32, 32));
            assert_eq!(icon.rgba(), base.rgba());
        }
    }

    #[test]
    fn recording_badge_changes_only_the_bottom_right_corner() {
        let base = tauri::include_image!("icons/32x32.png");
        let recording = status_icon(RecordingState::Recording, false);
        let mut changed = 0;
        for (index, (before, after)) in base
            .rgba()
            .as_chunks::<4>()
            .0
            .iter()
            .zip(recording.rgba().as_chunks::<4>().0)
            .enumerate()
        {
            if before != after {
                let (x, y) = (index % 32, index / 32);
                assert!(x >= 18 && y >= 18, "badge must not replace the app mark");
                changed += 1;
            }
        }
        assert!(changed > 0 && changed < 32 * 32 / 4);
        let center = (25 * 32 + 25) * 4;
        assert_eq!(&recording.rgba()[center..center + 4], &[200, 79, 69, 255]);
        assert!(
            recording
                .rgba()
                .as_chunks::<4>()
                .0
                .contains(&[255, 255, 255, 255])
        );
        assert!(
            recording
                .rgba()
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] > 0 && pixel[3] < 255)
        );
        // Every new state is composed from the base, never from a previous badge.
        assert_eq!(status_icon(RecordingState::Idle, false).rgba(), base.rgba());
    }

    #[test]
    fn tray_badges_preserve_busy_pause_and_capture_warning_states() {
        for state in [
            RecordingState::Preparing,
            RecordingState::Recording,
            RecordingState::Finalizing,
        ] {
            assert_eq!(tray_badge_color(state, false), Some([200, 79, 69]));
        }
        assert_eq!(
            tray_badge_color(RecordingState::Paused, false),
            Some([189, 139, 53])
        );
        for state in [RecordingState::Interrupted, RecordingState::Error] {
            assert_eq!(tray_badge_color(state, false), Some([217, 154, 66]));
        }
        for state in [
            RecordingState::Idle,
            RecordingState::Preparing,
            RecordingState::Recording,
            RecordingState::Paused,
            RecordingState::Finalizing,
            RecordingState::Completed,
            RecordingState::Interrupted,
            RecordingState::Error,
            RecordingState::Recovering,
        ] {
            assert_eq!(tray_badge_color(state, true), Some([217, 154, 66]));
            let icon = status_icon(state, true);
            let center = (25 * 32 + 25) * 4;
            assert_eq!(&icon.rgba()[center..center + 4], &[217, 154, 66, 255]);
        }
    }

    #[test]
    fn capture_prompt_height_is_bounded_and_rejects_invalid_values() {
        assert_eq!(
            normalize_capture_prompt_height(120.0).unwrap(),
            CAPTURE_PROMPT_MIN_HEIGHT
        );
        assert_eq!(normalize_capture_prompt_height(220.0).unwrap(), 220.0);
        assert_eq!(
            normalize_capture_prompt_height(500.0).unwrap(),
            CAPTURE_PROMPT_MAX_HEIGHT
        );
        assert!(normalize_capture_prompt_height(f64::NAN).is_err());
    }

    #[test]
    fn capture_interruption_can_replace_silence_but_not_the_reverse() {
        let silence = CapturePrompt {
            session_id: "session-1".into(),
            target_name: "Meeting".into(),
            kind: CapturePromptKind::ProlongedSilence,
        };
        let interruption = CapturePrompt {
            kind: CapturePromptKind::CaptureInterrupted,
            ..silence.clone()
        };

        assert!(should_replace_capture_prompt(
            Some(&silence),
            "session-1",
            CapturePromptKind::CaptureInterrupted
        ));
        assert!(!should_replace_capture_prompt(
            Some(&interruption),
            "session-1",
            CapturePromptKind::ProlongedSilence
        ));
        assert!(!should_replace_capture_prompt(
            Some(&silence),
            "session-1",
            CapturePromptKind::ProlongedSilence
        ));
        assert!(should_replace_capture_prompt(
            Some(&interruption),
            "session-2",
            CapturePromptKind::ProlongedSilence
        ));
    }

    #[test]
    fn tray_actions_match_recording_state() {
        assert_eq!(
            tray_controls(RecordingState::Idle),
            TrayControls {
                start_enabled: true,
                toggle_label: None,
                toggle_enabled: false,
                stop_enabled: false,
            }
        );
        assert_eq!(
            tray_controls(RecordingState::Recording),
            TrayControls {
                start_enabled: false,
                toggle_label: Some("暂停录音"),
                toggle_enabled: true,
                stop_enabled: true,
            }
        );
        assert_eq!(
            tray_controls(RecordingState::Paused),
            TrayControls {
                start_enabled: false,
                toggle_label: Some("继续录音"),
                toggle_enabled: true,
                stop_enabled: true,
            }
        );
        assert!(!tray_controls(RecordingState::Finalizing).toggle_enabled);
        assert!(tray_controls(RecordingState::Completed).start_enabled);
    }

    #[test]
    fn only_a_released_left_click_reveals_the_window() {
        assert!(is_window_reveal_click(
            MouseButton::Left,
            MouseButtonState::Up
        ));
        assert!(!is_window_reveal_click(
            MouseButton::Left,
            MouseButtonState::Down
        ));
        assert!(!is_window_reveal_click(
            MouseButton::Right,
            MouseButtonState::Up
        ));
    }

    #[test]
    fn packet_drain_has_a_per_tick_budget() {
        let (sender, receiver) = unbounded();
        for timestamp in 0..(MAX_PACKETS_PER_TICK + 10) {
            sender
                .send(AudioPacket {
                    samples: vec![0.0],
                    sample_rate: 48_000,
                    timestamp_100ns: timestamp as u64,
                    device_position: Some(timestamp as u64),
                    discontinuity: false,
                    source_epoch: 0,
                })
                .unwrap();
        }

        let mut drained = 0;
        assert!(drain_packets(&receiver, |_| drained += 1));
        assert_eq!(drained, MAX_PACKETS_PER_TICK);
        assert_eq!(receiver.len(), 10);
    }

    #[test]
    fn microphone_packet_drain_accepts_only_the_committed_source_epoch() {
        let (sender, receiver) = unbounded();
        for source_epoch in [4, 5, 4, 5] {
            sender
                .send(AudioPacket {
                    samples: vec![source_epoch as f32],
                    sample_rate: 48_000,
                    timestamp_100ns: source_epoch,
                    device_position: Some(source_epoch),
                    discontinuity: false,
                    source_epoch,
                })
                .unwrap();
        }

        let mut accepted = Vec::new();
        assert!(drain_current_source_packets(&receiver, 5, |packet| {
            accepted.push(packet.source_epoch)
        }));
        assert_eq!(accepted, vec![5, 5]);
        assert!(receiver.is_empty());
    }

    #[test]
    fn source_epoch_wraps_without_reusing_the_disabled_epoch() {
        assert_eq!(next_source_epoch(0), 1);
        assert_eq!(next_source_epoch(41), 42);
        assert_eq!(next_source_epoch(u64::MAX), 1);
    }
}
