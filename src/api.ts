import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import type {
  AppSettings,
  AsrConnectionTest,
  AsrModel,
  AsrProvider,
  AsrProviderProbeRequest,
  AudioDevice,
  CaptureSelection,
  CaptureTarget,
  DeviceSelection,
  LevelEvent,
  ParticipantProfile,
  RecordingItem,
  RecordingSnapshot,
  StartRecordingRequest,
  SaveAsrProviderRequest,
  TranscriptDocument,
  SpeakerIdentificationAssignment,
  SpeakerIdentificationSession,
  TranscriptionEvent,
  TranscriptionSummary,
} from "./types";

export const api = {
  getAppVersion: getVersion,
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
  quitApplication: (stopAndSave: boolean) =>
    invoke<void>("quit_application", { stopAndSave }),
  listAsrProviders: () => invoke<AsrProvider[]>("list_asr_providers"),
  saveAsrProvider: (request: SaveAsrProviderRequest) =>
    invoke<AsrProvider>("save_asr_provider", { request }),
  deleteAsrProvider: (id: string) =>
    invoke<void>("delete_asr_provider", { id }),
  setActiveAsrProvider: (id: string | null) =>
    invoke<AppSettings>("set_active_asr_provider", { id }),
  testAsrProvider: (request: AsrProviderProbeRequest) =>
    invoke<AsrConnectionTest>("test_asr_provider", { request }),
  listAsrModels: (request: AsrProviderProbeRequest) =>
    invoke<AsrModel[]>("list_asr_models", { request }),
  startTranscription: (
    recordingId: string,
    providerId?: string | null,
    speakerCount: number | null = null,
  ) =>
    invoke<TranscriptionSummary>("start_transcription", {
      recordingId,
      providerId: providerId ?? null,
      speakerCount,
    }),
  cancelTranscription: (recordingId: string) =>
    invoke<TranscriptionSummary>("cancel_transcription", { recordingId }),
  resumeTranscription: (recordingId: string) =>
    invoke<TranscriptionSummary>("resume_transcription", { recordingId }),
  getTranscript: (recordingId: string) =>
    invoke<TranscriptDocument>("get_transcript", { recordingId }),
  copyTranscript: (recordingId: string) =>
    invoke<void>("copy_transcript", { recordingId }),
  exportTranscript: (recordingId: string, path: string) =>
    invoke<void>("export_transcript", { recordingId, path }),
  revealTranscriptExport: (path: string) =>
    invoke<void>("reveal_transcript_export", { path }),
  identifyRecordingSpeakers: (recordingId: string, providerId?: string | null) =>
    invoke<SpeakerIdentificationSession>("identify_recording_speakers", {
      recordingId,
      providerId: providerId ?? null,
    }),
  saveSpeakerIdentification: (
    sessionId: string,
    assignments: SpeakerIdentificationAssignment[],
  ) => invoke<TranscriptDocument>("save_speaker_identification", {
    sessionId,
    assignments,
  }),
  updateRecordingSpeakerAssignments: (
    recordingId: string,
    assignments: SpeakerIdentificationAssignment[],
  ) => invoke<TranscriptDocument>("update_recording_speaker_assignments", {
    recordingId,
    assignments,
  }),
  discardSpeakerIdentification: (sessionId: string) =>
    invoke<void>("discard_speaker_identification", { sessionId }),
  listParticipants: () => invoke<ParticipantProfile[]>("list_participants"),
  renameParticipant: (id: string, displayName: string) =>
    invoke<ParticipantProfile[]>("rename_participant", { id, displayName }),
  deleteParticipant: (id: string) =>
    invoke<ParticipantProfile[]>("delete_participant", { id }),
  deleteVoiceprint: (id: string) =>
    invoke<ParticipantProfile[]>("delete_voiceprint", { id }),
  hasActiveTranscription: () =>
    invoke<boolean>("has_active_transcription"),
  onSnapshot: (handler: (snapshot: RecordingSnapshot) => void) =>
    listen<RecordingSnapshot>("recording://snapshot", (event) =>
      handler(event.payload),
    ),
  onLevels: (handler: (levels: LevelEvent) => void) =>
    listen<LevelEvent>("recording://levels", (event) =>
      handler(event.payload),
    ),
  onRequestStart: (
    handler: (mode: "process" | "system" | "current") => void,
  ) =>
    listen<"process" | "system" | "current">(
      "recording://request-start",
      (event) => handler(event.payload),
    ),
  onRequestExit: (handler: () => void) =>
    listen<void>("recording://request-exit", handler),
  onAsrStatus: (handler: (event: TranscriptionEvent) => void) =>
    listen<TranscriptionEvent>("asr://status", (event) =>
      handler(event.payload),
    ),
};

export type { UnlistenFn };
