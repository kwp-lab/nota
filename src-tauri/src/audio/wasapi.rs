use crate::models::{AudioDevice, CaptureTarget, DeviceDirection, DeviceSelection};
use anyhow::{Context, Result, anyhow, bail};
use crossbeam_channel::Sender;
use std::collections::{HashMap, HashSet};
use std::mem::{ManuallyDrop, size_of};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::{
    CloseHandle, HANDLE, HWND, LPARAM, RPC_E_CHANGED_MODE, WAIT_OBJECT_0,
};
use windows::Win32::Media::Audio::*;
use windows::Win32::Media::KernelStreaming::{KSDATAFORMAT_SUBTYPE_PCM, WAVE_FORMAT_EXTENSIBLE};
use windows::Win32::Media::Multimedia::{KSDATAFORMAT_SUBTYPE_IEEE_FLOAT, WAVE_FORMAT_IEEE_FLOAT};
use windows::Win32::System::Com::StructuredStorage::{
    PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0, PropVariantClear,
    PropVariantToStringAlloc,
};
use windows::Win32::System::Com::{
    BLOB, CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize, STGM_READ,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW, CreateEventW, OpenProcess,
    PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW, SetEvent, WaitForSingleObject,
};
use windows::Win32::System::Variant::VT_BLOB;
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
};
use windows::core::{BOOL, Interface, PCWSTR, PWSTR, implement};

use super::AudioPacket;

const CAPTURE_WAIT_MS: u32 = 100;
const ACTIVATION_WAIT_MS: u32 = 5_000;

#[derive(Debug, Clone)]
pub enum CaptureSource {
    Process {
        process_id: u32,
        executable_path: String,
    },
    System(DeviceSelection),
    Microphone(DeviceSelection),
}

pub struct CaptureHandle {
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    healthy: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl CaptureHandle {
    pub fn pause(&self) {
        self.paused.store(true, Ordering::Release);
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::Release);
    }

    pub fn health_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.healthy)
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for CaptureHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

pub fn start_capture(source: CaptureSource, packets: Sender<AudioPacket>) -> Result<CaptureHandle> {
    let stop = Arc::new(AtomicBool::new(false));
    let paused = Arc::new(AtomicBool::new(false));
    let healthy = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let thread_paused = Arc::clone(&paused);
    let thread_healthy = Arc::clone(&healthy);
    let (ready_tx, ready_rx) = crossbeam_channel::bounded::<std::result::Result<(), String>>(1);
    let join = thread::Builder::new()
        .name("nota-wasapi".into())
        .spawn(move || {
            let result = capture_thread(
                source,
                packets,
                thread_stop,
                thread_paused,
                thread_healthy,
                &ready_tx,
            );
            if let Err(error) = result {
                let _ = ready_tx.try_send(Err(format!("{error:#}")));
            }
        })?;

    match ready_rx.recv_timeout(Duration::from_secs(7)) {
        Ok(Ok(())) => Ok(CaptureHandle {
            stop,
            paused,
            healthy,
            join: Some(join),
        }),
        Ok(Err(error)) => {
            stop.store(true, Ordering::Release);
            let _ = join.join();
            bail!(error)
        }
        Err(_) => {
            stop.store(true, Ordering::Release);
            let _ = join.join();
            bail!("音频设备启动超时")
        }
    }
}

fn capture_thread(
    source: CaptureSource,
    packets: Sender<AudioPacket>,
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    healthy: Arc<AtomicBool>,
    ready: &Sender<std::result::Result<(), String>>,
) -> Result<()> {
    let _com = initialize_com()?;
    let _mmcss = MmcssGuard::new();
    let device_changed = Arc::new(AtomicBool::new(false));

    let mut first_attempt = true;
    while !stop.load(Ordering::Acquire) {
        let setup_result = setup_source(&source);
        let setup = match setup_result {
            Ok(setup) => setup,
            Err(error) if first_attempt => {
                let _ = ready.send(Err(format!("{error:#}")));
                return Ok(());
            }
            Err(_) => {
                healthy.store(false, Ordering::Release);
                thread::sleep(Duration::from_secs(1));
                continue;
            }
        };
        let notification_registration = notification_filter(&source, setup.endpoint_id.as_deref())
            .map(|filter| DeviceNotificationRegistration::new(Arc::clone(&device_changed), filter))
            .transpose()?;
        if let Err(error) = unsafe { setup.client.Start() } {
            if first_attempt {
                let _ = ready.send(Err(format!("{error:#}")));
                return Ok(());
            }
            healthy.store(false, Ordering::Release);
            thread::sleep(Duration::from_secs(1));
            continue;
        }
        healthy.store(true, Ordering::Release);
        if first_attempt {
            let _ = ready.send(Ok(()));
            first_attempt = false;
        }
        device_changed.store(false, Ordering::Release);
        if let Err(error) =
            capture_session(&setup, &source, &packets, &stop, &paused, &device_changed)
        {
            log::warn!(
                "{} capture session is being rebuilt: {error:#}",
                capture_source_label(&source)
            );
        }
        healthy.store(false, Ordering::Release);
        unsafe {
            let _ = setup.client.Stop();
        }
        if stop.load(Ordering::Acquire) {
            break;
        }
        thread::sleep(Duration::from_secs(1));
        drop(notification_registration);
    }
    Ok(())
}

fn capture_source_label(source: &CaptureSource) -> &'static str {
    match source {
        CaptureSource::Process { .. } => "process loopback",
        CaptureSource::System(_) => "system loopback",
        CaptureSource::Microphone(_) => "microphone",
    }
}

