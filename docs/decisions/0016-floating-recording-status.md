# ADR 0016: Keep Recording Status Visible in a Separate Local Window

- Status: Accepted
- Date: 2026-11-26
- Last updated: 2026-11-26
- Decision owners: Nota desktop maintainers

## Context

A recording can outlive the meeting when its owner forgets to stop it.
Capture-health and silence reminders are heuristic and may not remain
visible. The user requested an understated, persistent waveform card instead
of another change to those detection rules.

## Decision

Keep a separate, taskbar-free Tauri window during an active recording.
Rust owns its lifecycle and work-area placement. Move the main window to
the tray only after successful recording startup and a matching frontend
readiness acknowledgment. A failed card must never make capture controls
inaccessible.

Create and reconcile the card on serialized blocking background tasks.
`run_on_main_thread` executes immediately when called from a synchronous
command on the main thread, so it does not avoid the Windows WebView2
creation deadlock. Keep that thread free for IPC and native window dispatch.

Drive a decorative fine waveform with existing scalar level events, not raw
audio. Keep an authoritative elapsed timer on silence. Reuse recording
commands and retain the card through finalization or error until the saved
recording is confirmed. The surface must remain usable by pointer and
keyboard, including reduced-motion preferences.

## Alternatives Considered

- A tray-only badge is too easy to overlook.
- Automatic meeting-end detection does not give a reliable lifecycle signal
  for every capture source and remains a separate concern.
- Passing PCM to a frontend oscilloscope violates the existing data boundary
  and is unnecessary for an activity indicator.
- Hiding main before the auxiliary frontend is ready can leave users with
  no visible controls after a loading or permission failure.

## Consequences

The second window has a small ongoing WebView rendering cost and needs
native Windows acceptance for focus, DPI, taskbars and monitor changes.
The card has no network access, persisted position or new recording mode.
Its appearance is neutral, but it does not promise exclusion from screen
sharing. Silence and capture-interruption prompts keep their existing rules.
