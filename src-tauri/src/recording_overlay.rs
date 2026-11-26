use crate::logging::{self, Field, FieldKey};
use crate::models::{RecordingSnapshot, RecordingState};
use anyhow::{Result, bail};
use parking_lot::Mutex;
use tauri::{
    AppHandle, LogicalSize, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

pub const WINDOW_LABEL: &str = "recording-overlay";
const WIDTH: f64 = 216.0;
const COLLAPSED_HEIGHT: f64 = 56.0;
const EXPANDED_HEIGHT: f64 = 136.0;
const ERROR_HEIGHT: f64 = 208.0;

#[derive(Debug, Clone, PartialEq, Eq)]
struct OverlaySession {
    id: String,
    state: RecordingState,
    has_fault: bool,
    ready: bool,
    hide_main_pending: bool,
}

impl OverlaySession {
    fn should_close(&self) -> bool {
        matches!(self.state, RecordingState::Idle)
            || (self.state == RecordingState::Completed && !self.has_fault)
    }

    fn may_hide_main(&self) -> bool {
        self.ready
            && self.hide_main_pending
            && !self.has_fault
            && matches!(self.state, RecordingState::Recording)
    }
}

#[derive(Default)]
pub struct RecordingOverlay {
    session: Mutex<Option<OverlaySession>>,
    render_gate: Mutex<()>,
}

impl RecordingOverlay {
    pub fn begin(&self, app: &AppHandle, snapshot: &RecordingSnapshot) {
        let Some(id) = snapshot.session_id.clone() else {
            return;
        };
        *self.session.lock() = Some(OverlaySession {
            id,
            state: snapshot.state,
            has_fault: snapshot.fault.is_some(),
            ready: false,
            hide_main_pending: true,
        });
        queue_render(app);
    }

    pub fn sync(&self, app: &AppHandle, snapshot: &RecordingSnapshot) {
        let mut session = self.session.lock();
        let Some(current) = session.as_mut() else {
            return;
        };
        if snapshot.session_id.as_deref() != Some(current.id.as_str()) {
            return;
        }
        if current.state == snapshot.state && current.has_fault == snapshot.fault.is_some() {
            return;
        }
        current.state = snapshot.state;
        current.has_fault = snapshot.fault.is_some();
        drop(session);
        queue_render(app);
    }

    pub fn ready(&self, app: &AppHandle, session_id: &str) -> Result<()> {
        let mut session = self.session.lock();
        let Some(current) = session.as_mut().filter(|session| session.id == session_id) else {
            bail!("该录音状态卡片已经过期");
        };
        current.ready = true;
        logging::info(
            "recording_overlay",
            "frontend_ready",
            &[Field::text(FieldKey::SessionId, current.id.clone())],
        );
        drop(session);
        queue_render(app);
        Ok(())
    }

    fn render(&self, app: &AppHandle) -> Result<()> {
        let Some(session) = self.session.lock().clone() else {
            return Ok(());
        };
        if session.should_close() {
            if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
                window.destroy()?;
                logging::info(
                    "recording_overlay",
                    "closed",
                    &[Field::text(FieldKey::SessionId, session.id.clone())],
                );
            }
            let mut current = self.session.lock();
            if current.as_ref() == Some(&session) {
                current.take();
            }
            return Ok(());
        }
        let window = match app.get_webview_window(WINDOW_LABEL) {
            Some(window) => window,
            None => {
                logging::info(
                    "recording_overlay",
                    "create_requested",
                    &[Field::text(FieldKey::SessionId, session.id.clone())],
                );
                let window = WebviewWindowBuilder::new(
                    app,
                    WINDOW_LABEL,
                    WebviewUrl::App("index.html?view=recording-overlay".into()),
                )
                .title("Nota")
                .inner_size(WIDTH, COLLAPSED_HEIGHT)
                .resizable(false)
                .maximizable(false)
                .minimizable(false)
                .decorations(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .visible(false)
                .focused(false)
                .build()?;
                if let Err(error) = place_window(app, &window, None) {
                    let _ = window.destroy();
                    return Err(error);
                }
                logging::info(
                    "recording_overlay",
                    "created",
                    &[Field::text(FieldKey::SessionId, session.id.clone())],
                );
                window
            }
        };
        if session.ready {
            window.show()?;
        }
        if session.ready && session.hide_main_pending {
            if session.may_hide_main()
                && let Some(main) = app.get_webview_window("main")
                && main.is_visible()?
            {
                main.hide()?;
                logging::info(
                    "recording_overlay",
                    "main_hidden",
                    &[Field::text(FieldKey::SessionId, session.id.clone())],
                );
            }
            if let Some(current) = self
                .session
                .lock()
                .as_mut()
                .filter(|current| current.id == session.id)
            {
                current.hide_main_pending = false;
            }
        }
        Ok(())
    }
}

fn queue_render(app: &AppHandle) {
    let render_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let overlay = render_app.state::<RecordingOverlay>();
        let _gate = overlay.render_gate.lock();
        if overlay.render(&render_app).is_err() {
            log_window_failure("update_failed", "overlay_update_failed");
            reveal_main_window(&render_app);
        }
    });
}

