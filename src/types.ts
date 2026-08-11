export type RecordingState =
  | "idle"
  | "preparing"
  | "recording"
  | "paused"
  | "interrupted"
  | "recovering"
  | "finalizing"
  | "completed"
  | "error";

export interface CaptureTarget {
  id: string;
  kind: "process";
  displayName: string;
  processId: number;
  executablePath: string;
  browser: boolean;
  priority: number;
}

export interface AudioDevice {
  id: string;
  name: string;
  direction: "render" | "capture";
  isDefaultCommunications: boolean;
  formFactor: string;
  active: boolean;
}

export type DeviceSelection =
  | { kind: "followDefaultCommunications" }
  | { kind: "fixed"; endpointId: string };

export type CaptureSelection =
  | { kind: "process"; targetId: string }
  | { kind: "system"; device: DeviceSelection };

export type AecMode = "auto" | "on" | "off";

export interface StartRecordingRequest {
  capture: CaptureSelection;
  microphone: DeviceSelection | null;
  aecMode: AecMode;
  outputDirectory: string;
}

export interface SourceStatus {
  healthy: boolean;
  label: string;
  detail?: string;
}

export interface RecordingSnapshot {
  sessionId: string | null;
  state: RecordingState;
  startedAt: string | null;
  activeDurationMs: number;
  bytesWritten: number;
  outputPath: string | null;
  system: SourceStatus;
  microphone: SourceStatus;
  microphoneSelection: DeviceSelection | null;
  aecStatus: "disabled" | "enabled" | "converging";
  fault: RecordingFault | null;
}

export interface RecordingFault {
  component: string;
  code: string;
  recoverable: boolean;
  userMessage: string;
  occurredAt: string;
}

export interface RecordingItem {
  id: string;
  title: string;
  path: string;
  createdAt: string;
  durationMs: number;
  sizeBytes: number;
  recovered: boolean;
  origin: "captured" | "imported";
  sourceFileName: string | null;
  sourceFormat: string | null;
  importedAt: string | null;
  transcription?: TranscriptionSummary | null;
}

export type AudioImportBatchStatus = "running" | "completed" | "cancelled";

export type AudioImportItemStatus =
  | "queued"
  | "probing"
  | "decoding"
  | "finalizing"
  | "completed"
  | "failed"
  | "skipped"
  | "cancelled";

export interface AudioImportItemSnapshot {
  id: string;
  fileName: string;
  status: AudioImportItemStatus;
  progressCurrentMs: number;
  progressTotalMs: number;
  errorMessage: string | null;
  recordingId: string | null;
}

export interface AudioImportBatchSnapshot {
  id: string;
  status: AudioImportBatchStatus;
  currentIndex: number;
  total: number;
  completed: number;
  failed: number;
  skipped: number;
  items: AudioImportItemSnapshot[];
}

export interface LevelEvent {
  system: number;
  microphone: number;
}

export interface MeetingEndPrompt {
  sessionId: string;
  targetName: string;
}

export interface AppSettings {
  outputDirectory: string;
  aiDocumentsDirectory: string;
  aecMode: AecMode;
  microphoneEnabled: boolean;
  firstRunComplete: boolean;
  shortcutsEnabled: boolean;
  toggleShortcut: string;
  stopShortcut: string;
  activeAsrProviderId: string | null;
  voiceprintProviderId: string | null;
  autoTranscribe: boolean;
  activeLlmProviderId: string | null;
}

export type AsrProviderKind = "funAsr" | "openAiCompatible";

export interface AsrProvider {
  id: string;
  name: string;
  kind: AsrProviderKind;
  baseUrl: string;
  modelId: string;
  hasApiKey: boolean;
  createdAt: string;
  updatedAt: string;
}

export type AsrApiKeyUpdate =
  | { kind: "keep" }
  | { kind: "replace"; value: string }
  | { kind: "clear" };

export interface SaveAsrProviderRequest {
  id: string | null;
  name: string;
  kind: AsrProviderKind;
  baseUrl: string;
  modelId: string;
  apiKey: AsrApiKeyUpdate;
}

export interface AsrProviderProbeRequest {
  id: string | null;
  kind: AsrProviderKind;
  baseUrl: string;
  modelId: string;
  apiKey: AsrApiKeyUpdate;
}

export interface AsrModel {
  id: string;
  ownedBy: string | null;
  ready: boolean | null;
}

export interface AsrConnectionTest {
  reachable: boolean;
  level: "success" | "warning";
  message: string;
  models: AsrModel[];
  device: string | null;
}

export type LlmProviderKind = "openAi" | "openAiCompatible";

