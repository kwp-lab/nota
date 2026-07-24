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
  consentConfirmed: boolean;
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
}

export interface LevelEvent {
  system: number;
  microphone: number;
}

export interface AppSettings {
  outputDirectory: string;
  aecMode: AecMode;
  microphoneEnabled: boolean;
  consentTemplate: string;
  firstRunComplete: boolean;
  recordingNoticeAcknowledged: boolean;
  shortcutsEnabled: boolean;
  toggleShortcut: string;
  stopShortcut: string;
}
