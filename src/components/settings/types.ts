import type {
  AppSettings,
  AsrProvider,
  LlmProvider,
} from "../../types";
import type { ToastTone } from "../ToastRegion";

export type SettingsRoute =
  | "setup"
  | "recording"
  | "transcription"
  | "asrProviders"
  | "aiDocuments"
  | "llmProviders"
  | "aiTemplates"
  | "shortcuts"
  | "diagnostics"
  | "about";

export interface SettingsModel {
  settings: AppSettings;
  asrProviders: AsrProvider[];
  llmProviders: LlmProvider[];
  microphoneCount: number;
  appVersion: string;
  recordingActive: boolean;
}

export interface SettingsActions {
  onSettingsChange: (settings: AppSettings) => void;
  onAsrProvidersChange: (providers: AsrProvider[]) => void;
  onLlmProvidersChange: (providers: LlmProvider[]) => void;
  onFirstRunComplete: () => void;
  onEditorDirtyChange: (dirty: boolean) => void;
  onToast: (tone: ToastTone, message: string) => void;
  onError: (error: unknown) => void;
}

export interface SettingsWorkspaceProps {
  route: SettingsRoute;
  onRouteChange: (route: SettingsRoute) => void;
  model: SettingsModel;
  actions: SettingsActions;
}

export const parentSettingsRoute = (route: SettingsRoute): SettingsRoute => {
  if (route === "asrProviders") return "transcription";
  if (route === "llmProviders" || route === "aiTemplates") return "aiDocuments";
  return route;
};