export interface LlmProvider {
  id: string;
  name: string;
  kind: LlmProviderKind;
  baseUrl: string;
  modelId: string;
  inputTokenBudget: number;
  maxOutputTokens: number;
  hasApiKey: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface SaveLlmProviderRequest {
  id: string | null;
  name: string;
  kind: LlmProviderKind;
  baseUrl: string;
  modelId: string;
  inputTokenBudget: number;
  maxOutputTokens: number;
  apiKey: AsrApiKeyUpdate;
}

export interface LlmProviderProbeRequest {
  id: string | null;
  kind: LlmProviderKind;
  baseUrl: string;
  modelId: string;
  inputTokenBudget: number;
  maxOutputTokens: number;
  apiKey: AsrApiKeyUpdate;
}

export interface LlmModel {
  id: string;
  ownedBy: string | null;
}

export interface LlmConnectionTest {
  reachable: boolean;
  message: string;
}

export type TranscriptionStatus =
  | "queued"
  | "preparing"
  | "transcribing"
  | "completed"
  | "failed"
  | "interrupted"
  | "cancelled";

export type TranscriptionProtocol = "legacy_chunks" | "nota_batch_v1";

export type TranscriptionProgressPhase =
  | "preparing"
  | "uploading"
  | "queued"
  | "transcribing"
  | "diarizing"
  | "finalizing";

export type TranscriptionProgressUnit = "bytes" | "windows" | "steps" | "chunks";

export interface TranscriptionSummary {
  status: TranscriptionStatus;
  completedChunks: number;
  totalChunks: number;
  providerName: string;
  modelId: string;
  speakerCount: number | null;
  errorMessage: string | null;
  hasText: boolean;
  protocol: TranscriptionProtocol;
  progressPhase: TranscriptionProgressPhase | null;
  progressCurrent: number;
  progressTotal: number;
  progressUnit: TranscriptionProgressUnit | null;
}

export interface TranscriptSegment {
  startMs: number;
  endMs: number;
  text: string;
  speaker: string | null;
}

export interface TranscriptDocument {
  recordingId: string;
  status: TranscriptionStatus;
  providerName: string;
  modelId: string;
  language: string | null;
  text: string;
  segments: TranscriptSegment[];
  speakerNames: Record<string, string>;
  speakerAssignments: Record<string, RecordingSpeakerAssignment>;
  completedChunks: number;
  totalChunks: number;
  errorMessage: string | null;
  updatedAt: string;
}

export interface RecordingSpeakerAssignment {
  participantId: string;
  displayName: string;
}

export interface VoiceprintSample {
  id: string;
  participantId: string;
  embeddingFingerprint: string;
  sourceRecordingId: string | null;
  sourceRecordingTitle: string | null;
  sourceSpeaker: string;
  previewStartMs: number;
  previewEndMs: number;
  previewAvailable: boolean;
  speechDurationMs: number;
  createdAt: string;
}

export interface ParticipantProfile {
  id: string;
  displayName: string;
  createdAt: string;
  updatedAt: string;
  samples: VoiceprintSample[];
}

export interface SpeakerIdentificationCandidate {
  rawSpeaker: string;
  totalSpeechMs: number;
  previewStartMs: number;
  previewEndMs: number;
  embeddingExtracted: boolean;
  sampleStatus: "enrollable" | "preview_only" | "unavailable";
  statusMessage: string | null;
  errorMessage: string | null;
  suggestedParticipantId: string | null;
  suggestedParticipantName: string | null;
  matchScore: number | null;
}

export interface SpeakerIdentificationSession {
  id: string;
  recordingId: string;
  speakerCount: number;
  voiceprintCount: number;
  candidates: SpeakerIdentificationCandidate[];
}

export interface SpeakerIdentificationAssignment {
  rawSpeaker: string;
  participantId: string | null;
  newDisplayName: string | null;
}

export interface TranscriptionEvent {
  recordingId: string;
  summary: TranscriptionSummary;
}

export interface AiTemplate {
  id: string;
  name: string;
  description: string;
  builtinKey: string | null;
  taskInstructions: string;
  outputRequirements: string;
  requiresSpeakerLabels: boolean;
  revision: number;
  archived: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface SaveAiTemplateRequest {
  id: string | null;
  name: string;
  description: string;
  taskInstructions: string;
  outputRequirements: string;
  requiresSpeakerLabels: boolean;
}

export interface AiMeetingProfile {
  recordingId: string;
  workspacePath: string;
  meetingContext: string;
  updatedAt: string;
}

export type AiGenerationMode = "create" | "regenerate" | "revise";
export type AiGenerationStatus =
  | "queued"
  | "generating"
  | "completed"
  | "failed"
  | "cancelled"
  | "interrupted";
export type AiFileState = "pending" | "ready" | "modified" | "missing";

export interface AiDocumentVersion {
  id: string;
  documentId: string;
  versionNumber: number;
  mode: AiGenerationMode;
  parentVersionId: string | null;
  status: AiGenerationStatus;
  filePath: string | null;
  fileState: AiFileState;
  providerName: string;
  providerKind: LlmProviderKind;
  modelId: string;
  templateName: string;
  templateRevision: number;
  transcriptionGeneration: number;
  estimatedInputTokens: number;
  inputTokens: number | null;
  outputTokens: number | null;
  errorMessage: string | null;
  createdAt: string;
  completedAt: string | null;
}

export interface AiDocument {
  id: string;
  recordingId: string;
  templateId: string;
  title: string;
  requirements: string;
  templateName: string;
  templateBuiltinKey: string | null;
  latestVersion: AiDocumentVersion | null;
  createdAt: string;
  updatedAt: string;
}

export interface AiWorkspace {
  profile: AiMeetingProfile;
  documents: AiDocument[];
  templates: AiTemplate[];
}

export interface AiGenerationRequest {
  recordingId: string;
  mode: AiGenerationMode;
  documentId: string | null;
  templateId: string | null;
  title: string | null;
  meetingContext: string;
  documentRequirements: string;
  runRequest: string;
  providerId: string | null;
  modelId: string | null;
  sourceVersionId: string | null;
}

export interface AiDocumentContent {
  version: AiDocumentVersion;
  markdown: string;
}

export interface AiGenerationEvent {
  recordingId: string;
  documentId: string;
  version: AiDocumentVersion;
}