fn setup_source(source: &CaptureSource) -> Result<CaptureSetup> {
    match source {
        CaptureSource::Process {
            process_id,
            executable_path,
        } => {
            let pid = match process_path(*process_id) {
                Ok(path) if path.eq_ignore_ascii_case(executable_path) => *process_id,
                _ => list_capture_targets()?
                    .into_iter()
                    .find(|target| target.executable_path.eq_ignore_ascii_case(executable_path))
                    .map(|target| target.process_id)
                    .context("目标应用尚未重新出现")?,
            };
            let root_pid = root_process_with_same_executable(pid, executable_path).unwrap_or(pid);
            log::info!(
                "process loopback target selected_pid={pid} root_pid={root_pid} executable={}",
                executable_path
            );
            setup_process_loopback(root_pid)
        }
        CaptureSource::System(selection) => setup_endpoint(selection.clone(), eRender, true),
        CaptureSource::Microphone(selection) => setup_endpoint(selection.clone(), eCapture, false),
    }
}

fn capture_session(
    setup: &CaptureSetup,
    source: &CaptureSource,
    packets: &Sender<AudioPacket>,
    stop: &AtomicBool,
    paused: &AtomicBool,
    device_changed: &AtomicBool,
) -> Result<()> {
    let mut running = true;
    let mut last_default_check = std::time::Instant::now();
    let mut next_packet_timestamp_100ns = None;
    let mut discontinuities = 0u64;
    let mut timestamp_errors = 0u64;
    while !stop.load(Ordering::Acquire) {
        if device_changed.swap(false, Ordering::AcqRel) {
            bail!("Windows 报告音频设备配置已改变");
        }
        if paused.load(Ordering::Acquire) {
            if running {
                unsafe {
                    let _ = setup.client.Stop();
                    let _ = setup.client.Reset();
                }
                running = false;
            }
            thread::sleep(Duration::from_millis(20));
            continue;
        }
        if !running {
            unsafe { setup.client.Start()? };
            running = true;
        }

        let wait = unsafe { WaitForSingleObject(setup.event, CAPTURE_WAIT_MS) };
        if wait != WAIT_OBJECT_0 {
            if last_default_check.elapsed() >= Duration::from_secs(1) {
                if default_device_changed(source, setup.endpoint_id.as_deref())? {
                    bail!("默认音频设备已改变");
                }
                if process_target_changed(source, setup.process_id)? {
                    bail!("目标应用进程已退出或被替换");
                }
                last_default_check = std::time::Instant::now();
            }
            continue;
        }
        loop {
            let available = unsafe { setup.capture.GetNextPacketSize()? };
            if available == 0 {
                break;
            }
            let mut data = std::ptr::null_mut();
            let mut frames = 0u32;
            let mut flags = 0u32;
            let mut device_position = 0u64;
            let mut qpc = 0u64;
            unsafe {
                setup.capture.GetBuffer(
                    &mut data,
                    &mut frames,
                    &mut flags,
                    Some(&mut device_position),
                    Some(&mut qpc),
                )?;
            }
            let discontinuity = flags & AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY.0 as u32 != 0;
            if discontinuity {
                discontinuities += 1;
                if discontinuities.is_power_of_two() {
                    log::warn!(
                        "{} reported capture discontinuity count={discontinuities}",
                        capture_source_label(source)
                    );
                }
            }
            let timestamp_has_error = flags & AUDCLNT_BUFFERFLAGS_TIMESTAMP_ERROR.0 as u32 != 0;
            if timestamp_has_error {
                timestamp_errors += 1;
                if timestamp_errors.is_power_of_two() {
                    log::warn!(
                        "{} reported timestamp error count={timestamp_errors}",
                        capture_source_label(source)
                    );
                }
            }
            let timestamp_100ns = if timestamp_has_error {
                next_packet_timestamp_100ns.unwrap_or(qpc)
            } else {
                qpc
            };
            let duration_100ns =
                frames as u64 * 10_000_000 / setup.format.sample_rate.max(1) as u64;
            next_packet_timestamp_100ns = Some(timestamp_100ns.saturating_add(duration_100ns));
            let samples = if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null() {
                vec![0.0; frames as usize]
            } else {
                unsafe { decode_mono(data, frames as usize, &setup.format) }
            };
            unsafe { setup.capture.ReleaseBuffer(frames)? };
            if packets
                .send(AudioPacket {
                    samples,
                    sample_rate: setup.format.sample_rate,
                    timestamp_100ns,
                    // Loopback positions are not a reliable packet sequence:
                    // the virtual process client and some Bluetooth render
                    // endpoints repeat or move their device position while
                    // continuing to deliver valid audio. Treating that as an
                    // overlap deletes real render packets. QPC is shared by
                    // all WASAPI clients and remains the alignment clock for
                    // both loopback modes; retain device position only for
                    // physical microphone capture.
                    device_position: matches!(source, CaptureSource::Microphone(_))
                        .then_some(device_position),
                    discontinuity,
                })
                .is_err()
            {
                return Ok(());
            }
        }
        if last_default_check.elapsed() >= Duration::from_secs(1) {
            if default_device_changed(source, setup.endpoint_id.as_deref())? {
                bail!("默认音频设备已改变");
            }
            if process_target_changed(source, setup.process_id)? {
                bail!("目标应用进程已退出或被替换");
            }
            last_default_check = std::time::Instant::now();
        }
    }
    Ok(())
}

#[implement(IMMNotificationClient)]
struct DeviceNotification {
    changed: Arc<AtomicBool>,
    filter: DeviceNotificationFilter,
}

