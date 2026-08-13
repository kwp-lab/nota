import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import type {
  AiDocumentContent,
  AiDocumentVersion,
  AiGenerationDraftRequest,
  AiGenerationDetails,
  AiGenerationEvent,
  AiGenerationRequest,
  AiGenerationRequestPreview,
  AiTemplate,
  AiWorkspace,
  AppSettings,
  AsrConnectionTest,
  AsrModel,
  AsrProvider,
  AsrProviderProbeRequest,
  AudioImportBatchSnapshot,
  AudioDevice,
  CaptureSelection,
  CaptureTarget,
  DeviceSelection,
  LevelEvent,
  LlmConnectionTest,
  LlmModel,
  LlmProvider,
  LlmProviderProbeRequest,
  CapturePrompt,
  ParticipantProfile,
  RecordingItem,
  RecordingSnapshot,
  StartRecordingRequest,
  SaveAsrProviderRequest,
  SaveAiTemplateRequest,
  SaveLlmProviderRequest,
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
  getCapturePrompt: () => invoke<CapturePrompt | null>("get_capture_prompt"),
  respondCapturePrompt: (sessionId: string, stopAndSave: boolean) =>
    invoke<RecordingSnapshot>("respond_capture_prompt", {
      sessionId,
      stopAndSave,
    }),
  resizeCapturePrompt: (height: number) =>
    invoke<void>("resize_capture_prompt", { height }),
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
  startAudioImport: (paths: string[]) =>
    invoke<AudioImportBatchSnapshot>("start_audio_import", { paths }),
  getAudioImportSnapshot: () =>
    invoke<AudioImportBatchSnapshot | null>("get_audio_import_snapshot"),
  cancelAudioImport: () =>
    invoke<AudioImportBatchSnapshot>("cancel_audio_import"),
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
  deleteRecording: (
    id: string,
    permanent: boolean,
    deleteAiDocuments = false,
  ) => invoke<void>("delete_recording", { id, permanent, deleteAiDocuments }),
  openMicrophoneSettings: () => invoke<void>("open_microphone_settings"),
  openLogDirectory: () => invoke<void>("open_log_directory"),
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
  listLlmProviders: () => invoke<LlmProvider[]>("list_llm_providers"),
  saveLlmProvider: (request: SaveLlmProviderRequest) =>
    invoke<LlmProvider>("save_llm_provider", { request }),
  deleteLlmProvider: (id: string) =>
    invoke<void>("delete_llm_provider", { id }),
  setActiveLlmProvider: (id: string | null) =>
    invoke<AppSettings>("set_active_llm_provider", { id }),
  testLlmProvider: (request: LlmProviderProbeRequest) =>
    invoke<LlmConnectionTest>("test_llm_provider", { request }),
  listLlmModels: (request: LlmProviderProbeRequest) =>
    invoke<LlmModel[]>("list_llm_models", { request }),
  listAiTemplates: () => invoke<AiTemplate[]>("list_ai_templates"),
  saveAiTemplate: (request: SaveAiTemplateRequest) =>
    invoke<AiTemplate>("save_ai_template", { request }),
  cloneAiTemplate: (id: string, name: string) =>
    invoke<AiTemplate>("clone_ai_template", { id, name }),
  archiveAiTemplate: (id: string) =>
    invoke<void>("archive_ai_template", { id }),
  getAiWorkspace: (recordingId: string) =>
    invoke<AiWorkspace>("get_ai_workspace", { recordingId }),
  previewAiGenerationRequest: (request: AiGenerationDraftRequest) =>
    invoke<AiGenerationRequestPreview>("preview_ai_generation_request", { request }),
  copyAiRequestBody: (requestBody: string) =>
    invoke<void>("copy_ai_request_body", { requestBody }),
  copyAiGenerationJson: (json: string) =>
    invoke<void>("copy_ai_generation_json", { json }),
  listAiDocumentVersions: (documentId: string) =>
    invoke<AiDocumentVersion[]>("list_ai_document_versions", { documentId }),
  generateAiDocument: (request: AiGenerationRequest) =>
    invoke<AiDocumentVersion>("generate_ai_document", { request }),
  cancelAiGeneration: (versionId: string) =>
    invoke<AiDocumentVersion>("cancel_ai_generation", { versionId }),
  readAiDocumentVersion: (versionId: string) =>
    invoke<AiDocumentContent>("read_ai_document_version", { versionId }),
  readAiGenerationDetails: (versionId: string) =>
    invoke<AiGenerationDetails>("read_ai_generation_details", { versionId }),
  relinkAiDocumentVersion: (versionId: string, path: string) =>
    invoke<AiDocumentVersion>("relink_ai_document_version", {
      versionId,
      path,
    }),
  findAiDocumentVersion: (versionId: string, workspacePath: string) =>
    invoke<AiDocumentVersion>("find_ai_document_version", {
      versionId,
      workspacePath,
    }),
  openAiDocumentVersion: (versionId: string) =>
    invoke<void>("open_ai_document_version", { versionId }),
  revealAiDocumentVersion: (versionId: string) =>
    invoke<void>("reveal_ai_document_version", { versionId }),
  copyAiDocumentVersion: (versionId: string) =>
    invoke<void>("copy_ai_document_version", { versionId }),
  copyAiDocumentPath: (versionId: string) =>
    invoke<void>("copy_ai_document_path", { versionId }),
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
  onCapturePromptUpdated: (handler: (prompt: CapturePrompt) => void) =>
    listen<CapturePrompt>("capture://prompt-updated", (event) =>
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
  onAiStatus: (handler: (event: AiGenerationEvent) => void) =>
    listen<AiGenerationEvent>("ai://status", (event) =>
      handler(event.payload),
    ),
  onAudioImportStatus: (handler: (snapshot: AudioImportBatchSnapshot) => void) =>
    listen<AudioImportBatchSnapshot>("audio-import://status", (event) =>
      handler(event.payload),
    ),
};

export type { UnlistenFn };
