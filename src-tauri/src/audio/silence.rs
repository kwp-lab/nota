use std::time::{Duration, Instant};

pub const SILENCE_REMINDER_DELAY: Duration = Duration::from_secs(3 * 60);
const AUDIBLE_PEAK_THRESHOLD: f32 = 0.001;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SilenceEvent {
    ProlongedSilence,
    AudioResumed,
}

#[derive(Default)]
pub struct RecordingSilenceMonitor {
    silence_since: Option<Instant>,
    alert_emitted: bool,
}

impl RecordingSilenceMonitor {
    pub fn observe(
        &mut self,
        now: Instant,
        system_audible: bool,
        microphone_audible: bool,
    ) -> Option<SilenceEvent> {
        if system_audible || microphone_audible {
            self.silence_since = None;
            return std::mem::replace(&mut self.alert_emitted, false)
                .then_some(SilenceEvent::AudioResumed);
        }
        if self.alert_emitted {
            return None;
        }
        let silence_since = self.silence_since.get_or_insert(now);
        if now.duration_since(*silence_since) < SILENCE_REMINDER_DELAY {
            return None;
        }
        self.alert_emitted = true;
        Some(SilenceEvent::ProlongedSilence)
    }

    pub fn suspend(&mut self) {
        self.silence_since = None;
    }
}

pub fn contains_audible_audio(samples: &[f32]) -> bool {
    samples
        .iter()
        .any(|sample| sample.abs() > AUDIBLE_PEAK_THRESHOLD)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn either_source_prevents_a_reminder_while_speech_continues() {
        for (system_audible, microphone_audible) in [(true, false), (false, true), (true, true)] {
            let started = Instant::now();
            let mut monitor = RecordingSilenceMonitor::default();
            for seconds in 0..600 {
                let now = started + Duration::from_secs(seconds);
                assert_eq!(
                    monitor.observe(now, system_audible, microphone_audible),
                    None
                );
                assert_eq!(
                    monitor.observe(now + Duration::from_millis(10), false, false),
                    None
                );
            }
        }
    }

    #[test]
    fn alternating_speakers_keep_the_recording_active() {
        let started = Instant::now();
        let mut monitor = RecordingSilenceMonitor::default();
        for seconds in 0..600 {
            let system_audible = seconds % 120 < 60;
            assert_eq!(
                monitor.observe(
                    started + Duration::from_secs(seconds),
                    system_audible,
                    !system_audible,
                ),
                None
            );
        }
    }

    #[test]
    fn both_sources_must_remain_silent_for_three_minutes() {
        let started = Instant::now();
        let mut monitor = RecordingSilenceMonitor::default();
        assert_eq!(monitor.observe(started, false, false), None);
        assert_eq!(
            monitor.observe(started + Duration::from_secs(179), false, false),
            None
        );
        assert_eq!(
            monitor.observe(started + SILENCE_REMINDER_DELAY, false, false),
            Some(SilenceEvent::ProlongedSilence)
        );
        assert_eq!(
            monitor.observe(started + Duration::from_secs(600), false, false),
            None
        );
    }

    #[test]
    fn either_source_restarts_the_full_silence_delay() {
        for (system_audible, microphone_audible) in [(true, false), (false, true)] {
            let started = Instant::now();
            let mut monitor = RecordingSilenceMonitor::default();
            assert_eq!(monitor.observe(started, false, false), None);
            assert_eq!(
                monitor.observe(
                    started + Duration::from_secs(179),
                    system_audible,
                    microphone_audible,
                ),
                None
            );
            assert_eq!(
                monitor.observe(started + Duration::from_secs(180), false, false),
                None
            );
            assert_eq!(
                monitor.observe(started + Duration::from_secs(359), false, false),
                None
            );
            assert_eq!(
                monitor.observe(started + Duration::from_secs(360), false, false),
                Some(SilenceEvent::ProlongedSilence)
            );
        }
    }

    #[test]
    fn either_source_dismisses_and_rearms_an_existing_reminder() {
        for (system_audible, microphone_audible) in [(true, false), (false, true)] {
            let started = Instant::now();
            let mut monitor = RecordingSilenceMonitor::default();
            assert_eq!(monitor.observe(started, false, false), None);
            assert_eq!(
                monitor.observe(started + SILENCE_REMINDER_DELAY, false, false),
                Some(SilenceEvent::ProlongedSilence)
            );
            assert_eq!(
                monitor.observe(
                    started + Duration::from_secs(200),
                    system_audible,
                    microphone_audible,
                ),
                Some(SilenceEvent::AudioResumed)
            );
            assert_eq!(
                monitor.observe(
                    started + Duration::from_secs(201),
                    system_audible,
                    microphone_audible,
                ),
                None
            );
            assert_eq!(
                monitor.observe(started + Duration::from_secs(240), false, false),
                None
            );
            assert_eq!(
                monitor.observe(started + Duration::from_secs(420), false, false),
                Some(SilenceEvent::ProlongedSilence)
            );
        }
    }

    #[test]
    fn suspension_restarts_the_timer_without_repeating_an_existing_alert() {
        let started = Instant::now();
        let mut monitor = RecordingSilenceMonitor::default();
        assert_eq!(monitor.observe(started, false, false), None);
        monitor.suspend();
        assert_eq!(
            monitor.observe(started + Duration::from_secs(170), false, false),
            None
        );
        assert_eq!(
            monitor.observe(started + Duration::from_secs(349), false, false),
            None
        );
        assert_eq!(
            monitor.observe(started + Duration::from_secs(350), false, false),
            Some(SilenceEvent::ProlongedSilence)
        );
        monitor.suspend();
        assert_eq!(
            monitor.observe(started + Duration::from_secs(600), false, false),
            None
        );
    }

    #[test]
    fn audible_threshold_ignores_digital_silence_but_accepts_real_signal() {
        assert!(!contains_audible_audio(&[0.0, 0.0005, -0.001]));
        assert!(contains_audible_audio(&[0.0, -0.0011]));
    }
}