#[derive(Clone)]
struct DeviceNotificationFilter {
    flow: EDataFlow,
    follow_default: bool,
    endpoint_id: String,
}

impl DeviceNotificationFilter {
    fn endpoint_matches(&self, device_id: &PCWSTR) -> bool {
        if device_id.is_null() {
            return false;
        }
        unsafe { device_id.to_string() }
            .is_ok_and(|value| value.eq_ignore_ascii_case(&self.endpoint_id))
    }

    fn default_matches(&self, flow: EDataFlow, role: ERole) -> bool {
        self.follow_default && flow.0 == self.flow.0 && role.0 == eCommunications.0
    }
}

impl IMMNotificationClient_Impl for DeviceNotification_Impl {
    fn OnDeviceStateChanged(
        &self,
        device_id: &PCWSTR,
        _new_state: DEVICE_STATE,
    ) -> windows::core::Result<()> {
        if self.filter.endpoint_matches(device_id) {
            self.changed.store(true, Ordering::Release);
        }
        Ok(())
    }

    fn OnDeviceAdded(&self, _device_id: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }

    fn OnDeviceRemoved(&self, device_id: &PCWSTR) -> windows::core::Result<()> {
        if self.filter.endpoint_matches(device_id) {
            self.changed.store(true, Ordering::Release);
        }
        Ok(())
    }

    fn OnDefaultDeviceChanged(
        &self,
        flow: EDataFlow,
        role: ERole,
        _device_id: &PCWSTR,
    ) -> windows::core::Result<()> {
        if self.filter.default_matches(flow, role) {
            self.changed.store(true, Ordering::Release);
        }
        Ok(())
    }

    fn OnPropertyValueChanged(
        &self,
        _device_id: &PCWSTR,
        _key: &windows::Win32::Foundation::PROPERTYKEY,
    ) -> windows::core::Result<()> {
        Ok(())
    }
}

struct DeviceNotificationRegistration {
    enumerator: IMMDeviceEnumerator,
    listener: IMMNotificationClient,
}

impl DeviceNotificationRegistration {
    fn new(changed: Arc<AtomicBool>, filter: DeviceNotificationFilter) -> Result<Self> {
        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
        let listener: IMMNotificationClient = DeviceNotification { changed, filter }.into();
        unsafe { enumerator.RegisterEndpointNotificationCallback(&listener)? };
        Ok(Self {
            enumerator,
            listener,
        })
    }
}

fn notification_filter(
    source: &CaptureSource,
    endpoint_id: Option<&str>,
) -> Option<DeviceNotificationFilter> {
    let endpoint_id = endpoint_id?.to_owned();
    match source {
        CaptureSource::System(selection) => Some(DeviceNotificationFilter {
            flow: eRender,
            follow_default: matches!(selection, DeviceSelection::FollowDefaultCommunications),
            endpoint_id,
        }),
        CaptureSource::Microphone(selection) => Some(DeviceNotificationFilter {
            flow: eCapture,
            follow_default: matches!(selection, DeviceSelection::FollowDefaultCommunications),
            endpoint_id,
        }),
        CaptureSource::Process { .. } => None,
    }
}

impl Drop for DeviceNotificationRegistration {
    fn drop(&mut self) {
        unsafe {
            let _ = self
                .enumerator
                .UnregisterEndpointNotificationCallback(&self.listener);
        }
    }
}

struct CaptureSetup {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    event: HANDLE,
    format: SampleFormat,
    endpoint_id: Option<String>,
    process_id: Option<u32>,
}

struct MmcssGuard {
    handle: Option<HANDLE>,
}

impl MmcssGuard {
    fn new() -> Self {
        let task_name = wide("Audio");
        let mut task_index = 0u32;
        match unsafe { AvSetMmThreadCharacteristicsW(PCWSTR(task_name.as_ptr()), &mut task_index) }
        {
            Ok(handle) => Self {
                handle: Some(handle),
            },
            Err(error) => {
                log::warn!("unable to enable MMCSS for audio capture thread: {error}");
                Self { handle: None }
            }
        }
    }
}

impl Drop for MmcssGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            unsafe {
                let _ = AvRevertMmThreadCharacteristics(handle);
            }
        }
    }
}

impl Drop for CaptureSetup {
    fn drop(&mut self) {
        unsafe {
            if !self.event.is_invalid() {
                let _ = CloseHandle(self.event);
            }
        }
    }
}

#[derive(Clone, Copy)]
enum SampleKind {
    Float32,
    Pcm16,
    Pcm24,
    Pcm32,
}

#[derive(Clone, Copy)]
struct SampleFormat {
    sample_rate: u32,
    channels: usize,
    block_align: usize,
    bits_per_sample: u16,
    valid_bits_per_sample: u16,
    kind: SampleKind,
}

unsafe fn decode_mono(data: *const u8, frames: usize, format: &SampleFormat) -> Vec<f32> {
    let mut output = Vec::with_capacity(frames);
    for frame in 0..frames {
        let base = unsafe { data.add(frame * format.block_align) };
        let mut sum = 0.0f32;
        for channel in 0..format.channels {
            sum += match format.kind {
                SampleKind::Float32 => unsafe {
                    std::ptr::read_unaligned(base.add(channel * 4).cast::<f32>())
                },
                SampleKind::Pcm16 => unsafe {
                    std::ptr::read_unaligned(base.add(channel * 2).cast::<i16>()) as f32 / 32_768.0
                },
                SampleKind::Pcm24 => unsafe {
                    let pointer = base.add(channel * 3);
                    let raw = (*pointer as i32)
                        | ((*pointer.add(1) as i32) << 8)
                        | ((*pointer.add(2) as i32) << 16);
                    let signed = if raw & 0x80_0000 != 0 {
                        raw | !0xFF_FFFF
                    } else {
                        raw
                    };
                    signed as f32 / 8_388_608.0
                },
                SampleKind::Pcm32 => unsafe {
                    std::ptr::read_unaligned(base.add(channel * 4).cast::<i32>()) as f32
                        / 2_147_483_648.0
                },
            };
        }
        output.push((sum / format.channels.max(1) as f32).clamp(-1.0, 1.0));
    }
    output
}

