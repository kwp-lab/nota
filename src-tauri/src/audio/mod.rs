mod dsp;
mod encoder;
mod recovery;
mod silence;
mod wasapi;

pub use dsp::{AudioMixer, AudioPacket, SAMPLE_RATE};
pub use encoder::OpusOggWriter;
pub use recovery::{list_recoverable_files, move_verified, recover_ogg_file};
pub use silence::{
    RecordingSilenceMonitor, SILENCE_REMINDER_DELAY, SilenceEvent, contains_audible_audio,
};
pub use wasapi::{
    CaptureEvent, CaptureHandle, CaptureSource, list_audio_devices, list_capture_targets,
    start_capture,
};
