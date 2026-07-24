use crate::audio::{
    AudioMixer, AudioPacket, CaptureHandle, CaptureSource, OpusOggWriter,
    list_audio_devices as enumerate_audio_devices,
    list_capture_targets as enumerate_capture_targets, list_recoverable_files, move_verified,
    recover_ogg_file, start_capture,
};
use crate::models::*;
use crate::paths::AppPaths;
use crate::state_machine::{RecordingEvent, transition};
use crate::storage::Storage;
use anyhow::{Context, Result, anyhow, bail};
use chrono::{Local, Utc};
use crossbeam_channel::{Receiver, Sender, TryRecvError, unbounded};
use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use uuid::Uuid;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::PCWSTR;

struct AppState {
    storage: Arc<Storage>,
    recorder: Arc<RecordingController>,
    tray_menu: Mutex<Option<TrayMenuItems>>,
}

struct TrayMenuItems {
    toggle: MenuItem<tauri::Wry>,
    stop: MenuItem<tauri::Wry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrayControls {
    toggle_label: &'static str,
    toggle_enabled: bool,
    stop_enabled: bool,
}

struct RecordingRuntime {
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    system_capture: Option<CaptureHandle>,
    microphone_capture: Option<CaptureHandle>,
    system_sender: Sender<AudioPacket>,
    microphone_sender: Sender<AudioPacket>,
    system_health: Arc<Mutex<Arc<AtomicBool>>>,
    microphone_health: Arc<Mutex<Arc<AtomicBool>>>,
    microphone_enabled: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

struct RecordingController {
    snapshot: Arc<Mutex<RecordingSnapshot>>,
    runtime: Mutex<Option<RecordingRuntime>>,
    storage: Arc<Storage>,
}

impl RecordingController {
    fn new(storage: Arc<Storage>) -> Self {
        Self {
            snapshot: Arc::new(Mutex::new(RecordingSnapshot::default())),
            runtime: Mutex::new(None),
            storage,
        }
    }

    fn snapshot(&self) -> RecordingSnapshot {
        self.snapshot.lock().clone()
    }

    fn start(&self, app: AppHandle, request: StartRecordingRequest) -> Result<RecordingSnapshot> {
        if !request.consent_confirmed {
            bail!("开始录音前必须确认已告知参会者");
        }
        if self.runtime.lock().is_some() {
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
        log::info!("recording prepare session={session_id}");
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
        log::info!("recording started session={session_id}");

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

        let (system_tx, system_rx) = unbounded::<AudioPacket>();
        let system_source = capture_source_from_selection(&request.capture)?;
        let system_capture = match start_capture(system_source, system_tx.clone()) {
            Ok(handle) => handle,
            Err(error) => {
                drop(writer);
                let _ = std::fs::remove_file(&partial_path);
                self.set_error(&app, "system", "CAPTURE_START_FAILED", &error);
                bail!("无法启动所选录音来源：{error:#}。可切换到“全部系统声音”后重试")
            }
        };

        let (microphone_tx, microphone_rx) = unbounded::<AudioPacket>();
        let (microphone_capture, microphone_error) = match request.microphone.clone() {
            Some(selection) => {
                match start_capture(CaptureSource::Microphone(selection), microphone_tx.clone()) {
                    Ok(handle) => (Some(handle), None),
                    Err(error) => (None, Some(format!("{error:#}"))),
                }
            }
            None => (None, None),
        };
        let microphone_expected = request.microphone.is_some();
        let auto_should_enable_aec = auto_should_enable_aec(&request.capture);
        let aec_enabled = microphone_capture.is_some()
            && (matches!(request.aec_mode, AecMode::On)
                || (matches!(request.aec_mode, AecMode::Auto) && auto_should_enable_aec));
        let system_health = Arc::new(Mutex::new(system_capture.health_flag()));
        let microphone_health = Arc::new(Mutex::new(
            microphone_capture
                .as_ref()
                .map(CaptureHandle::health_flag)
                .unwrap_or_else(|| Arc::new(AtomicBool::new(false))),
        ));
        let microphone_enabled = Arc::new(AtomicBool::new(microphone_expected));
        let output_path = unique_recording_path(&output_directory);

        self.storage
            .record_consent(&session_id, &self.storage.settings()?.consent_template)?;

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
                    microphone_expected,
                    auto_should_enable_aec,
                    worker_system_health,
                    worker_microphone_health,
                    worker_microphone_enabled,
                    worker_stop,
                    worker_paused,
                );
            })?;
        *self.runtime.lock() = Some(RecordingRuntime {
            stop,
            paused,
            system_capture: Some(system_capture),
            microphone_capture,
            system_sender: system_tx,
            microphone_sender: microphone_tx,
            system_health,
            microphone_health,
            microphone_enabled,
            worker: Some(worker),
        });
        Ok(self.snapshot())
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
        emit_snapshot(app, &self.snapshot);
        Ok(self.snapshot())
    }