fn setup_endpoint(
    selection: DeviceSelection,
    flow: EDataFlow,
    loopback: bool,
) -> Result<CaptureSetup> {
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
    let device = endpoint_for_selection(&enumerator, &selection, flow)?;
    let client: IAudioClient = unsafe { device.Activate(CLSCTX_ALL, None)? };
    if flow.0 == eCapture.0 && !loopback {
        match client.cast::<IAudioClient2>() {
            Ok(communications_client) => {
                let properties = AudioClientProperties {
                    cbSize: size_of::<AudioClientProperties>() as u32,
                    eCategory: AudioCategory_Communications,
                    ..Default::default()
                };
                if let Err(error) =
                    unsafe { communications_client.SetClientProperties(&properties) }
                {
                    log::warn!(
                        "unable to categorize microphone as a communications stream: {error}"
                    );
                }
            }
            Err(error) => {
                log::warn!("microphone audio client does not support IAudioClient2: {error}");
            }
        }
    }
    let format_pointer = unsafe { client.GetMixFormat()? };
    if format_pointer.is_null() {
        bail!("音频设备没有返回共享模式格式");
    }
    let format = unsafe { parse_format(format_pointer)? };
    log::info!(
        "{} format rate={}Hz channels={} container_bits={} valid_bits={} block_align={}",
        if loopback {
            "system loopback"
        } else {
            "microphone"
        },
        format.sample_rate,
        format.channels,
        format.bits_per_sample,
        format.valid_bits_per_sample,
        format.block_align
    );
    let stream_flags = AUDCLNT_STREAMFLAGS_EVENTCALLBACK
        | if loopback {
            AUDCLNT_STREAMFLAGS_LOOPBACK
        } else {
            0
        };
    let initialized = unsafe {
        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            stream_flags,
            0,
            0,
            format_pointer,
            None,
        )
    };
    unsafe { CoTaskMemFree(Some(format_pointer.cast())) };
    initialized?;
    setup_event_capture(client, format, Some(device_id(&device)?), None)
}

fn endpoint_for_selection(
    enumerator: &IMMDeviceEnumerator,
    selection: &DeviceSelection,
    flow: EDataFlow,
) -> Result<IMMDevice> {
    match selection {
        DeviceSelection::FollowDefaultCommunications => unsafe {
            enumerator
                .GetDefaultAudioEndpoint(flow, eCommunications)
                .context("找不到默认通信音频设备")
        },
        DeviceSelection::Fixed { endpoint_id } => {
            let id = wide(endpoint_id);
            unsafe {
                enumerator
                    .GetDevice(PCWSTR(id.as_ptr()))
                    .context("找不到所选音频设备")
            }
        }
    }
}

fn setup_process_loopback(pid: u32) -> Result<CaptureSetup> {
    let format = WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_IEEE_FLOAT as u16,
        nChannels: 2,
        nSamplesPerSec: 48_000,
        nAvgBytesPerSec: 48_000 * 2 * 4,
        nBlockAlign: 8,
        wBitsPerSample: 32,
        cbSize: 0,
    };
    let sample_format = SampleFormat {
        sample_rate: 48_000,
        channels: 2,
        block_align: 8,
        bits_per_sample: 32,
        valid_bits_per_sample: 32,
        kind: SampleKind::Float32,
    };
    let mut activation = AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
            ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                TargetProcessId: pid,
                ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
            },
        },
    };
    let inner = PROPVARIANT_0_0 {
        vt: VT_BLOB,
        wReserved1: 0,
        wReserved2: 0,
        wReserved3: 0,
        Anonymous: PROPVARIANT_0_0_0 {
            blob: BLOB {
                cbSize: size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
                pBlobData: (&mut activation as *mut AUDIOCLIENT_ACTIVATION_PARAMS).cast(),
            },
        },
    };
    // `PROPVARIANT` from windows-rs calls `PropVariantClear` in `Drop`.
    // The VT_BLOB used by the process-loopback API only borrows this
    // stack-allocated activation structure (matching Microsoft's C++ sample).
    // Keep the outer value in `ManuallyDrop` so it cannot try to free the
    // borrowed stack pointer as COM task memory.
    let parameters = ManuallyDrop::new(PROPVARIANT {
        Anonymous: PROPVARIANT_0 {
            Anonymous: ManuallyDrop::new(inner),
        },
    });
    let event = unsafe { CreateEventW(None, false, false, None)? };
    let handler: IActivateAudioInterfaceCompletionHandler = ActivationHandler { event }.into();
    let operation = unsafe {
        ActivateAudioInterfaceAsync(
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
            &IAudioClient::IID,
            Some(&*parameters),
            &handler,
        )?
    };
    let wait = unsafe { WaitForSingleObject(event, ACTIVATION_WAIT_MS) };
    unsafe {
        let _ = CloseHandle(event);
    }
    if wait != WAIT_OBJECT_0 {
        bail!("指定应用音频回环启动超时");
    }
    let mut activation_result = windows::core::HRESULT(0);
    let mut unknown = None;
    unsafe {
        operation.GetActivateResult(&mut activation_result, &mut unknown)?;
    }
    activation_result.ok()?;
    let client: IAudioClient = unknown
        .ok_or_else(|| anyhow!("Windows 未返回应用回环音频客户端"))?
        .cast()?;
    unsafe {
        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK
                | AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
            0,
            0,
            &format,
            None,
        )?;
    }
    setup_event_capture(client, sample_format, None, Some(pid))
}

