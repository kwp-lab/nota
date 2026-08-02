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
  transcription?: TranscriptionSummary | null;
}

export interface LevelEvent {
  system: number;
  microphone: number;
}

export interface AppSettings {
  outputDirectory: string;
  aecMode: AecMode;
  microphoneEnabled: boolean;
  firstRunComplete: boolean;
  shortcutsEnabled: boolean;
  toggleShortcut: string;
  stopShortcut: string;
  activeAsrProviderId: string | null;
  autoTranscribe: boolean;
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
  completedChunks: number;
  totalChunks: number;
  errorMessage: string | null;
  updatedAt: string;
}

export interface TranscriptionEvent {
  recordingId: string;
  summary: TranscriptionSummary;
}
