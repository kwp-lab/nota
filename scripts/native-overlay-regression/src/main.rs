#[allow(dead_code)]
#[path = "../../../src-tauri/src/models.rs"]
mod models;
#[path = "../../../src-tauri/src/recording_overlay.rs"]
mod recording_overlay;

mod logging {
    pub enum FieldKey {
        ErrorCode,
        SessionId,
    }

    pub struct Field;

    impl Field {
        pub fn text(_: FieldKey, _: impl Into<String>) -> Self {
            Self
        }
    }

    pub fn info(component: &str, event: &str, _: &[Field]) {
        eprintln!("{component}: {event}");
    }

    pub fn warn(component: &str, event: &str, fields: &[Field]) {
        info(component, event, fields);
    }
}

use models::{RecordingSnapshot, RecordingState};
use parking_lot::Mutex;
use recording_overlay::{RecordingOverlay, WINDOW_LABEL, reveal_main_window};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl};

struct NativeTestState(Mutex<RecordingSnapshot>);

#[tauri::command]
fn begin_overlay_test(app: AppHandle, state: State<NativeTestState>) {
    let snapshot = state.0.lock().clone();
    app.state::<RecordingOverlay>().begin(&app, &snapshot);
}

#[tauri::command]
fn get_recording_snapshot(state: State<NativeTestState>) -> RecordingSnapshot {
    state.0.lock().clone()
}

#[tauri::command]
fn recording_overlay_ready(app: AppHandle, session_id: String) -> Result<(), String> {
    app.state::<RecordingOverlay>()
        .ready(&app, &session_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn pause_recording(app: AppHandle) {
    update(&app, RecordingState::Paused);
}

#[tauri::command]
fn resume_recording(app: AppHandle) {
    update(&app, RecordingState::Recording);
}

#[tauri::command]
async fn stop_recording(app: AppHandle) {
    update(&app, RecordingState::Finalizing);
}

fn update(app: &AppHandle, state: RecordingState) {
    let snapshot = {
        let holder = app.state::<NativeTestState>();
        let mut snapshot = holder.0.lock();
        snapshot.state = state;
        snapshot.clone()
    };
    app.state::<RecordingOverlay>().sync(app, &snapshot);
    app.emit("recording://snapshot", snapshot).unwrap();
}

fn wait_for(check: impl Fn() -> bool) {
    let started = Instant::now();
    while !check() {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "native overlay condition timed out"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn verify(app: &AppHandle) {
    let main = app.get_webview_window("main").unwrap();
    for session_id in ["native-first-session", "native-second-session"] {
        {
            let holder = app.state::<NativeTestState>();
            let mut snapshot = holder.0.lock();
            snapshot.session_id = Some(session_id.into());
            snapshot.state = RecordingState::Recording;
        }
        main.show().unwrap();
        main.eval("window.__TAURI_INTERNALS__.invoke('begin_overlay_test')")
            .unwrap();
        wait_for(|| {
            app.get_webview_window(WINDOW_LABEL)
                .is_some_and(|window| window.is_visible().unwrap_or(false))
                && !main.is_visible().unwrap()
        });
        let window = app.get_webview_window(WINDOW_LABEL).unwrap();
        window
            .eval("window.__TAURI_INTERNALS__.invoke('pause_recording')")
            .unwrap();
        wait_for(|| app.state::<NativeTestState>().0.lock().state == RecordingState::Paused);
        reveal_main_window(app);
        window
            .eval("window.__TAURI_INTERNALS__.invoke('resume_recording')")
            .unwrap();
        wait_for(|| app.state::<NativeTestState>().0.lock().state == RecordingState::Recording);
        window
            .eval("window.__TAURI_INTERNALS__.invoke('stop_recording')")
            .unwrap();
        wait_for(|| app.state::<NativeTestState>().0.lock().state == RecordingState::Finalizing);
        assert!(main.is_visible().unwrap());
        assert!(window.is_visible().unwrap());
        update(app, RecordingState::Completed);
        wait_for(|| app.get_webview_window(WINDOW_LABEL).is_none());
    }
}

fn main() {
    let finished = Arc::new(AtomicBool::new(false));
    let watchdog_finished = Arc::clone(&finished);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(45));
        if !watchdog_finished.load(Ordering::Acquire) {
            eprintln!("native overlay regression timed out; terminating this test process");
            std::process::exit(1);
        }
    });
    let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
    let mut context = tauri::generate_context!();
    context.config_mut().identifier =
        format!("com.local.nota.overlay-regression-{}", std::process::id());
    context.config_mut().app.windows[0].url =
        WebviewUrl::App("index.html?view=overlay-regression".into());
    context.config_mut().app.windows[0].title = "Nota Overlay Regression".into();
    tauri::Builder::default()
        .manage(RecordingOverlay::default())
        .manage(NativeTestState(Mutex::new(RecordingSnapshot::default())))
        .setup(move |app| {
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(2));
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| verify(&handle)));
                let passed = result.is_ok();
                result_tx.send(passed).unwrap();
                handle.exit(if passed { 0 } else { 1 });
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            begin_overlay_test,
            get_recording_snapshot,
            recording_overlay_ready,
            pause_recording,
            resume_recording,
            stop_recording,
            recording_overlay::resize_recording_overlay,
            recording_overlay::drag_recording_overlay,
            recording_overlay::nudge_recording_overlay,
            recording_overlay::show_recording_main_window,
        ])
        .run(context)
        .expect("native overlay test could not run");
    finished.store(true, Ordering::Release);
    assert!(result_rx.recv_timeout(Duration::from_secs(1)).unwrap());
    println!("PASS synchronous WebView start, pause/resume/stop and two recording sessions");
}