fn setup_event_capture(
    client: IAudioClient,
    format: SampleFormat,
    endpoint_id: Option<String>,
    process_id: Option<u32>,
) -> Result<CaptureSetup> {
    let event = unsafe { CreateEventW(None, false, false, None)? };
    if let Err(error) = unsafe { client.SetEventHandle(event) } {
        unsafe {
            let _ = CloseHandle(event);
        }
        return Err(error.into());
    }
    let capture: IAudioCaptureClient = unsafe { client.GetService()? };
    Ok(CaptureSetup {
        client,
        capture,
        event,
        format,
        endpoint_id,
        process_id,
    })
}

fn process_target_changed(source: &CaptureSource, effective_pid: Option<u32>) -> Result<bool> {
    let CaptureSource::Process {
        executable_path, ..
    } = source
    else {
        return Ok(false);
    };
    let Some(pid) = effective_pid else {
        return Ok(true);
    };
    Ok(process_path(pid)
        .map(|path| !path.eq_ignore_ascii_case(executable_path))
        .unwrap_or(true))
}

fn default_device_changed(source: &CaptureSource, endpoint_id: Option<&str>) -> Result<bool> {
    let flow = match source {
        CaptureSource::System(DeviceSelection::FollowDefaultCommunications) => eRender,
        CaptureSource::Microphone(DeviceSelection::FollowDefaultCommunications) => eCapture,
        _ => return Ok(false),
    };
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
    let current = unsafe { enumerator.GetDefaultAudioEndpoint(flow, eCommunications)? };
    Ok(endpoint_id != Some(device_id(&current)?.as_str()))
}

#[implement(IActivateAudioInterfaceCompletionHandler)]
struct ActivationHandler {
    event: HANDLE,
}

impl IActivateAudioInterfaceCompletionHandler_Impl for ActivationHandler_Impl {
    fn ActivateCompleted(
        &self,
        _operation: windows::core::Ref<IActivateAudioInterfaceAsyncOperation>,
    ) -> windows::core::Result<()> {
        unsafe { SetEvent(self.event)? };
        Ok(())
    }
}

unsafe fn parse_format(pointer: *const WAVEFORMATEX) -> Result<SampleFormat> {
    let format_tag = unsafe { std::ptr::addr_of!((*pointer).wFormatTag).read_unaligned() };
    let channels = unsafe { std::ptr::addr_of!((*pointer).nChannels).read_unaligned() as usize };
    let sample_rate = unsafe { std::ptr::addr_of!((*pointer).nSamplesPerSec).read_unaligned() };
    let block_align =
        unsafe { std::ptr::addr_of!((*pointer).nBlockAlign).read_unaligned() as usize };
    let bits_per_sample = unsafe { std::ptr::addr_of!((*pointer).wBitsPerSample).read_unaligned() };
    if channels == 0 || block_align == 0 || sample_rate == 0 {
        bail!("音频设备返回了无效格式");
    }
    let mut valid_bits_per_sample = bits_per_sample;
    let kind = if format_tag as u32 == WAVE_FORMAT_IEEE_FLOAT {
        SampleKind::Float32
    } else if format_tag as u32 == WAVE_FORMAT_PCM {
        pcm_kind(bits_per_sample)?
    } else if format_tag as u32 == WAVE_FORMAT_EXTENSIBLE {
        let extensible = pointer.cast::<WAVEFORMATEXTENSIBLE>();
        let sub_format = unsafe { std::ptr::addr_of!((*extensible).SubFormat).read_unaligned() };
        valid_bits_per_sample = unsafe {
            std::ptr::addr_of!((*extensible).Samples.wValidBitsPerSample).read_unaligned()
        };
        if sub_format == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT {
            SampleKind::Float32
        } else if sub_format == KSDATAFORMAT_SUBTYPE_PCM {
            pcm_kind(bits_per_sample)?
        } else {
            bail!("不支持的 WASAPI 共享模式子格式")
        }
    } else {
        bail!("不支持的 WASAPI 共享模式格式 {format_tag}")
    };
    Ok(SampleFormat {
        sample_rate,
        channels,
        block_align,
        bits_per_sample,
        valid_bits_per_sample,
        kind,
    })
}

fn pcm_kind(bits: u16) -> Result<SampleKind> {
    match bits {
        16 => Ok(SampleKind::Pcm16),
        24 => Ok(SampleKind::Pcm24),
        32 => Ok(SampleKind::Pcm32),
        _ => bail!("不支持的 PCM 位深 {bits}"),
    }
}

pub fn list_audio_devices() -> Result<Vec<AudioDevice>> {
    let _com = initialize_com()?;
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
    let default_render = unsafe {
        enumerator
            .GetDefaultAudioEndpoint(eRender, eCommunications)
            .ok()
            .and_then(|device| device_id(&device).ok())
    };
    let default_capture = unsafe {
        enumerator
            .GetDefaultAudioEndpoint(eCapture, eCommunications)
            .ok()
            .and_then(|device| device_id(&device).ok())
    };
    let mut result = Vec::new();
    enumerate_direction(
        &enumerator,
        eRender,
        DeviceDirection::Render,
        default_render.as_deref(),
        &mut result,
    )?;
    enumerate_direction(
        &enumerator,
        eCapture,
        DeviceDirection::Capture,
        default_capture.as_deref(),
        &mut result,
    )?;
    Ok(result)
}