fn log_window_failure(event: &'static str, code: &'static str) {
    logging::warn(
        "recording_overlay",
        event,
        &[Field::text(FieldKey::ErrorCode, code)],
    );
}

pub fn reveal_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn fitted_position(
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    origin: PhysicalPosition<i32>,
    available: PhysicalSize<u32>,
) -> PhysicalPosition<i32> {
    let max_x = i64::from(origin.x) + i64::from(available.width.saturating_sub(size.width));
    let max_y = i64::from(origin.y) + i64::from(available.height.saturating_sub(size.height));
    PhysicalPosition::new(
        i64::from(position.x).clamp(i64::from(origin.x), max_x) as i32,
        i64::from(position.y).clamp(i64::from(origin.y), max_y) as i32,
    )
}

fn place_window(
    app: &AppHandle,
    window: &WebviewWindow,
    anchor: Option<(PhysicalPosition<i32>, PhysicalSize<u32>)>,
) -> Result<()> {
    let monitor = if anchor.is_some() {
        window.current_monitor()?
    } else {
        app.get_webview_window("main")
            .and_then(|main| main.current_monitor().ok().flatten())
    }
    .or(window.primary_monitor()?);
    let Some(monitor) = monitor else {
        bail!("无法确定状态卡片所在的屏幕");
    };
    let area = monitor.work_area();
    let size = window.outer_size()?;
    let margin = (16.0 * monitor.scale_factor()).round() as i32;
    let desired = match anchor {
        Some((position, previous_size)) => PhysicalPosition::new(
            position
                .x
                .saturating_add(previous_size.width as i32)
                .saturating_sub(size.width as i32),
            position
                .y
                .saturating_add(previous_size.height as i32)
                .saturating_sub(size.height as i32),
        ),
        None => PhysicalPosition::new(
            area.position
                .x
                .saturating_add(area.size.width as i32)
                .saturating_sub(size.width as i32)
                .saturating_sub(margin),
            area.position
                .y
                .saturating_add(area.size.height as i32)
                .saturating_sub(size.height as i32)
                .saturating_sub(margin),
        ),
    };
    window.set_position(fitted_position(desired, size, area.position, area.size))?;
    Ok(())
}

#[tauri::command]
pub fn resize_recording_overlay(
    app: AppHandle,
    expanded: bool,
    has_error: bool,
) -> Result<(), String> {
    let main_thread_app = app.clone();
    app.run_on_main_thread(move || {
        let Some(window) = main_thread_app.get_webview_window(WINDOW_LABEL) else {
            return;
        };
        let result = (|| -> Result<()> {
            let anchor = (window.outer_position()?, window.outer_size()?);
            let height = if has_error {
                ERROR_HEIGHT
            } else if expanded {
                EXPANDED_HEIGHT
            } else {
                COLLAPSED_HEIGHT
            };
            window.set_size(LogicalSize::new(WIDTH, height))?;
            place_window(&main_thread_app, &window, Some(anchor))
        })();
        if result.is_err() {
            log_window_failure("resize_failed", "overlay_resize_failed");
        }
    })
    .map_err(|_| "无法调整录音状态卡片".into())
}

