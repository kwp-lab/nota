use crate::models::RecordingState;
use anyhow::{Result, bail};

#[derive(Debug, Clone, Copy)]
pub enum RecordingEvent {
    Prepare,
    Started,
    Pause,
    Resume,
    Stop,
    Finalized,
    SourcesChanged { system: bool, microphone: bool },
    Recover,
    Fail,
}

pub fn transition(state: RecordingState, event: RecordingEvent) -> Result<RecordingState> {
    use RecordingEvent::*;
    use RecordingState::*;
    let next = match (state, event) {
        (Idle | Completed | Error, Prepare) => Preparing,
        (Preparing, Started) => Recording,
        (Recording | Interrupted, Pause) => Paused,
        (Paused, Pause) => Paused,
        (Paused, Resume) => Recording,
        (Recording, Resume) => Recording,
        (Preparing | Recording | Paused | Interrupted, Stop) => Finalizing,
        (Finalizing, Stop) => Finalizing,
        (Finalizing | Recovering, Finalized) => Completed,
        (
            _,
            SourcesChanged {
                system: false,
                microphone: false,
            },
        ) => Interrupted,
        (Interrupted, SourcesChanged { system: true, .. })
        | (
            Interrupted,
            SourcesChanged {
                microphone: true, ..
            },
        ) => Recording,
        (current, SourcesChanged { .. }) => current,
        (Idle | Completed | Error, Recover) => Recovering,
        (_, Fail) => Error,
        _ => bail!("非法录音状态转换：{state:?} + {event:?}"),
    };
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_controls_are_idempotent() {
        assert_eq!(
            transition(RecordingState::Paused, RecordingEvent::Pause).unwrap(),
            RecordingState::Paused
        );
        assert_eq!(
            transition(RecordingState::Recording, RecordingEvent::Resume).unwrap(),
            RecordingState::Recording
        );
        assert_eq!(
            transition(RecordingState::Finalizing, RecordingEvent::Stop).unwrap(),
            RecordingState::Finalizing
        );
    }

    #[test]
    fn source_failures_preserve_the_surviving_source() {
        assert_eq!(
            transition(
                RecordingState::Recording,
                RecordingEvent::SourcesChanged {
                    system: false,
                    microphone: true,
                },
            )
            .unwrap(),
            RecordingState::Recording
        );
        assert_eq!(
            transition(
                RecordingState::Recording,
                RecordingEvent::SourcesChanged {
                    system: false,
                    microphone: false,
                },
            )
            .unwrap(),
            RecordingState::Interrupted
        );
    }

    #[test]
    fn supports_recovery_and_safe_finalization() {
        assert_eq!(
            transition(RecordingState::Idle, RecordingEvent::Recover).unwrap(),
            RecordingState::Recovering
        );
        assert_eq!(
            transition(RecordingState::Recording, RecordingEvent::Stop).unwrap(),
            RecordingState::Finalizing
        );
        assert_eq!(
            transition(RecordingState::Finalizing, RecordingEvent::Finalized).unwrap(),
            RecordingState::Completed
        );
    }
}