fn enumerate_direction(
    enumerator: &IMMDeviceEnumerator,
    flow: EDataFlow,
    direction: DeviceDirection,
    default_id: Option<&str>,
    output: &mut Vec<AudioDevice>,
) -> Result<()> {
    let collection = unsafe { enumerator.EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE)? };
    let count = unsafe { collection.GetCount()? };
    for index in 0..count {
        let device = unsafe { collection.Item(index)? };
        let id = device_id(&device)?;
        let name = device_name(&device).unwrap_or_else(|_| "未命名音频设备".into());
        let form_factor = device_form_factor(&device).unwrap_or_else(|_| "Unknown".into());
        output.push(AudioDevice {
            is_default_communications: default_id == Some(id.as_str()),
            id,
            name,
            direction,
            form_factor,
            active: true,
        });
    }
    Ok(())
}

fn device_form_factor(device: &IMMDevice) -> Result<String> {
    let store: IPropertyStore = unsafe { device.OpenPropertyStore(STGM_READ)? };
    let mut value = unsafe { store.GetValue(&PKEY_AudioEndpoint_FormFactor)? };
    let raw = unsafe {
        let inner = &value.Anonymous.Anonymous;
        inner.Anonymous.ulVal
    };
    unsafe { PropVariantClear(&mut value)? };
    Ok(match EndpointFormFactor(raw as i32) {
        value if value == Speakers => "Speakers",
        value if value == Headphones => "Headphones",
        value if value == Headset => "Headset",
        value if value == Microphone => "Microphone",
        value if value == Handset => "Handset",
        value if value == LineLevel => "LineLevel",
        value if value == DigitalAudioDisplayDevice => "HDMI",
        value if value == SPDIF => "SPDIF",
        value if value == RemoteNetworkDevice => "Remote",
        _ => "Unknown",
    }
    .into())
}

fn device_id(device: &IMMDevice) -> Result<String> {
    let value = unsafe { device.GetId()? };
    let result = unsafe { value.to_string()? };
    unsafe { CoTaskMemFree(Some(value.0.cast())) };
    Ok(result)
}

fn device_name(device: &IMMDevice) -> Result<String> {
    let store: IPropertyStore = unsafe { device.OpenPropertyStore(STGM_READ)? };
    let mut value = unsafe { store.GetValue(&PKEY_Device_FriendlyName)? };
    let text = unsafe { PropVariantToStringAlloc(&value)? };
    let result = unsafe { text.to_string()? };
    unsafe {
        CoTaskMemFree(Some(text.0.cast()));
        PropVariantClear(&mut value)?;
    }
    Ok(result)
}

pub fn list_capture_targets() -> Result<Vec<CaptureTarget>> {
    let mut windows = Box::new(WindowAccumulator::default());
    unsafe {
        EnumWindows(
            Some(enumerate_window),
            LPARAM((&mut *windows as *mut WindowAccumulator) as isize),
        )?;
    }
    if let Ok(processes) = active_audio_processes() {
        for pid in processes {
            windows.seen.insert(pid);
            windows.titles.entry(pid).or_default();
        }
    }
    let own_pid = std::process::id();
    let mut targets = Vec::new();
    for (pid, title) in windows.titles {
        if pid == own_pid {
            continue;
        }
        let executable_path = match process_path(pid) {
            Ok(path) => path,
            Err(_) => continue,
        };
        let executable = Path::new(&executable_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let browser = matches!(executable.as_str(), "chrome.exe" | "msedge.exe");
        let priority = match executable.as_str() {
            "zoom.exe" => 100,
            "feishu.exe" | "lark.exe" => 95,
            "wemeetapp.exe" | "tencentmeeting.exe" => 90,
            "ms-teams.exe" | "teams.exe" => 85,
            "chrome.exe" => 80,
            "msedge.exe" => 75,
            _ => 0,
        };
        targets.push(CaptureTarget {
            id: format!("process:{pid}"),
            kind: "process".into(),
            display_name: if title.trim().is_empty() {
                executable.trim_end_matches(".exe").into()
            } else {
                title
            },
            process_id: pid,
            executable_path,
            browser,
            priority,
        });
    }
    targets.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| a.display_name.cmp(&b.display_name))
    });
    Ok(targets)
}

fn active_audio_processes() -> Result<Vec<u32>> {
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
    let endpoint = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)? };
    let manager: IAudioSessionManager2 = unsafe { endpoint.Activate(CLSCTX_ALL, None)? };
    let sessions = unsafe { manager.GetSessionEnumerator()? };
    let count = unsafe { sessions.GetCount()? };
    let mut result = Vec::new();
    for index in 0..count {
        let session = unsafe { sessions.GetSession(index)? };
        if let Ok(control) = session.cast::<IAudioSessionControl2>() {
            if let Ok(pid) = unsafe { control.GetProcessId() } {
                if pid != 0 {
                    result.push(pid);
                }
            }
        }
    }
    Ok(result)
}

#[derive(Default)]
struct WindowAccumulator {
    titles: HashMap<u32, String>,
    seen: HashSet<u32>,
}

