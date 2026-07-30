use crate::models::AecMode;
use rubato::{
    Resampler, SincFixedOut, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use sonora::config::EchoCanceller;
use sonora::{AudioProcessing, Config, StreamConfig};
use std::collections::VecDeque;

pub const SAMPLE_RATE: u32 = 48_000;
pub const FRAME_SAMPLES: usize = 480;
const BUFFER_RESERVE_MS: usize = 80;
const MAX_ALIGNMENT_GAP_100NS: u64 = 20_000_000;
const REMOTE_MIX_GAIN: f32 = 0.707_945_76;
const MICROPHONE_MIX_GAIN: f32 = 1.0;
const LIMIT: f32 = 0.891_250_9;
const LIMITER_RELEASE: f32 = 0.05;

#[derive(Debug, Clone)]
pub struct AudioPacket {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub timestamp_100ns: u64,
    pub device_position: Option<u64>,
    pub discontinuity: bool,
}

struct AdaptiveStream {
    input: VecDeque<f32>,
    source_rate: u32,
    format_initialized: bool,
    ratio_adjustment: f64,
    resampler: Option<StreamResampler>,
    started: bool,
    underflowing: bool,
    fade_in: bool,
    last_sample: f32,
    first_timestamp_100ns: Option<u64>,
    next_timestamp_100ns: Option<u64>,
    next_device_position: Option<u64>,
    underflows: u64,
    discontinuities: u64,
    concealed_gap_samples: u64,
    packets: u64,
    non_silent_packets: u64,
    sample_count: u64,
    sample_square_sum: f64,
    peak: f32,
}

struct StreamResampler {
    inner: SincFixedOut<f32>,
    input: Vec<Vec<f32>>,
    output: Vec<Vec<f32>>,
}

impl StreamResampler {
    fn new(source_rate: u32) -> Option<Self> {
        let parameters = SincInterpolationParameters {
            sinc_len: 64,
            f_cutoff: 0.95,
            interpolation: SincInterpolationType::Linear,
            oversampling_factor: 128,
            window: WindowFunction::BlackmanHarris2,
        };
        let inner = match SincFixedOut::new(
            SAMPLE_RATE as f64 / source_rate.max(1) as f64,
            1.01,
            parameters,
            FRAME_SAMPLES,
            1,
        ) {
            Ok(value) => value,
            Err(error) => {
                log::error!("unable to create {source_rate} Hz audio resampler: {error}");
                return None;
            }
        };
        let input = inner.input_buffer_allocate(false);
        let output = inner.output_buffer_allocate(true);
        Some(Self {
            inner,
            input,
            output,
        })
    }

    fn reset(&mut self) {
        self.inner.reset();
        self.input[0].clear();
        self.output[0].fill(0.0);
    }
}

impl AdaptiveStream {
    fn new() -> Self {
        Self {
            input: VecDeque::with_capacity(16_384),
            source_rate: SAMPLE_RATE,
            format_initialized: false,
            ratio_adjustment: 1.0,
            resampler: None,
            started: false,
            underflowing: false,
            fade_in: false,
            last_sample: 0.0,
            first_timestamp_100ns: None,
            next_timestamp_100ns: None,
            next_device_position: None,
            underflows: 0,
            discontinuities: 0,
            concealed_gap_samples: 0,
            packets: 0,
            non_silent_packets: 0,
            sample_count: 0,
            sample_square_sum: 0.0,
            peak: 0.0,
        }
    }

    fn push(&mut self, packet: AudioPacket) {
        self.packets = self.packets.saturating_add(1);
        self.sample_count = self
            .sample_count
            .saturating_add(packet.samples.len() as u64);
        self.sample_square_sum += packet
            .samples
            .iter()
            .map(|sample| (*sample as f64) * (*sample as f64))
            .sum::<f64>();
        self.peak = packet
            .samples
            .iter()
            .map(|sample| sample.abs())
            .fold(self.peak, f32::max);
        if packet.samples.iter().any(|sample| sample.abs() > 1.0e-6) {
            self.non_silent_packets = self.non_silent_packets.saturating_add(1);
        }
        let packet_rate = packet.sample_rate.max(1);
        if !self.format_initialized || packet_rate != self.source_rate {
            self.input.clear();
            self.source_rate = packet_rate;
            self.format_initialized = true;
            self.ratio_adjustment = 1.0;
            self.resampler = if packet_rate == SAMPLE_RATE {
                None
            } else {
                StreamResampler::new(packet_rate)
            };
            self.started = false;
            self.underflowing = false;
            self.fade_in = false;
            self.last_sample = 0.0;
            self.first_timestamp_100ns = None;
            self.next_timestamp_100ns = None;
            self.next_device_position = None;
        }
        let mut samples = packet.samples;
        if self.underflowing {
            self.smooth_packet_boundary(&mut samples);
        }
        let original_sample_count = samples.len();
        self.first_timestamp_100ns
            .get_or_insert(packet.timestamp_100ns);
        if packet.discontinuity {
            self.discontinuities += 1;
        }
        let device_delta = packet.device_position.and_then(|position| {
            self.next_device_position
                .map(|expected| position as i128 - expected as i128)
        });
        let timestamp_delta = self
            .next_timestamp_100ns
            .map(|expected| packet.timestamp_100ns as i128 - expected as i128);
        let delta_samples = if let Some(delta) = device_delta {
            // A restarted WASAPI client begins a new device-position timeline.
            // Treat large backward jumps as a new origin rather than deleting
            // every packet from the rebuilt stream.
            if delta.unsigned_abs() > (self.source_rate as u128 * 5) {
                None
            } else {
                Some(delta)
            }
        } else if let Some(delta) = timestamp_delta {
            if (delta > 20_000 && delta < 50_000_000) || delta < -20_000 {
                Some(delta * self.source_rate as i128 / 10_000_000)
            } else {
                None
            }
        } else {
            None
        };
        if let Some(delta) = delta_samples {
            if delta > 0 {
                let missing = (delta as u128)
                    .min((self.source_rate * 5) as u128)
                    .min(usize::MAX as u128) as usize;
                self.conceal_gap(&mut samples, missing);
            } else if delta < 0 {
                let overlap = (-delta) as usize;
                if overlap >= samples.len() {
                    self.next_device_position = packet
                        .device_position
                        .map(|position| position.saturating_add(original_sample_count as u64));
                    return;
                }
                samples.drain(..overlap);
                self.smooth_packet_boundary(&mut samples);
            } else if packet.discontinuity {
                self.smooth_packet_boundary(&mut samples);
            }
        } else if packet.discontinuity {
            self.smooth_packet_boundary(&mut samples);
        }
        if packet.device_position.is_none()
            && let Some(expected) = self.next_timestamp_100ns
        {
            let delta = packet.timestamp_100ns as i128 - expected as i128;
            if delta >= 50_000_000 {
                self.next_timestamp_100ns = None;
            }
        }
        let duration = original_sample_count as u64 * 10_000_000 / self.source_rate.max(1) as u64;
        self.next_timestamp_100ns = Some(packet.timestamp_100ns.saturating_add(duration));
        self.next_device_position = packet
            .device_position
            .map(|position| position.saturating_add(original_sample_count as u64));
        self.input.extend(samples);
        let target = (self.source_rate as usize / 10).max(1) * 3;
        let error = self.input.len() as isize - target as isize;
        self.ratio_adjustment =
            (1.0 + (error as f64 / target as f64) * 0.000_25).clamp(0.999, 1.001);
    }

    fn conceal_gap(&mut self, samples: &mut [f32], missing: usize) {
        if missing == 0 {
            return;
        }
        self.concealed_gap_samples = self.concealed_gap_samples.saturating_add(missing as u64);
        let short_gap = (self.source_rate as usize / 1_000).max(1);
        let previous = self.input.back().copied().unwrap_or(0.0);
        let next = samples.first().copied().unwrap_or(0.0);
        if missing <= short_gap {
            for index in 0..missing {
                let mix = (index + 1) as f32 / (missing + 1) as f32;
                self.input.push_back(previous + (next - previous) * mix);
            }
            return;
        }

        let fade = (self.source_rate as usize / 500)
            .max(1)
            .min(missing / 2)
            .min(self.input.len())
            .min(samples.len());
        let input_len = self.input.len();
        for (index, sample) in self
            .input
            .iter_mut()
            .skip(input_len.saturating_sub(fade))
            .enumerate()
        {
            *sample *= 1.0 - (index + 1) as f32 / fade.max(1) as f32;
        }
        self.input.extend(std::iter::repeat_n(0.0, missing));
        for (index, sample) in samples.iter_mut().take(fade).enumerate() {
            *sample *= (index + 1) as f32 / fade.max(1) as f32;
        }
    }

    fn smooth_packet_boundary(&self, samples: &mut [f32]) {
        let previous = self.input.back().copied().unwrap_or(0.0);
        let fade = (self.source_rate as usize / 500).max(1).min(samples.len());
        for (index, sample) in samples.iter_mut().take(fade).enumerate() {
            let mix = (index + 1) as f32 / fade as f32;
            *sample = previous * (1.0 - mix) + *sample * mix;
        }
    }

    fn first_timestamp_100ns(&self) -> Option<u64> {
        self.first_timestamp_100ns
    }

    fn prepend_silence_to(&mut self, origin_100ns: u64) {
        let Some(first) = self.first_timestamp_100ns else {
            return;
        };
        let gap = first.saturating_sub(origin_100ns);
        if gap == 0 {
            return;
        }
        if gap > MAX_ALIGNMENT_GAP_100NS {
            log::warn!(
                "capture stream start differs by {:.0} ms; skipping excessive alignment padding",
                gap as f64 / 10_000.0
            );
            return;
        }
        let silence = ((gap as u128 * self.source_rate as u128) / 10_000_000)
            .min(usize::MAX as u128) as usize;
        for _ in 0..silence {
            self.input.push_front(0.0);
        }
    }

    fn frame(&mut self) -> Vec<f32> {
        if self.source_rate == SAMPLE_RATE {
            return self.native_rate_frame();
        }
        let Some(resampler) = self.resampler.as_mut() else {
            return vec![0.0; FRAME_SAMPLES];
        };
        // For a fixed output size, lowering the output/input ratio consumes
        // slightly more source samples and drains an over-full jitter buffer.
        let relative_ratio = (2.0 - self.ratio_adjustment).clamp(0.999, 1.001);
        if let Err(error) = resampler
            .inner
            .set_resample_ratio_relative(relative_ratio, true)
        {
            log::warn!("unable to adjust audio resampling ratio: {error}");
        }
        let needed = resampler.inner.input_frames_next();
        let reserve = self.source_rate as usize * BUFFER_RESERVE_MS / 1_000;
        if !self.started {
            if self.input.len() < reserve.saturating_add(needed) {
                return vec![0.0; FRAME_SAMPLES];
            }
            self.started = true;
            self.fade_in = true;
        } else if self.input.len() < needed {
            if !self.underflowing {
                self.underflows += 1;
                if self.underflows.is_power_of_two() {
                    log::warn!(
                        "audio jitter buffer underrun episode={} rate={}Hz buffered={} needed={}",
                        self.underflows,
                        self.source_rate,
                        self.input.len(),
                        needed
                    );
                }
            }
            self.underflowing = true;
            let available = self.input.len();
            let fade = (self.source_rate as usize / 500).max(1).min(available);
            for (index, sample) in self
                .input
                .iter_mut()
                .skip(available.saturating_sub(fade))
                .enumerate()
            {
                *sample *= 1.0 - (index + 1) as f32 / fade.max(1) as f32;
            }
            self.input
                .extend(std::iter::repeat_n(0.0, needed - available));
        } else {
            self.underflowing = false;
        }

        resampler.input[0].clear();
        resampler.input[0].extend((0..needed).filter_map(|_| self.input.pop_front()));
        let output =
            match resampler
                .inner
                .process_into_buffer(&resampler.input, &mut resampler.output, None)
            {
                Ok((_, written)) => {
                    let mut frame = resampler.output[0][..written].to_vec();
                    frame.resize(FRAME_SAMPLES, 0.0);
                    frame
                }
                Err(error) => {
                    self.underflows += 1;
                    self.started = false;
                    self.fade_in = true;
                    resampler.reset();
                    log::warn!("audio resampling failed; rebuilding jitter buffer: {error}");
                    return self.conceal_underflow();
                }
            };
        let mut output = output;
        if self.fade_in {
            let fade_samples = (SAMPLE_RATE as usize / 200).min(output.len());
            for (index, sample) in output.iter_mut().take(fade_samples).enumerate() {
                *sample *= index as f32 / fade_samples.max(1) as f32;
            }
            self.fade_in = false;
        }
        self.last_sample = output.last().copied().unwrap_or(0.0);
        output
    }

    fn native_rate_frame(&mut self) -> Vec<f32> {
        let reserve = SAMPLE_RATE as usize * BUFFER_RESERVE_MS / 1_000;
        if !self.started {
            if self.input.len() < reserve.saturating_add(FRAME_SAMPLES) {
                return vec![0.0; FRAME_SAMPLES];
            }
            self.started = true;
            self.fade_in = true;
        }
        let target = SAMPLE_RATE as usize * 300 / 1_000;
        let consume = if self.input.len() > target.saturating_add(FRAME_SAMPLES) {
            FRAME_SAMPLES + 1
        } else if self.input.len().saturating_add(FRAME_SAMPLES) < target {
            FRAME_SAMPLES - 1
        } else {
            FRAME_SAMPLES
        };
        if self.input.len() < consume {
            if !self.underflowing {
                self.underflows += 1;
                if self.underflows.is_power_of_two() {
                    log::warn!(
                        "native-rate jitter buffer underrun episode={} buffered={} needed={}",
                        self.underflows,
                        self.input.len(),
                        consume
                    );
                }
            }
            self.underflowing = true;
            let available = self.input.len();
            let fade = (SAMPLE_RATE as usize / 500).max(1).min(available);
            for (index, sample) in self
                .input
                .iter_mut()
                .skip(available.saturating_sub(fade))
                .enumerate()
            {
                *sample *= 1.0 - (index + 1) as f32 / fade.max(1) as f32;
            }
            self.input
                .extend(std::iter::repeat_n(0.0, consume - available));
        } else {
            self.underflowing = false;
        }
        let input = self.input.drain(..consume).collect::<Vec<_>>();
        let mut output = if consume == FRAME_SAMPLES {
            input
        } else {
            (0..FRAME_SAMPLES)
                .map(|index| {
                    let position = index as f64 * (consume - 1) as f64 / (FRAME_SAMPLES - 1) as f64;
                    let first = position.floor() as usize;
                    let second = (first + 1).min(consume - 1);
                    let fraction = (position - first as f64) as f32;
                    input[first] + (input[second] - input[first]) * fraction
                })
                .collect()
        };
        if self.fade_in {
            let fade_samples = (SAMPLE_RATE as usize / 200).min(output.len());
            for (index, sample) in output.iter_mut().take(fade_samples).enumerate() {
                *sample *= index as f32 / fade_samples.max(1) as f32;
            }
            self.fade_in = false;
        }
        self.last_sample = output.last().copied().unwrap_or(0.0);
        output
    }

    fn conceal_underflow(&mut self) -> Vec<f32> {
        let mut output = vec![0.0; FRAME_SAMPLES];
        let fade_samples = (SAMPLE_RATE as usize / 500).min(FRAME_SAMPLES);
        for (index, sample) in output.iter_mut().take(fade_samples).enumerate() {
            *sample = self.last_sample * (1.0 - index as f32 / fade_samples.max(1) as f32);
        }
        self.last_sample = 0.0;
        output
    }

    fn clear(&mut self) {
        self.input.clear();
        if let Some(resampler) = self.resampler.as_mut() {
            resampler.reset();
        }
        self.started = false;
        self.underflowing = false;
        self.fade_in = false;
        self.last_sample = 0.0;
        self.first_timestamp_100ns = None;
        self.next_timestamp_100ns = None;
        self.next_device_position = None;
    }
}

pub struct AudioMixer {
    system: AdaptiveStream,
    microphone: AdaptiveStream,
    aec: Option<AudioProcessing>,
    aligned: bool,
    limiter: PeakLimiter,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioDiagnostics {
    pub system_underflows: u64,
    pub microphone_underflows: u64,
    pub system_discontinuities: u64,
    pub microphone_discontinuities: u64,
    pub system_concealed_gap_samples: u64,
    pub microphone_concealed_gap_samples: u64,
    pub system_packets: u64,
    pub microphone_packets: u64,
    pub system_non_silent_packets: u64,
    pub microphone_non_silent_packets: u64,
    pub system_input_rms: f64,
    pub microphone_input_rms: f64,
    pub system_input_peak: f32,
    pub microphone_input_peak: f32,
}

struct PeakLimiter {
    gain: f32,
}

impl PeakLimiter {
    fn new() -> Self {
        Self { gain: 1.0 }
    }

    fn process(&mut self, samples: &mut [f32]) {
        let peak = samples
            .iter()
            .map(|sample| sample.abs())
            .fold(0.0f32, f32::max);
        let required = if peak > LIMIT { LIMIT / peak } else { 1.0 };
        if required < self.gain {
            self.gain = required;
        } else {
            self.gain += (required - self.gain) * LIMITER_RELEASE;
        }
        for sample in samples {
            *sample = (*sample * self.gain).clamp(-LIMIT, LIMIT);
        }
    }

    fn reset(&mut self) {
        self.gain = 1.0;
    }
}

impl AudioMixer {
    pub fn new(aec_mode: AecMode, auto_should_enable: bool) -> Self {
        let enabled = matches!(aec_mode, AecMode::On)
            || (matches!(aec_mode, AecMode::Auto) && auto_should_enable);
        Self {
            system: AdaptiveStream::new(),
            microphone: AdaptiveStream::new(),
            aec: enabled.then(create_aec),
            aligned: false,
            limiter: PeakLimiter::new(),
        }
    }

    pub fn push_system(&mut self, packet: AudioPacket) {
        self.system.push(packet);
    }

    pub fn push_microphone(&mut self, packet: AudioPacket) {
        self.microphone.push(packet);
    }

    pub fn next_frame(
        &mut self,
        system_present: bool,
        microphone_present: bool,
    ) -> (Vec<f32>, f32, f32) {
        if !self.aligned && system_present && microphone_present {
            if let (Some(system_start), Some(microphone_start)) = (
                self.system.first_timestamp_100ns(),
                self.microphone.first_timestamp_100ns(),
            ) {
                let origin = system_start.min(microphone_start);
                self.system.prepend_silence_to(origin);
                self.microphone.prepend_silence_to(origin);
                self.aligned = true;
            } else {
                return (vec![0.0; FRAME_SAMPLES], 0.0, 0.0);
            }
        } else if !system_present || !microphone_present {
            self.aligned = true;
        }
        let system = self.system.frame();
        let microphone = self.microphone.frame();
        let system_level = rms(&system);
        let microphone_level = rms(&microphone);

        let processed_microphone = if let Some(processor) = self.aec.as_mut() {
            let mut render_output = vec![0.0f32; FRAME_SAMPLES];
            let mut capture_output = vec![0.0f32; FRAME_SAMPLES];
            if processor
                .process_render_f32(&[&system], &mut [&mut render_output])
                .is_ok()
                && processor
                    .process_capture_f32(&[&microphone], &mut [&mut capture_output])
                    .is_ok()
            {
                capture_output
            } else {
                microphone.clone()
            }
        } else {
            microphone.clone()
        };

        let mut mixed = Vec::with_capacity(FRAME_SAMPLES);
        for index in 0..FRAME_SAMPLES {
            let sample = match (system_present, microphone_present) {
                (true, true) => {
                    system[index] * REMOTE_MIX_GAIN
                        + processed_microphone[index] * MICROPHONE_MIX_GAIN
                }
                (true, false) => system[index],
                (false, true) => processed_microphone[index],
                (false, false) => 0.0,
            };
            mixed.push(sample);
        }
        self.limiter.process(&mut mixed);
        (mixed, system_level, microphone_level)
    }

    pub fn diagnostics(&self) -> AudioDiagnostics {
        AudioDiagnostics {
            system_underflows: self.system.underflows,
            microphone_underflows: self.microphone.underflows,
            system_discontinuities: self.system.discontinuities,
            microphone_discontinuities: self.microphone.discontinuities,
            system_concealed_gap_samples: self.system.concealed_gap_samples,
            microphone_concealed_gap_samples: self.microphone.concealed_gap_samples,
            system_packets: self.system.packets,
            microphone_packets: self.microphone.packets,
            system_non_silent_packets: self.system.non_silent_packets,
            microphone_non_silent_packets: self.microphone.non_silent_packets,
            system_input_rms: stream_input_rms(&self.system),
            microphone_input_rms: stream_input_rms(&self.microphone),
            system_input_peak: self.system.peak,
            microphone_input_peak: self.microphone.peak,
        }
    }

    pub fn reset(&mut self, aec_mode: AecMode, auto_should_enable: bool) {
        self.system.clear();
        self.microphone.clear();
        self.aligned = false;
        self.limiter.reset();
        let enabled = matches!(aec_mode, AecMode::On)
            || (matches!(aec_mode, AecMode::Auto) && auto_should_enable);
        self.aec = enabled.then(create_aec);
    }
}

fn create_aec() -> AudioProcessing {
    let stream = StreamConfig::new(SAMPLE_RATE, 1);
    AudioProcessing::builder()
        .config(Config {
            echo_canceller: Some(EchoCanceller::default()),
            ..Default::default()
        })
        .capture_config(stream)
        .render_config(stream)
        .build()
}

fn stream_input_rms(stream: &AdaptiveStream) -> f64 {
    (stream.sample_square_sum / stream.sample_count.max(1) as f64).sqrt()
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32)
        .sqrt()
        .clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiter_never_clips() {
        let mut limiter = PeakLimiter::new();
        let mut values = vec![-20.0, -2.0, -1.0, 0.0, 1.0, 2.0, 20.0];
        limiter.process(&mut values);
        assert!(values.iter().all(|value| value.abs() <= LIMIT));
    }

    #[test]
    fn resampler_handles_clock_drift_without_unbounded_buffering() {
        let mut stream = AdaptiveStream::new();
        for packet in 0..1000 {
            stream.push(AudioPacket {
                samples: vec![0.25; 481],
                sample_rate: SAMPLE_RATE,
                timestamp_100ns: packet * 100_000,
                device_position: Some(packet * 481),
                discontinuity: false,
            });
            let output = stream.frame();
            assert_eq!(output.len(), FRAME_SAMPLES);
        }
        assert!(stream.input.len() < 10_000);
        assert_eq!(stream.underflows, 0);
    }

    #[test]
    fn native_rate_audio_bypasses_resampling_and_preserves_signal() {
        let mut stream = AdaptiveStream::new();
        stream.push(AudioPacket {
            samples: vec![0.25; 4_800],
            sample_rate: SAMPLE_RATE,
            timestamp_100ns: 1_000_000,
            device_position: Some(0),
            discontinuity: false,
        });
        assert!(stream.resampler.is_none());
        let output = stream.frame();
        assert_eq!(output.len(), FRAME_SAMPLES);
        assert!(
            output
                .iter()
                .skip(240)
                .all(|sample| (*sample - 0.25).abs() < 1.0e-6)
        );
    }

    #[test]
    fn bluetooth_rate_waits_for_jitter_buffer_before_starting() {
        let mut stream = AdaptiveStream::new();
        for packet in 0..8 {
            stream.push(AudioPacket {
                samples: vec![0.25; 160],
                sample_rate: 16_000,
                timestamp_100ns: 1_000_000 + packet * 100_000,
                device_position: Some(packet * 160),
                discontinuity: false,
            });
        }
        assert!(stream.frame().iter().all(|sample| *sample == 0.0));
        stream.push(AudioPacket {
            samples: vec![0.25; 160],
            sample_rate: 16_000,
            timestamp_100ns: 1_800_000,
            device_position: Some(8 * 160),
            discontinuity: false,
        });
        stream.push(AudioPacket {
            samples: vec![0.25; 160],
            sample_rate: 16_000,
            timestamp_100ns: 1_900_000,
            device_position: Some(9 * 160),
            discontinuity: false,
        });
        assert!(stream.frame().iter().any(|sample| sample.abs() > 0.01));
    }

    #[test]
    fn underrun_conceals_in_place_without_periodic_rebuffering() {
        let mut stream = AdaptiveStream::new();
        for packet in 0..12 {
            stream.push(AudioPacket {
                samples: vec![0.25; 160],
                sample_rate: 16_000,
                timestamp_100ns: 1_000_000 + packet * 100_000,
                device_position: Some(packet * 160),
                discontinuity: false,
            });
        }
        let _ = stream.frame();
        for _ in 0..20 {
            let _ = stream.frame();
        }
        assert_eq!(stream.underflows, 1);
        assert!(stream.started);
        assert!(stream.underflowing);
        stream.push(AudioPacket {
            samples: vec![0.25; 160],
            sample_rate: 16_000,
            timestamp_100ns: 2_200_000,
            device_position: Some(12 * 160),
            discontinuity: false,
        });
        assert!(stream.input.iter().any(|sample| sample.abs() > 0.01));
    }

    #[test]
    fn aligns_later_stream_with_leading_silence() {
        let mut stream = AdaptiveStream::new();
        stream.push(AudioPacket {
            samples: vec![0.25; 1600],
            sample_rate: 16_000,
            timestamp_100ns: 2_000_000,
            device_position: Some(0),
            discontinuity: false,
        });
        stream.prepend_silence_to(1_000_000);
        assert_eq!(stream.input.len(), 3_200);
        assert!(stream.input.iter().take(1_600).all(|sample| *sample == 0.0));
    }

    #[test]
    fn device_position_gap_is_faded_instead_of_hard_zero_spliced() {
        let mut stream = AdaptiveStream::new();
        stream.push(AudioPacket {
            samples: vec![1.0; 160],
            sample_rate: 16_000,
            timestamp_100ns: 1_000_000,
            device_position: Some(0),
            discontinuity: false,
        });
        stream.push(AudioPacket {
            samples: vec![-1.0; 160],
            sample_rate: 16_000,
            timestamp_100ns: 1_125_000,
            device_position: Some(200),
            discontinuity: true,
        });
        assert_eq!(stream.concealed_gap_samples, 40);
        assert_eq!(stream.discontinuities, 1);
        let samples = stream.input.iter().copied().collect::<Vec<_>>();
        assert!(
            samples
                .windows(2)
                .all(|pair| (pair[1] - pair[0]).abs() < 0.1)
        );
    }

    #[test]
    fn qpc_gap_inserts_silence_but_sleep_gap_does_not() {
        let mut stream = AdaptiveStream::new();
        stream.push(AudioPacket {
            samples: vec![1.0; 480],
            sample_rate: SAMPLE_RATE,
            timestamp_100ns: 1_000_000,
            device_position: None,
            discontinuity: false,
        });
        let before_gap = stream.input.len();
        stream.push(AudioPacket {
            samples: vec![1.0; 480],
            sample_rate: SAMPLE_RATE,
            timestamp_100ns: 1_200_000,
            device_position: None,
            discontinuity: false,
        });
        assert!(stream.input.len() > before_gap + 480);

        let mut after_sleep = AdaptiveStream::new();
        after_sleep.push(AudioPacket {
            samples: vec![1.0; 480],
            sample_rate: SAMPLE_RATE,
            timestamp_100ns: 1_000_000,
            device_position: None,
            discontinuity: false,
        });
        after_sleep.push(AudioPacket {
            samples: vec![1.0; 480],
            sample_rate: SAMPLE_RATE,
            timestamp_100ns: 101_000_000,
            device_position: None,
            discontinuity: false,
        });
        assert_eq!(after_sleep.input.len(), 960);
    }
}