#[tauri::command]
pub fn drag_recording_overlay(window: WebviewWindow) -> Result<(), String> {
    if window.label() != WINDOW_LABEL {
        return Err("该操作仅适用于录音状态卡片".into());
    }
    window
        .start_dragging()
        .map_err(|_| "无法移动录音状态卡片".into())
}

#[tauri::command]
pub fn show_recording_main_window(app: AppHandle) {
    reveal_main_window(&app);
}

#[tauri::command]
pub fn nudge_recording_overlay(app: AppHandle, direction: String) -> Result<(), String> {
    let offset = match direction.as_str() {
        "left" => (-16, 0),
        "right" => (16, 0),
        "up" => (0, -16),
        "down" => (0, 16),
        _ => return Err("无效的移动方向".into()),
    };
    let main_thread_app = app.clone();
    app.run_on_main_thread(move || {
        let Some(window) = main_thread_app.get_webview_window(WINDOW_LABEL) else {
            return;
        };
        let result = (|| -> Result<()> {
            let position = window.outer_position()?;
            let scale = window.scale_factor()?;
            let desired = PhysicalPosition::new(
                position
                    .x
                    .saturating_add((f64::from(offset.0) * scale) as i32),
                position
                    .y
                    .saturating_add((f64::from(offset.1) * scale) as i32),
            );
            let size = window.outer_size()?;
            place_window(&main_thread_app, &window, Some((desired, size)))
        })();
        if result.is_err() {
            log_window_failure("move_failed", "overlay_move_failed");
        }
    })
    .map_err(|_| "无法移动录音状态卡片".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(state: RecordingState, has_fault: bool, ready: bool) -> OverlaySession {
        OverlaySession {
            id: "session".into(),
            state,
            has_fault,
            ready,
            hide_main_pending: true,
        }
    }

    #[test]
    fn main_window_waits_for_a_ready_successful_recording() {
        assert!(!session(RecordingState::Recording, false, false).may_hide_main());
        assert!(session(RecordingState::Recording, false, true).may_hide_main());
        for state in [
            RecordingState::Preparing,
            RecordingState::Error,
            RecordingState::Finalizing,
            RecordingState::Completed,
        ] {
            assert!(!session(state, false, true).may_hide_main());
        }
        assert!(!session(RecordingState::Recording, true, true).may_hide_main());
        let mut revealed = session(RecordingState::Recording, false, true);
        revealed.hide_main_pending = false;
        assert!(!revealed.may_hide_main());
    }

    #[test]
    fn overlay_survives_pause_finalization_and_faults_until_safely_saved() {
        for state in [
            RecordingState::Recording,
            RecordingState::Paused,
            RecordingState::Interrupted,
            RecordingState::Finalizing,
            RecordingState::Error,
        ] {
            assert!(!session(state, false, true).should_close());
        }
        assert!(!session(RecordingState::Completed, true, true).should_close());
        assert!(session(RecordingState::Completed, false, true).should_close());
    }

    #[test]
    fn position_respects_negative_monitors_taskbars_and_small_work_areas() {
        let origin = PhysicalPosition::new(-1920, 40);
        let area = PhysicalSize::new(1920, 1000);
        let size = PhysicalSize::new(270, 170);
        assert_eq!(
            fitted_position(PhysicalPosition::new(100, 2000), size, origin, area),
            PhysicalPosition::new(-270, 870)
        );
        assert_eq!(
            fitted_position(PhysicalPosition::new(-4000, -30), size, origin, area),
            origin
        );
        assert_eq!(
            fitted_position(
                PhysicalPosition::new(100, 100),
                size,
                origin,
                PhysicalSize::new(100, 100)
            ),
            origin
        );
    }
}