unsafe extern "system" fn enumerate_window(hwnd: HWND, parameter: LPARAM) -> BOOL {
    if unsafe { !IsWindowVisible(hwnd).as_bool() } {
        return BOOL(1);
    }
    let length = unsafe { GetWindowTextLengthW(hwnd) };
    if length <= 0 {
        return BOOL(1);
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 {
        return BOOL(1);
    }
    let accumulator = unsafe { &mut *(parameter.0 as *mut WindowAccumulator) };
    if !accumulator.seen.insert(pid) {
        return BOOL(1);
    }
    let mut buffer = vec![0u16; length as usize + 1];
    let copied = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    let title = String::from_utf16_lossy(&buffer[..copied.max(0) as usize]);
    accumulator.titles.insert(pid, title);
    BOOL(1)
}

fn process_path(pid: u32) -> Result<String> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)? };
    let mut capacity = 32_768u32;
    let mut buffer = vec![0u16; capacity as usize];
    let result = unsafe {
        QueryFullProcessImageNameW(
            process,
            Default::default(),
            PWSTR(buffer.as_mut_ptr()),
            &mut capacity,
        )
    };
    unsafe {
        let _ = CloseHandle(process);
    }
    result?;
    Ok(String::from_utf16_lossy(&buffer[..capacity as usize]))
}

fn root_process_with_same_executable(pid: u32, executable_path: &str) -> Result<u32> {
    let parents = process_parent_map()?;
    let mut current = pid;
    let mut visited = HashSet::new();
    visited.insert(current);
    while let Some(parent) = parents.get(&current).copied() {
        if parent == 0 || !visited.insert(parent) {
            break;
        }
        let Ok(parent_path) = process_path(parent) else {
            break;
        };
        if !parent_path.eq_ignore_ascii_case(executable_path) {
            break;
        }
        current = parent;
    }
    Ok(current)
}

fn process_parent_map() -> Result<HashMap<u32, u32>> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)? };
    let result = (|| {
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut parents = HashMap::new();
        unsafe { Process32FirstW(snapshot, &mut entry)? };
        loop {
            parents.insert(entry.th32ProcessID, entry.th32ParentProcessID);
            if unsafe { Process32NextW(snapshot, &mut entry) }.is_err() {
                break;
            }
        }
        Ok(parents)
    })();
    unsafe {
        let _ = CloseHandle(snapshot);
    }
    result
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

struct ComGuard {
    uninitialize: bool,
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.uninitialize {
            unsafe { CoUninitialize() };
        }
    }
}

