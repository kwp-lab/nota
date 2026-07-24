import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppSettings,
  AudioDevice,
  CaptureSelection,
  CaptureTarget,
  DeviceSelection,
  LevelEvent,
  RecordingItem,
  RecordingSnapshot,
  StartRecordingRequest,
} from "./types";

export const api = {
  listCaptureTargets: () => invoke<CaptureTarget[]>("list_capture_targets"),
  listAudioDevices: () => invoke<AudioDevice[]>("list_audio_devices"),
  getSettings: () => invoke<AppSettings>("get_settings"),
  saveSettings: (settings: AppSettings) =>
    invoke<void>("save_settings", { settings }),
  getSnapshot: () => invoke<RecordingSnapshot>("get_recording_snapshot"),
  startRecording: (request: StartRecordingRequest) =>
    invoke<RecordingSnapshot>("start_recording", { request }),
  pauseRecording: () => invoke<RecordingSnapshot>("pause_recording"),
  resumeRecording: () => invoke<RecordingSnapshot>("resume_recording"),
  stopRecording: () => invoke<RecordingSnapshot>("stop_recording"),
  switchCaptureSource: (capture: CaptureSelection) =>
    invoke<RecordingSnapshot>("switch_capture_source", { capture }),
  setMicrophoneEnabled: (microphone: DeviceSelection | null) =>
    invoke<RecordingSnapshot>("set_microphone_enabled", { microphone }),
  listRecordings: () => invoke<RecordingItem[]>("list_recordings"),
  prepareRecordingPlayback: (id: string) =>
    invoke<string>("prepare_recording_playback", { id }),
  listRecoverable: () => invoke<RecordingItem[]>("list_recoverable_recordings"),
  recoverRecording: (id: string) =>
    invoke<RecordingItem>("recover_recording", { id }),
  deleteRecoverable: (id: string) =>
    invoke<void>("delete_recoverable_recording", { id }),
  renameRecording: (id: string, title: string) =>
    invoke<RecordingItem>("rename_recording", { id, title }),
  revealRecording: (id: string) => invoke<void>("reveal_recording", { id }),
  deleteRecording: (id: string, permanent: boolean) =>
    invoke<void>("delete_recording", { id, permanent }),
  openMicrophoneSettings: () => invoke<void>("open_microphone_settings"),
  copyConsentTemplate: (text: string) =>
    invoke<void>("copy_consent_template", { text }),
  quitApplication: (stopAndSave: boolean) =>
    invoke<void>("quit_application", { stopAndSave }),
  onSnapshot: (handler: (snapshot: RecordingSnapshot) => void) =>
    listen<RecordingSnapshot>("recording://snapshot", (event) =>
      handler(event.payload),
    ),
  onLevels: (handler: (levels: LevelEvent) => void) =>
    listen<LevelEvent>("recording://levels", (event) =>
      handler(event.payload),
    ),
  onRequestStart: (handler: () => void) =>
    listen<void>("recording://request-start", handler),
  onRequestExit: (handler: () => void) =>
    listen<void>("recording://request-exit", handler),
};

export type { UnlistenFn };