    fn stop(&self, app: &AppHandle) -> Result<RecordingSnapshot> {
        let mut runtime = match self.runtime.lock().take() {
            Some(runtime) => runtime,
            None => return Ok(self.snapshot()),
        };
        {
            let mut snapshot = self.snapshot.lock();
            snapshot.state = transition(snapshot.state, RecordingEvent::Stop)?;
        }
        emit_snapshot(app, &self.snapshot);
        runtime.stop.store(true, Ordering::Release);
        log::info!("recording finalizing");
        if let Some(capture) = runtime.system_capture.take() {
            log::info!("stopping system capture");
            capture.stop();
            log::info!("system capture stopped");
        }
        if let Some(capture) = runtime.microphone_capture.take() {
            log::info!("stopping microphone capture");
            capture.stop();
            log::info!("microphone capture stopped");
        }
        if let Some(worker) = runtime.worker.take() {
            log::info!("waiting for recording worker");
            worker.join().map_err(|_| anyhow!("录音封装线程异常结束"))?;
            log::info!("recording worker stopped");
        }
        emit_snapshot(app, &self.snapshot);
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
        let _ = app.emit("recording://snapshot", value);
    }

    fn switch_capture(
        &self,
        app: &AppHandle,
        selection: CaptureSelection,
    ) -> Result<RecordingSnapshot> {
        let mut runtime = self.runtime.lock();
        let Some(runtime) = runtime.as_mut() else {
            return Ok(self.snapshot());
        };
        let source = capture_source_from_selection(&selection)?;
        let replacement = start_capture(source, runtime.system_sender.clone())?;
        let replacement_health = replacement.health_flag();
        *runtime.system_health.lock() = replacement_health;
        if let Some(previous) = runtime.system_capture.replace(replacement) {
            previous.stop();
        }
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
        update_tray(app, value.state);
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
                let replacement = start_capture(
                    CaptureSource::Microphone(selection),
                    runtime.microphone_sender.clone(),
                )?;
                *runtime.microphone_health.lock() = replacement.health_flag();
                runtime.microphone_enabled.store(true, Ordering::Release);
                if let Some(previous) = runtime.microphone_capture.replace(replacement) {
                    previous.stop();
                }
                let mut snapshot = self.snapshot.lock();
                snapshot.microphone.healthy = true;
                snapshot.microphone.detail = Some("已启用".into());
                snapshot.aec_status = AecStatus::Converging;
            }
            None => {
                runtime.microphone_enabled.store(false, Ordering::Release);
                runtime
                    .microphone_health
                    .lock()
                    .store(false, Ordering::Release);
                if let Some(previous) = runtime.microphone_capture.take() {
                    previous.stop();
                }
                let mut snapshot = self.snapshot.lock();
                snapshot.microphone.healthy = false;
                snapshot.microphone.detail = Some("已关闭".into());
                snapshot.aec_status = AecStatus::Disabled;
            }
        }
        emit_snapshot(app, &self.snapshot);
        Ok(self.snapshot())
    }
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
    microphone_expected: bool,
    auto_should_enable_aec: bool,
    system_health: Arc<Mutex<Arc<AtomicBool>>>,
    microphone_health: Arc<Mutex<Arc<AtomicBool>>>,
    microphone_enabled: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
) {
    let mut mixer = AudioMixer::new(request.aec_mode, auto_should_enable_aec);
    let mut system_connected;
    let mut microphone_connected;
    let mut last_emit = Instant::now();
    let mut last_disk_check = Instant::now();
    let mut was_paused = false;
    let mut microphone_was_enabled = microphone_expected;
    let tick = crossbeam_channel::tick(Duration::from_millis(10));
    let mut failure: Option<anyhow::Error> = None;

    while !stop.load(Ordering::Acquire) {
        let _ = tick.recv();
        let _ = drain_packets(&system_rx, |packet| mixer.push_system(packet));
        system_connected = system_health.lock().load(Ordering::Acquire);
        let microphone_is_enabled = microphone_enabled.load(Ordering::Acquire);
        let _ = drain_packets(&microphone_rx, |packet| mixer.push_microphone(packet));
        microphone_connected =
            microphone_is_enabled && microphone_health.lock().load(Ordering::Acquire);
        if microphone_is_enabled != microphone_was_enabled {
            mixer.reset(request.aec_mode, auto_should_enable_aec);
            microphone_was_enabled = microphone_is_enabled;
        }
        if paused.load(Ordering::Acquire) {
            was_paused = true;
            continue;
        }
        if was_paused {
            mixer.reset(request.aec_mode, auto_should_enable_aec);
            was_paused = false;
        }
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
            if value.aec_status == AecStatus::Converging && value.active_duration_ms >= 2_000 {
                value.aec_status = AecStatus::Enabled;
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

    let stop_was_requested = stop.load(Ordering::Acquire);
    log::info!("recording worker leaving mix loop");
    snapshot.lock().state = RecordingState::Finalizing;
    if !stop_was_requested {
        emit_snapshot(&app, &snapshot);
    }
    log::info!("recording worker finishing Ogg stream");
    let finalization = writer.finish().and_then(|_| {
        log::info!("Ogg stream finished; moving recording to output");
        move_verified(&partial_path, &output_path)?;
        log::info!("recording moved to output");
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
            log::info!("recording completed bytes={}", item.size_bytes);
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
            log::error!("recording finalization failed: {error:#}");
        }
    }
    if !stop_was_requested {
        emit_snapshot(&app, &snapshot);
    }
}

fn drain_packets(receiver: &Receiver<AudioPacket>, mut consume: impl FnMut(AudioPacket)) -> bool {
    loop {
        match receiver.try_recv() {
            Ok(packet) => consume(packet),
            Err(TryRecvError::Empty) => return true,
            Err(TryRecvError::Disconnected) => return false,
        }
    }
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

fn emit_snapshot(app: &AppHandle, snapshot: &Arc<Mutex<RecordingSnapshot>>) {
    let value = snapshot.lock().clone();
    let _ = app.emit("recording://snapshot", value.clone());
    update_tray(app, value.state);
}

fn command_result<T>(result: Result<T>) -> std::result::Result<T, String> {
    result.map_err(|error| format!("{error:#}"))
}

#[tauri::command]
fn list_capture_targets() -> std::result::Result<Vec<CaptureTarget>, String> {
    command_result(enumerate_capture_targets())
}

#[tauri::command]
fn list_audio_devices() -> std::result::Result<Vec<AudioDevice>, String> {
    command_result(enumerate_audio_devices())
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> std::result::Result<AppSettings, String> {
    command_result(state.storage.settings())
}

#[tauri::command]
fn save_settings(
    app: AppHandle,
    state: State<AppState>,
    settings: AppSettings,
) -> std::result::Result<(), String> {
    command_result((|| {
        let previous = state.storage.settings()?;
        if let Err(error) = register_shortcuts(&app, &settings) {
            let _ = register_shortcuts(&app, &previous);
            bail!("快捷键注册失败，可能与其他应用冲突：{error}");
        }
        state.storage.save_settings(&settings)
    })())
}

#[tauri::command]
fn get_recording_snapshot(state: State<AppState>) -> RecordingSnapshot {
    state.recorder.snapshot()
}

#[tauri::command]
fn start_recording(
    app: AppHandle,
    state: State<AppState>,
    request: StartRecordingRequest,
) -> std::result::Result<RecordingSnapshot, String> {
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
fn stop_recording(
    app: AppHandle,
    state: State<AppState>,
) -> std::result::Result<RecordingSnapshot, String> {
    command_result(state.recorder.stop(&app))
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
        self.recorder.runtime.lock().is_some()
    }
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
    if state.runtime_active() {
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
) -> std::result::Result<(), String> {
    command_result((|| {
        let item = state.storage.find_recording(&id)?;
        if permanent {
            std::fs::remove_file(&item.path)?;
        } else {
            trash::delete(&item.path)?;
        }
        state.storage.remove_recording(&id)?;
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
fn copy_consent_template(text: String) -> std::result::Result<(), String> {
    command_result((|| {
        arboard::Clipboard::new()?.set_text(text)?;
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
fn quit_application(
    app: AppHandle,
    state: State<AppState>,
    stop_and_save: bool,
) -> std::result::Result<(), String> {
    command_result((|| {
        if state.runtime_active() {
            if !stop_and_save {
                bail!("录音仍在进行");
            }
            state.recorder.stop(&app)?;
        }
        app.exit(0);
        Ok(())
    })())
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
                let _ = app.emit("recording://request-start", ());
            }
        }
    })?;
    shortcuts.on_shortcut(settings.stop_shortcut.as_str(), |app, _, event| {
        if event.state == ShortcutState::Pressed {
            let state = app.state::<AppState>();
            let _ = state.recorder.stop(app);
        }
    })?;
    Ok(())
}

fn create_tray(app: &tauri::App) -> Result<()> {
    let show = MenuItem::with_id(app, "show", "显示窗口", true, None::<&str>)?;
    let controls = tray_controls(RecordingState::Idle);
    let toggle = MenuItem::with_id(
        app,
        "toggle",
        controls.toggle_label,
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
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &toggle, &stop, &separator, &quit])?;
    TrayIconBuilder::with_id("main-tray")
        .icon(status_icon(RecordingState::Idle))
        .tooltip("Nota · 空闲")
        .menu(&menu)
        .on_menu_event(|app, event| {
            let state = app.state::<AppState>();
            match event.id().as_ref() {
                "show" => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
                "toggle" => match state.recorder.snapshot().state {
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
                        let _ = app.emit("recording://request-start", ());
                    }
                },
                "stop" => {
                    let _ = state.recorder.stop(app);
                }
                "quit" => {
                    if state.runtime_active() {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                        let _ = app.emit("recording://request-exit", ());
                    } else {
                        app.exit(0);
                    }
                }
                _ => {}
            }
        })
        .build(app)?;
    *app.state::<AppState>().tray_menu.lock() = Some(TrayMenuItems { toggle, stop });
    Ok(())
}

fn update_tray(app: &AppHandle, state: RecordingState) {
    let Some(tray) = app.tray_by_id("main-tray") else {
        return;
    };
    let label = match state {
        RecordingState::Recording | RecordingState::Preparing | RecordingState::Finalizing => {
            "Nota · 正在录音"
        }
        RecordingState::Paused => "Nota · 已暂停",
        RecordingState::Interrupted | RecordingState::Error => "Nota · 音源中断",
        _ => "Nota · 空闲",
    };
    let _ = tray.set_icon(Some(status_icon(state)));
    let _ = tray.set_tooltip(Some(label));
    let controls = tray_controls(state);
    if let Some(items) = app.state::<AppState>().tray_menu.lock().as_ref() {
        let _ = items.toggle.set_text(controls.toggle_label);
        let _ = items.toggle.set_enabled(controls.toggle_enabled);
        let _ = items.stop.set_enabled(controls.stop_enabled);
    }
}

fn tray_controls(state: RecordingState) -> TrayControls {
    match state {
        RecordingState::Idle | RecordingState::Completed | RecordingState::Error => TrayControls {
            toggle_label: "开始录音…",
            toggle_enabled: true,
            stop_enabled: false,
        },
        RecordingState::Recording | RecordingState::Interrupted => TrayControls {
            toggle_label: "暂停录音",
            toggle_enabled: true,
            stop_enabled: true,
        },
        RecordingState::Paused => TrayControls {
            toggle_label: "继续录音",
            toggle_enabled: true,
            stop_enabled: true,
        },
        RecordingState::Preparing => TrayControls {
            toggle_label: "正在准备…",
            toggle_enabled: false,
            stop_enabled: false,
        },
        RecordingState::Finalizing => TrayControls {
            toggle_label: "正在保存…",
            toggle_enabled: false,
            stop_enabled: false,
        },
        RecordingState::Recovering => TrayControls {
            toggle_label: "正在恢复…",
            toggle_enabled: false,
            stop_enabled: false,
        },
    }
}

fn status_icon(state: RecordingState) -> Image<'static> {
    let color = match state {
        RecordingState::Recording | RecordingState::Preparing | RecordingState::Finalizing => {
            [202, 69, 69, 255]
        }
        RecordingState::Paused => [198, 145, 62, 255],
        RecordingState::Interrupted | RecordingState::Error => [164, 61, 61, 255],
        _ => [58, 128, 116, 255],
    };
    let mut pixels = vec![0u8; 16 * 16 * 4];
    for y in 1..15 {
        for x in 1..15 {
            let dx = x as i32 - 7;
            let dy = y as i32 - 7;
            if dx * dx + dy * dy <= 42 {
                let offset = (y * 16 + x) * 4;
                pixels[offset..offset + 4].copy_from_slice(&color);
            }
        }
    }
    Image::new_owned(pixels, 16, 16)
}

pub fn run_app() {
    let paths = AppPaths::discover().expect("无法初始化 Nota 数据目录");
    crate::logging::init(&paths.logs).expect("无法初始化 Nota 技术日志");
    log::info!("Nota starting; local data initialized");
    let storage = Arc::new(Storage::open(paths).expect("无法初始化 Nota 数据库"));
    let recorder = Arc::new(RecordingController::new(Arc::clone(&storage)));
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
            tray_menu: Mutex::new(None),
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
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            list_capture_targets,
            list_audio_devices,
            get_settings,
            save_settings,
            get_recording_snapshot,
            start_recording,
            pause_recording,
            resume_recording,
            stop_recording,
            switch_capture_source,
            set_microphone_enabled,
            list_recordings,
            prepare_recording_playback,
            list_recoverable_recordings,
            recover_recording,
            rename_recording,
            reveal_recording,
            delete_recording,
            delete_recoverable_recording,
            open_microphone_settings,
            copy_consent_template,
            quit_application,
        ])
        .build(tauri::generate_context!())
        .expect("Nota 构建失败");
    application.run(|app, event| {
        if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
            let state = app.state::<AppState>();
            let _ = state.recorder.stop(app);
        }
    });
}

#[cfg(test)]
mod tray_tests {
    use super::*;

    #[test]
    fn tray_actions_match_recording_state() {
        assert_eq!(
            tray_controls(RecordingState::Idle),
            TrayControls {
                toggle_label: "开始录音…",
                toggle_enabled: true,
                stop_enabled: false,
            }
        );
        assert_eq!(
            tray_controls(RecordingState::Recording),
            TrayControls {
                toggle_label: "暂停录音",
                toggle_enabled: true,
                stop_enabled: true,
            }
        );
        assert_eq!(
            tray_controls(RecordingState::Paused),
            TrayControls {
                toggle_label: "继续录音",
                toggle_enabled: true,
                stop_enabled: true,
            }
        );
        assert!(!tray_controls(RecordingState::Finalizing).toggle_enabled);
        assert!(!tray_controls(RecordingState::Completed).stop_enabled);
    }
}