fn initialize_com() -> Result<ComGuard> {
    let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if result == RPC_E_CHANGED_MODE {
        // Tauri/WebView commands can run on an already initialized STA. COM
        // audio enumeration is apartment-neutral, so reuse that apartment.
        return Ok(ComGuard {
            uninitialize: false,
        });
    }
    result.ok()?;
    Ok(ComGuard { uninitialize: true })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_tone_wav() -> Vec<u8> {
        let sample_rate = 48_000u32;
        let sample_count = sample_rate / 2;
        let data_size = sample_count * 2;
        let mut wav = Vec::with_capacity((44 + data_size) as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_size).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_size.to_le_bytes());
        for index in 0..sample_count {
            let phase = index as f32 * 440.0 * std::f32::consts::TAU / sample_rate as f32;
            let sample = (phase.sin() * 2_000.0).round() as i16;
            wav.extend_from_slice(&sample.to_le_bytes());
        }
        wav
    }

    #[test]
    fn downmixes_interleaved_stereo_float() {
        let input = [1.0f32, -1.0, 0.5, 0.5];
        let format = SampleFormat {
            sample_rate: 48_000,
            channels: 2,
            block_align: 8,
            bits_per_sample: 32,
            valid_bits_per_sample: 32,
            kind: SampleKind::Float32,
        };
        let output = unsafe { decode_mono(input.as_ptr().cast(), 2, &format) };
        assert_eq!(output, vec![0.0, 0.5]);
    }

    #[test]
    fn converts_common_pcm_formats() {
        let pcm16 = [i16::MAX, i16::MAX];
        let format16 = SampleFormat {
            sample_rate: 48_000,
            channels: 2,
            block_align: 4,
            bits_per_sample: 16,
            valid_bits_per_sample: 16,
            kind: SampleKind::Pcm16,
        };
        let output16 = unsafe { decode_mono(pcm16.as_ptr().cast(), 1, &format16) };
        assert!(output16[0] > 0.99);

        let pcm24 = [0xffu8, 0xff, 0x7f, 0x00, 0x00, 0x80];
        let format24 = SampleFormat {
            sample_rate: 48_000,
            channels: 2,
            block_align: 6,
            bits_per_sample: 24,
            valid_bits_per_sample: 24,
            kind: SampleKind::Pcm24,
        };
        let output24 = unsafe { decode_mono(pcm24.as_ptr(), 1, &format24) };
        assert!(output24[0].abs() < 0.001);
    }

    #[test]
    #[ignore = "requires a running Chrome process and Windows Core Audio"]
    fn process_loopback_activation_smoke_test() {
        let target = list_capture_targets()
            .unwrap()
            .into_iter()
            .find(|target| {
                Path::new(&target.executable_path)
                    .file_name()
                    .is_some_and(|name| name.eq_ignore_ascii_case("chrome.exe"))
            })
            .expect("Chrome must be running for this hardware smoke test");
        let (sender, _receiver) = crossbeam_channel::unbounded();
        let capture = start_capture(
            CaptureSource::Process {
                process_id: target.process_id,
                executable_path: target.executable_path,
            },
            sender,
        )
        .unwrap();

        std::thread::sleep(Duration::from_millis(500));
        assert!(capture.health_flag().load(Ordering::Acquire));
        capture.stop();
    }

    #[test]
    #[ignore = "plays a short local tone and requires Windows Core Audio"]
    fn process_loopback_delivers_audible_audio_through_the_mixer() {
        use crate::audio::AudioMixer;
        use crate::models::AecMode;

        let executable_path = std::env::current_exe()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let (sender, receiver) = crossbeam_channel::unbounded();
        let capture = start_capture(
            CaptureSource::Process {
                process_id: std::process::id(),
                executable_path,
            },
            sender,
        )
        .unwrap();
        let tone = test_tone_wav();
        assert!(
            unsafe {
                PlaySoundW(
                    PCWSTR(tone.as_ptr().cast()),
                    None,
                    SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
                )
            }
            .as_bool()
        );
        let mut mixer = AudioMixer::new(AecMode::Off, false);
        let mut audible_frames = 0usize;
        for _ in 0..120 {
            std::thread::sleep(Duration::from_millis(10));
            while let Ok(packet) = receiver.try_recv() {
                mixer.push_system(packet);
            }
            let (_, system_level, _) = mixer.next_frame(true, false);
            if system_level > 0.005 {
                audible_frames += 1;
            }
        }
        unsafe {
            let _ = PlaySoundW(PCWSTR::null(), None, SND_ASYNC);
        }
        capture.stop();
        assert!(
            audible_frames >= 20,
            "captured tone was not preserved by process loopback and mixing: {audible_frames}"
        );
    }

    #[test]
    #[ignore = "requires a Windows microphone endpoint"]
    fn microphone_activation_smoke_test() {
        let (sender, _receiver) = crossbeam_channel::unbounded();
        let capture = start_capture(
            CaptureSource::Microphone(DeviceSelection::FollowDefaultCommunications),
            sender,
        )
        .unwrap();

        std::thread::sleep(Duration::from_millis(500));
        assert!(capture.health_flag().load(Ordering::Acquire));
        capture.stop();
    }

    #[test]
    #[ignore = "requires Windows render and microphone endpoints"]
    fn system_loopback_and_microphone_stay_healthy_together() {
        use crate::audio::AudioMixer;
        use crate::models::AecMode;

        let (system_sender, system_receiver) = crossbeam_channel::unbounded();
        let (microphone_sender, microphone_receiver) = crossbeam_channel::unbounded();
        let system_capture = start_capture(
            CaptureSource::System(DeviceSelection::FollowDefaultCommunications),
            system_sender,
        )
        .unwrap();
        let microphone_capture = start_capture(
            CaptureSource::Microphone(DeviceSelection::FollowDefaultCommunications),
            microphone_sender,
        )
        .unwrap();

        let mut mixer = AudioMixer::new(AecMode::Off, false);
        let tone = test_tone_wav();
        assert!(
            unsafe {
                PlaySoundW(
                    PCWSTR(tone.as_ptr().cast()),
                    None,
                    SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
                )
            }
            .as_bool()
        );
        let started = std::time::Instant::now();
        let mut system_samples = 0usize;
        let mut microphone_samples = 0usize;
        let mut raw_system_audio = Vec::new();
        let mut audible_system_frames = 0usize;
        let mut consecutive_audible = 0usize;
        let mut longest_audible_run = 0usize;
        for _ in 0..300 {
            std::thread::sleep(Duration::from_millis(10));
            assert!(system_capture.health_flag().load(Ordering::Acquire));
            assert!(microphone_capture.health_flag().load(Ordering::Acquire));
            while let Ok(packet) = system_receiver.try_recv() {
                system_samples += packet.samples.len();
                raw_system_audio.extend_from_slice(&packet.samples);
                mixer.push_system(packet);
            }
            while let Ok(packet) = microphone_receiver.try_recv() {
                microphone_samples += packet.samples.len();
                mixer.push_microphone(packet);
            }
            let (_, system_level, _) = mixer.next_frame(true, true);
            if system_level > 0.001 {
                audible_system_frames += 1;
                consecutive_audible += 1;
                longest_audible_run = longest_audible_run.max(consecutive_audible);
            } else {
                consecutive_audible = 0;
            }
        }
        unsafe {
            let _ = PlaySoundW(PCWSTR::null(), None, SND_ASYNC);
        }
        let mut raw_run = 0usize;
        let mut raw_longest_run = 0usize;
        for block in raw_system_audio.chunks(480) {
            let level = (block.iter().map(|sample| sample * sample).sum::<f32>()
                / block.len().max(1) as f32)
                .sqrt();
            if level > 0.001 {
                raw_run += 1;
                raw_longest_run = raw_longest_run.max(raw_run);
            } else {
                raw_run = 0;
            }
        }
        let diagnostics = mixer.diagnostics();
        eprintln!(
            "hardware delivery system_samples={} microphone_samples={} raw_longest_run={} audible_system_frames={} longest_audible_run={} system_underflows={} microphone_underflows={} elapsed={:.3}s",
            system_samples,
            microphone_samples,
            raw_longest_run,
            audible_system_frames,
            longest_audible_run,
            diagnostics.system_underflows,
            diagnostics.microphone_underflows,
            started.elapsed().as_secs_f64()
        );
        assert!(system_samples > 0);
        assert!(microphone_samples > 0);
        assert!(
            longest_audible_run >= 30,
            "system loopback tone was pulsed or interrupted: {longest_audible_run}"
        );
        microphone_capture.stop();
        system_capture.stop();
    }

    #[test]
    fn endpoint_notifications_ignore_unrelated_flows() {
        let filter = DeviceNotificationFilter {
            flow: eCapture,
            follow_default: true,
            endpoint_id: "microphone".into(),
        };
        assert!(filter.default_matches(eCapture, eCommunications));
        assert!(!filter.default_matches(eRender, eCommunications));
        assert!(!filter.default_matches(eCapture, eMultimedia));
    }
}
