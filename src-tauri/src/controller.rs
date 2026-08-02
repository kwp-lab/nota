use crate::asr::{
    AsrManager, clean_stale_temporary_chunks, list_models as list_provider_models,
    normalize_base_url, remove_temporary_chunks, test_connection,
};
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
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use uuid::Uuid;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::PCWSTR;

const MAX_PACKETS_PER_TICK: usize = 64;

struct AppState {
    storage: Arc<Storage>,
    recorder: Arc<RecordingController>,
    asr: Arc<AsrManager>,
    tray_state: Mutex<Option<RecordingState>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrayControls {
    start_enabled: bool,
    toggle_label: Option<&'static str>,
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
    capture_selection: CaptureSelection,
    worker: Option<JoinHandle<()>>,
}

struct RecordingController {
    snapshot: Arc<Mutex<RecordingSnapshot>>,
    runtime: Mutex<Option<RecordingRuntime>>,
    finalizer: Mutex<Option<JoinHandle<()>>>,
    storage: Arc<Storage>,
}

impl RecordingController {
    fn new(storage: Arc<Storage>) -> Self {
        Self {
            snapshot: Arc::new(Mutex::new(RecordingSnapshot::default())),
            runtime: Mutex::new(None),
            finalizer: Mutex::new(None),
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

    fn start(&self, app: AppHandle, request: StartRecordingRequest) -> Result<RecordingSnapshot> {
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

        let process_capture = matches!(request.capture, CaptureSelection::Process { .. });
        let (microphone_tx, microphone_rx) = unbounded::<AudioPacket>();
        let (system_tx, system_rx) = unbounded::<AudioPacket>();
        let system_source = capture_source_from_selection(&request.capture)?;
        // Attach process loopback before opening a Bluetooth microphone.
        // Switching a headset from A2DP to HFP can make a meeting client move
        // its render stream to a replacement process/session. Endpoint
        // loopback needs the opposite order so it binds to the post-switch HFP
        // endpoint instead of the now-silent A2DP endpoint.
        let mut system_capture = if process_capture {
            Some(
                match start_capture(system_source.clone(), system_tx.clone()) {
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
                match start_capture(CaptureSource::Microphone(selection), microphone_tx.clone()) {
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
            system_capture = Some(match start_capture(system_source, system_tx.clone()) {
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
            });
        }
        let system_capture = system_capture.expect("capture source was initialized");
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
            capture_selection: request.capture,
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
            let finalizer_app = app.clone();
            let finalizer_snapshot = Arc::clone(&self.snapshot);
            let finalizer = std::thread::Builder::new()
                .name("nota-finalizer".into())
                .spawn(move || {
                    log::info!("waiting for recording worker");
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
                        log::error!("recording worker panicked during finalization");
                    } else {
                        log::info!("recording worker stopped");
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
        runtime.capture_selection = selection.clone();
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
        restart_system_loopback_after_device_mode_change(runtime)?;
        emit_snapshot(app, &self.snapshot);
        Ok(self.snapshot())
    }
}

fn restart_system_loopback_after_device_mode_change(runtime: &mut RecordingRuntime) -> Result<()> {
    if !matches!(runtime.capture_selection, CaptureSelection::System { .. }) {
        return Ok(());
    }
    std::thread::sleep(Duration::from_millis(250));
    let source = capture_source_from_selection(&runtime.capture_selection)?;
    let replacement = start_capture(source, runtime.system_sender.clone())?;
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

    let diagnostics = mixer.diagnostics();
    log::info!(
        "recording worker leaving mix loop; audio diagnostics system_underflows={} microphone_underflows={} system_discontinuities={} microphone_discontinuities={} system_concealed_gap_samples={} microphone_concealed_gap_samples={} system_packets={} microphone_packets={} system_non_silent_packets={} microphone_non_silent_packets={} system_input_rms_db={:.1} microphone_input_rms_db={:.1} system_input_peak_db={:.1} microphone_input_peak_db={:.1}",
        diagnostics.system_underflows,
        diagnostics.microphone_underflows,
        diagnostics.system_discontinuities,
        diagnostics.microphone_discontinuities,
        diagnostics.system_concealed_gap_samples,
        diagnostics.microphone_concealed_gap_samples,
        diagnostics.system_packets,
        diagnostics.microphone_packets,
        diagnostics.system_non_silent_packets,
        diagnostics.microphone_non_silent_packets,
        amplitude_db(diagnostics.system_input_rms),
        amplitude_db(diagnostics.microphone_input_rms),
        amplitude_db(diagnostics.system_input_peak as f64),
        amplitude_db(diagnostics.microphone_input_peak as f64)
    );
    snapshot.lock().state = RecordingState::Finalizing;
    emit_snapshot(&app, &snapshot);
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

fn amplitude_db(value: f64) -> f64 {
    20.0 * value.max(1.0e-12).log10()
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
    schedule_tray_update(app, value.state);
}

fn command_result<T>(result: Result<T>) -> std::result::Result<T, String> {
    result.map_err(|error| format!("{error:#}"))
}

fn stop_in_background(app: &AppHandle, recorder: Arc<RecordingController>) {
    let stop_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(error) = recorder.stop(&stop_app) {
            log::error!("background stop request failed: {error:#}");
            recorder.set_error(&stop_app, "controller", "STOP_FAILED", &error);
        }
    });
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
        self.recorder.is_active() || self.asr.has_active()
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
) -> std::result::Result<(), String> {
    command_result((|| {
        if state.asr.is_active(&id) {
            bail!("该录音正在转写，请先中断任务后再删除");
        }
        let item = state.storage.find_recording(&id)?;
        remove_temporary_chunks(&state.storage.paths().recovery, &id)?;
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
        request.base_url = normalize_base_url(&request.base_url)?;
        state.storage.save_asr_provider(request)
    })())
}

#[tauri::command]
fn delete_asr_provider(state: State<AppState>, id: String) -> std::result::Result<(), String> {
    command_result(state.storage.delete_asr_provider(&id))
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
        request.base_url = normalize_base_url(&request.base_url)?;
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
        request.base_url = normalize_base_url(&request.base_url)?;
        let credentials = storage.asr_probe_credentials(request)?;
        list_provider_models(&credentials)
    })
    .await
    .map_err(|error| format!("读取模型任务异常结束：{error}"))?;
    command_result(result)
}

#[tauri::command]
fn start_transcription(
    app: AppHandle,
    state: State<AppState>,
    recording_id: String,
    provider_id: Option<String>,
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
        state.asr.start(app, &recording_id, &provider_id)
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
                Some(speaker) => format!("{speaker}：{text}"),
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
    let menu = build_tray_menu(app.handle(), RecordingState::Idle)?;
    TrayIconBuilder::with_id("main-tray")
        .icon(status_icon(RecordingState::Idle))
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

fn is_window_reveal_click(button: MouseButton, button_state: MouseButtonState) -> bool {
    button == MouseButton::Left && button_state == MouseButtonState::Up
}

fn build_tray_menu(app: &AppHandle, state: RecordingState) -> Result<Menu<tauri::Wry>> {
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
    Ok(Menu::with_items(
        app,
        &[&show, &toggle, &stop, &separator, &quit],
    )?)
}

fn schedule_tray_update(app: &AppHandle, state: RecordingState) {
    {
        let app_state = app.state::<AppState>();
        let mut previous = app_state.tray_state.lock();
        if *previous == Some(state) {
            return;
        }
        *previous = Some(state);
    }
    let main_thread_app = app.clone();
    if let Err(error) = app.run_on_main_thread(move || {
        update_tray_on_main_thread(&main_thread_app, state);
    }) {
        app.state::<AppState>().tray_state.lock().take();
        log::warn!("unable to schedule tray update: {error}");
    }
}

fn update_tray_on_main_thread(app: &AppHandle, state: RecordingState) {
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
    match build_tray_menu(app, state) {
        Ok(menu) => {
            let _ = tray.set_menu(Some(menu));
        }
        Err(error) => {
            log::warn!("unable to rebuild tray menu: {error:#}");
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
    clean_stale_temporary_chunks(&storage.paths().recovery)
        .expect("无法清理上次遗留的转写临时文件");
    storage
        .interrupt_running_transcriptions()
        .expect("无法恢复上次中断的转写任务状态");
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
            tray_state: Mutex::new(Some(RecordingState::Idle)),
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
            list_asr_providers,
            save_asr_provider,
            delete_asr_provider,
            set_active_asr_provider,
            test_asr_provider,
            list_asr_models,
            start_transcription,
            cancel_transcription,
            resume_transcription,
            get_transcript,
            copy_transcript,
            export_transcript,
            reveal_transcript_export,
            has_active_transcription,
            quit_application,
        ])
        .build(tauri::generate_context!())
        .expect("Nota 构建失败");
    application.run(|app, event| {
        if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
            let state = app.state::<AppState>();
            if let Err(error) = state.recorder.stop_and_wait(app) {
                log::error!("best-effort shutdown finalization failed: {error:#}");
            }
            if let Err(error) = state.asr.interrupt_all() {
                log::error!("best-effort ASR interruption failed: {error:#}");
            }
        }
    });
}

#[cfg(test)]
mod transcript_export_tests {
    use super::*;

    fn transcript(text: &str, segments: Vec<TranscriptSegment>) -> TranscriptDocument {
        TranscriptDocument {
            recording_id: "recording".into(),
            status: TranscriptionStatus::Completed,
            provider_name: "FunASR".into(),
            model_id: "sensevoice".into(),
            language: Some("zh".into()),
            text: text.into(),
            segments,
            completed_chunks: 1,
            total_chunks: 1,
            error_message: None,
            updated_at: "2026-08-02T00:00:00Z".into(),
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
}

#[cfg(test)]
mod tray_tests {
    use super::*;

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
                })
                .unwrap();
        }

        let mut drained = 0;
        assert!(drain_packets(&receiver, |_| drained += 1));
        assert_eq!(drained, MAX_PACKETS_PER_TICK);
        assert_eq!(receiver.len(), 10);
    }
}
