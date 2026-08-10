import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  ChevronDown,
  Folder,
  Fingerprint,
  Headphones,
  Library,
  Mic,
  Pause,
  Radio,
  RefreshCw,
  RotateCcw,
  Settings,
  Square,
  Volume2,
} from "lucide-react";
import { api, type UnlistenFn } from "./api";
import {
  captureTargetLabel,
  preferenceForTarget,
  resolveCaptureTarget,
  type CaptureTargetPreference,
} from "./captureTargets";
import { LevelMeter } from "./components/LevelMeter";
import { RecordingsWorkspace } from "./components/RecordingsWorkspace";
import { SettingsWorkspace } from "./components/SettingsWorkspace";
import { VoiceprintsWorkspace } from "./components/VoiceprintsWorkspace";
import {
  enqueueToast,
  ToastRegion,
  type AppToast,
  type ToastTone,
} from "./components/ToastRegion";
import type {
  AecMode,
  AppSettings,
  AsrProvider,
  AsrProviderProbeRequest,
  AudioDevice,
  CaptureSelection,
  CaptureTarget,
  DeviceSelection,
  LevelEvent,
  LlmProvider,
  LlmProviderProbeRequest,
  ParticipantProfile,
  RecordingItem,
  RecordingSnapshot,
  TranscriptDocument,
} from "./types";

const defaultSnapshot: RecordingSnapshot = {
  sessionId: null,
  state: "idle",
  startedAt: null,
  activeDurationMs: 0,
  bytesWritten: 0,
  outputPath: null,
  system: { healthy: false, label: "系统声音" },
  microphone: { healthy: false, label: "麦克风" },
  microphoneSelection: null,
  aecStatus: "disabled",
  fault: null,
};

const defaultSettings: AppSettings = {
  outputDirectory: "",
  aiDocumentsDirectory: "",
  aecMode: "auto",
  microphoneEnabled: true,
  firstRunComplete: false,
  shortcutsEnabled: true,
  toggleShortcut: "Ctrl+Alt+F9",
  stopShortcut: "Ctrl+Alt+F10",
  activeAsrProviderId: null,
  voiceprintProviderId: null,
  autoTranscribe: false,
  activeLlmProviderId: null,
};

const formatElapsed = (milliseconds: number) => {
  const seconds = Math.floor(milliseconds / 1000);
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const rest = seconds % 60;
  return [hours, minutes, rest].map((part) => part.toString().padStart(2, "0")).join(":");
};

const isActive = (state: RecordingSnapshot["state"]) =>
  ["preparing", "recording", "paused", "interrupted", "finalizing"].includes(state);

const followDefaultDeviceLabel = (device?: AudioDevice) =>
  `跟随默认通信设备（${device?.name ?? "当前不可用"}）`;

const microphoneSelectionValue = (selection: DeviceSelection | null) => {
  if (!selection) return "off";
  return selection.kind === "followDefaultCommunications"
    ? "default"
    : selection.endpointId;
};

const microphoneSelectionFromValue = (value: string): DeviceSelection | null => {
  if (value === "off") return null;
  return value === "default"
    ? { kind: "followDefaultCommunications" }
    : { kind: "fixed", endpointId: value };
};

type CaptureMode = "process" | "system";
type StartRequestMode = CaptureMode | "current";
type AppPage = "recorder" | "recordings" | "voiceprints" | "settings";
type RecordingDeleteRequest = { id: string; permanent: boolean };

export default function App() {
  const [targets, setTargets] = useState<CaptureTarget[]>([]);
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  const [settings, setSettings] = useState(defaultSettings);
  const [captureMode, setCaptureMode] = useState<CaptureMode>("process");
  const [targetId, setTargetId] = useState("");
  const [targetsRefreshing, setTargetsRefreshing] = useState(false);
  const [renderDeviceId, setRenderDeviceId] = useState("default");
  const [micDeviceId, setMicDeviceId] = useState("default");
  const [snapshot, setSnapshot] = useState(defaultSnapshot);
  const [microphoneSwitching, setMicrophoneSwitching] = useState(false);
  const [levels, setLevels] = useState<LevelEvent>({ system: 0, microphone: 0 });
  const [recordings, setRecordings] = useState<RecordingItem[]>([]);
  const [recoverable, setRecoverable] = useState<RecordingItem[]>([]);
  const [providers, setProviders] = useState<AsrProvider[]>([]);
  const [llmProviders, setLlmProviders] = useState<LlmProvider[]>([]);
  const [participants, setParticipants] = useState<ParticipantProfile[]>([]);
  const [participantsLoading, setParticipantsLoading] = useState(false);
  const [page, setPage] = useState<AppPage>("recorder");
  const [selectedRecordingId, setSelectedRecordingId] = useState<string | null>(null);
  const [transcript, setTranscript] = useState<TranscriptDocument | null>(null);
  const [transcriptLoading, setTranscriptLoading] = useState(false);
  const [recordingDeleteRequest, setRecordingDeleteRequest] = useState<RecordingDeleteRequest | null>(null);
  const [deleteAiDocuments, setDeleteAiDocuments] = useState(false);
  const [recordingDeleteBusy, setRecordingDeleteBusy] = useState(false);
  const [toasts, setToasts] = useState<AppToast[]>([]);
  const [appVersion, setAppVersion] = useState("…");
  const [draftSettings, setDraftSettings] = useState(defaultSettings);
  const targetIdRef = useRef("");
  const targetPreferenceRef = useRef<CaptureTargetPreference | null>(null);
  const refreshTargetsPromiseRef = useRef<Promise<CaptureTarget[]> | null>(null);
  const requestStartRef = useRef<(mode?: StartRequestMode) => void>(() => undefined);
  const lastCompletedSessionRef = useRef<string | null>(null);
  const selectedRecordingIdRef = useRef<string | null>(null);
  const nextToastIdRef = useRef(1);
  const seenFaultKeysRef = useRef(new Set<string>());
  const settingsDirty = useMemo(
    () => JSON.stringify(draftSettings) !== JSON.stringify(settings),
    [draftSettings, settings],
  );

  const showToast = useCallback(
    (
      tone: ToastTone,
      message: string,
      options?: {
        durationMs?: number | null;
        dedupeKey?: string;
        action?: AppToast["action"];
      },
    ) => {
      const durationMs =
        options?.durationMs === undefined
          ? tone === "success" || tone === "info"
            ? 3_000
            : null
          : options.durationMs;
      const toast: AppToast = {
        id: nextToastIdRef.current++,
        tone,
        message,
        durationMs,
        dedupeKey: options?.dedupeKey,
        action: options?.action,
      };
      setToasts((current) => enqueueToast(current, toast));
    },
    [],
  );

  const showError = useCallback(
    (error: unknown, dedupeKey?: string) => {
      showToast("error", String(error), { dedupeKey });
    },
    [showToast],
  );

  const handleAiMessage = useCallback(
    (type: "success" | "error", message: string) => showToast(type, message),
    [showToast],
  );

  const applySnapshot = useCallback(
    (next: RecordingSnapshot) => {
      setSnapshot(next);
      if (next.fault) {
        const faultKey = `recording-fault:${next.fault.occurredAt}:${next.fault.code}`;
        if (!seenFaultKeysRef.current.has(faultKey)) {
          if (seenFaultKeysRef.current.size >= 100) {
            seenFaultKeysRef.current.clear();
          }
          seenFaultKeysRef.current.add(faultKey);
          showToast("error", next.fault.userMessage, {
            dedupeKey: faultKey,
          });
        }
      }
    },
    [showToast],
  );

  const dismissToast = useCallback((id: number) => {
    setToasts((current) => current.filter((toast) => toast.id !== id));
  }, []);

  const refreshLibrary = useCallback(async (options?: { clearSelectionId?: string }) => {
    const [items, recoverableItems] = await Promise.all([
      api.listRecordings(),
      api.listRecoverable(),
    ]);
    setRecordings(items);
    setRecoverable(recoverableItems);
    setSelectedRecordingId((current) => {
      if (current && current === options?.clearSelectionId) return null;
      return current && items.some((item) => item.id === current)
        ? current
        : items[0]?.id ?? null;
    });
  }, []);

  const refreshProviders = useCallback(async () => {
    const next = await api.listAsrProviders();
    setProviders(next);
    return next;
  }, []);

  const refreshLlmProviders = useCallback(async () => {
    const next = await api.listLlmProviders();
    setLlmProviders(next);
    return next;
  }, []);

  const refreshParticipants = useCallback(async () => {
    setParticipantsLoading(true);
    try {
      const next = await api.listParticipants();
      setParticipants(next);
      return next;
    } finally {
      setParticipantsLoading(false);
    }
  }, []);

  const applyCaptureTargets = useCallback((nextTargets: CaptureTarget[]) => {
    const selected = resolveCaptureTarget(
      nextTargets,
      targetIdRef.current,
      targetPreferenceRef.current,
    );
    setTargets(nextTargets);
    targetIdRef.current = selected?.id ?? "";
    setTargetId(selected?.id ?? "");
    if (selected) {
      targetPreferenceRef.current = preferenceForTarget(selected);
    }
    return selected;
  }, []);

  const refreshTargets = useCallback(() => {
    if (refreshTargetsPromiseRef.current) {
      return refreshTargetsPromiseRef.current;
    }
    setTargetsRefreshing(true);
    const request = api
      .listCaptureTargets()
      .then((nextTargets) => {
        applyCaptureTargets(nextTargets);
        return nextTargets;
      })
      .finally(() => {
        refreshTargetsPromiseRef.current = null;
        setTargetsRefreshing(false);
      });
    refreshTargetsPromiseRef.current = request;
    return request;
  }, [applyCaptureTargets]);

  const refreshDevices = useCallback(() => {
    return api.listAudioDevices().then((nextDevices) => {
      setDevices(nextDevices);
      return nextDevices;
    });
  }, []);

  useEffect(() => {
    let mounted = true;
    let unlistenSnapshot: UnlistenFn | undefined;
    let unlistenLevels: UnlistenFn | undefined;
    let unlistenStart: UnlistenFn | undefined;
    let unlistenExit: UnlistenFn | undefined;
    let unlistenAsr: UnlistenFn | undefined;
    void Promise.all([
      refreshTargets(),
      refreshDevices(),
      api.getSettings(),
      api.getSnapshot(),
      api.getAppVersion().catch(() => "未知"),
      api.listAsrProviders(),
      api.listLlmProviders(),
      api.listParticipants(),
    ])
      .then(async ([targetList, deviceList, savedSettings, current, version, savedProviders, savedLlmProviders, savedParticipants]) => {
        if (!mounted) return;
        applyCaptureTargets(targetList);
        setDevices(deviceList);
        setSettings(savedSettings);
        setDraftSettings(savedSettings);
        if (!savedSettings.firstRunComplete) {
          setPage("settings");
        }
        if (current.state === "completed") {
          lastCompletedSessionRef.current = current.sessionId;
        }
        applySnapshot(current);
        setAppVersion(version);
        setProviders(savedProviders);
        setLlmProviders(savedLlmProviders);
        setParticipants(savedParticipants);
        await refreshLibrary();
        unlistenSnapshot = await api.onSnapshot(applySnapshot);
        unlistenLevels = await api.onLevels(setLevels);
        unlistenStart = await api.onRequestStart((mode) => requestStartRef.current(mode));
        unlistenExit = await api.onRequestExit(() => {
          if (confirm("录音、语音转写或 AI 文档任务仍在进行。中断任务（录音会先保存）后退出应用？")) {
            void api.quitApplication(true);
          }
        });
        unlistenAsr = await api.onAsrStatus((event) => {
          setRecordings((currentItems) =>
            currentItems.map((item) =>
              item.id === event.recordingId
                ? { ...item, transcription: event.summary }
                : item,
            ),
          );
          if (event.recordingId === selectedRecordingIdRef.current && event.summary.status === "completed") {
            void api.getTranscript(event.recordingId).then(setTranscript).catch(() => undefined);
          }
        });
      })
      .catch(showError);
    return () => {
      mounted = false;
      unlistenSnapshot?.();
      unlistenLevels?.();
      unlistenStart?.();
      unlistenExit?.();
      unlistenAsr?.();
    };
  }, [
    applyCaptureTargets,
    applySnapshot,
    refreshDevices,
    refreshLibrary,
    refreshTargets,
    showError,
  ]);

  useEffect(() => {
    if (
      snapshot.state !== "completed" ||
      !snapshot.sessionId ||
      lastCompletedSessionRef.current === snapshot.sessionId
    ) {
      return;
    }
    lastCompletedSessionRef.current = snapshot.sessionId;
    const recordingId = snapshot.sessionId;
    void refreshLibrary()
      .then(async () => {
        if (settings.autoTranscribe && settings.activeAsrProviderId) {
          await api.startTranscription(recordingId, settings.activeAsrProviderId, null);
          showToast("success", "录音已保存，并已加入语音转写队列。");
        } else {
          showToast(
            "success",
            "录音已安全保存。可前往“录音记录”播放或开始转写。",
          );
        }
      })
      .catch(showError);
  }, [
    refreshLibrary,
    settings.activeAsrProviderId,
    settings.autoTranscribe,
    snapshot.sessionId,
    snapshot.state,
    showError,
    showToast,
  ]);

  useEffect(() => {
    selectedRecordingIdRef.current = selectedRecordingId;
    const item = recordings.find((candidate) => candidate.id === selectedRecordingId);
    if (!item?.transcription) {
      setTranscript(null);
      return;
    }
    setTranscriptLoading(true);
    void api
      .getTranscript(item.id)
      .then(setTranscript)
      .catch(() => setTranscript(null))
      .finally(() => setTranscriptLoading(false));
  }, [recordings, selectedRecordingId]);

  useEffect(() => {
    const refreshOnFocus = () => {
      void refreshDevices().catch(() => undefined);
      if (!isActive(snapshot.state)) {
        void refreshTargets().catch(() => undefined);
      }
    };
    const refreshWhenVisible = () => {
      if (document.visibilityState === "visible") refreshOnFocus();
    };
    const timer = window.setInterval(
      () => void refreshDevices().catch(() => undefined),
      5_000,
    );
    window.addEventListener("focus", refreshOnFocus);
    document.addEventListener("visibilitychange", refreshWhenVisible);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener("focus", refreshOnFocus);
      document.removeEventListener("visibilitychange", refreshWhenVisible);
    };
  }, [refreshDevices, refreshTargets, snapshot.state]);

  const selectedTarget = targets.find((target) => target.id === targetId);
  const renderDevices = devices.filter((device) => device.direction === "render");
  const micDevices = devices.filter((device) => device.direction === "capture");
  const defaultRenderDevice = renderDevices.find(
    (device) => device.isDefaultCommunications,
  );
  const defaultMicDevice = micDevices.find(
    (device) => device.isDefaultCommunications,
  );
  const defaultRenderLabel = followDefaultDeviceLabel(defaultRenderDevice);
  const defaultMicLabel = followDefaultDeviceLabel(defaultMicDevice);

  const systemCapture = useMemo<CaptureSelection>(
    () => ({
      kind: "system",
      device:
        renderDeviceId === "default"
          ? { kind: "followDefaultCommunications" }
          : { kind: "fixed", endpointId: renderDeviceId },
    }),
    [renderDeviceId],
  );

  const sourceDescription =
    captureMode === "process"
      ? selectedTarget?.displayName ?? "未选择应用"
      : renderDeviceId === "default"
        ? `全部系统声音 · ${defaultRenderLabel}`
        : `全部系统声音 · ${renderDevices.find((device) => device.id === renderDeviceId)?.name ?? ""}`;

  const selectTarget = (
    nextTargetId: string,
    knownTarget?: CaptureTarget,
  ) => {
    const target =
      knownTarget ??
      targets.find((candidate) => candidate.id === nextTargetId);
    targetIdRef.current = nextTargetId;
    targetPreferenceRef.current = target ? preferenceForTarget(target) : null;
    setTargetId(nextTargetId);
  };

  const start = async (requestedCapture: CaptureSelection) => {
    try {
      const next = await api.startRecording({
        capture: requestedCapture,
        microphone: settings.microphoneEnabled
          ? micDeviceId === "default"
            ? { kind: "followDefaultCommunications" }
            : { kind: "fixed", endpointId: micDeviceId }
          : null,
        aecMode: settings.aecMode,
        outputDirectory: settings.outputDirectory,
      });
      applySnapshot(next);
    } catch (error) {
      showError(error);
    }
  };

  const requestStart = async (requestedMode: CaptureMode = captureMode) => {
    try {
      let requestedCapture: CaptureSelection;
      setCaptureMode(requestedMode);

      if (requestedMode === "process") {
        const refreshedTargets = await refreshTargets();
        const target = resolveCaptureTarget(
          refreshedTargets,
          targetIdRef.current,
          targetPreferenceRef.current,
        );
        if (!target) {
          showToast(
            "warning",
            "没有找到可录制的应用。请启动会议应用后刷新并选择录音来源。",
          );
          return;
        }
        selectTarget(target.id, target);
        requestedCapture = { kind: "process", targetId: target.id };
      } else {
        await refreshDevices();
        requestedCapture = systemCapture;
      }
      await start(requestedCapture);
    } catch (error) {
      showError(error);
    }
  };
  requestStartRef.current = (mode = "current") => {
    void requestStart(mode === "current" ? captureMode : mode);
  };

  const chooseOutput = async () => {
    try {
      const selected = await open({ directory: true, multiple: false });
      if (typeof selected !== "string") return;
      const next = { ...settings, outputDirectory: selected };
      await api.saveSettings(next);
      setSettings(next);
    } catch (error) {
      showError(error);
    }
  };

  const chooseDraftOutput = async () => {
    try {
      const selected = await open({ directory: true, multiple: false });
      if (typeof selected === "string") {
        setDraftSettings((current) => ({
          ...current,
          outputDirectory: selected,
        }));
      }
    } catch (error) {
      showError(error);
    }
  };

  const chooseDraftAiDocuments = async () => {
    try {
      const selected = await open({ directory: true, multiple: false });
      if (typeof selected === "string") {
        setDraftSettings((current) => ({
          ...current,
          aiDocumentsDirectory: selected,
        }));
      }
    } catch (error) {
      showError(error);
    }
  };

  const navigateTo = (nextPage: AppPage) => {
    if (nextPage === page) return;
    if (
      page === "settings" &&
      settingsDirty &&
      !confirm("设置尚未保存。放弃这些更改并离开设置页吗？")
    ) {
      return;
    }
    if (page === "settings" && settingsDirty) {
      setDraftSettings(settings);
    }
    if (nextPage === "settings") {
      setDraftSettings(settings);
    }
    setPage(nextPage);
  };

  const saveDraftSettings = async () => {
    const next = {
      ...draftSettings,
      firstRunComplete: true,
      toggleShortcut: draftSettings.toggleShortcut.trim(),
      stopShortcut: draftSettings.stopShortcut.trim(),
    };
    try {
      await api.saveSettings(next);
      setSettings(next);
      setDraftSettings(next);
      showToast("success", "设置已保存");
    } catch (error) {
      showError(error);
    }
  };

  const skipFirstRun = async () => {
    const next = {
      ...settings,
      firstRunComplete: true,
    };
    try {
      await api.saveSettings(next);
      setSettings(next);
      setDraftSettings(next);
      setPage("recorder");
      showToast("info", "已跳过首次设置，可以随时从侧边栏返回。");
    } catch (error) {
      showError(error);
    }
  };

  const saveProvider = async (request: Parameters<typeof api.saveAsrProvider>[0]) => {
    const saved = await api.saveAsrProvider(request);
    const [nextSettings] = await Promise.all([api.getSettings(), refreshProviders()]);
    setSettings(nextSettings);
    setDraftSettings((current) => ({
      ...current,
      voiceprintProviderId: nextSettings.voiceprintProviderId,
    }));
    return saved;
  };

  const deleteProvider = async (id: string) => {
    await api.deleteAsrProvider(id);
    const [nextSettings] = await Promise.all([api.getSettings(), refreshProviders()]);
    setSettings(nextSettings);
    setDraftSettings((current) => ({
      ...current,
      activeAsrProviderId:
        current.activeAsrProviderId === id
          ? nextSettings.activeAsrProviderId
          : current.activeAsrProviderId,
      voiceprintProviderId:
        current.voiceprintProviderId === id
          ? nextSettings.voiceprintProviderId
          : current.voiceprintProviderId,
      autoTranscribe:
        current.activeAsrProviderId === id
          ? nextSettings.autoTranscribe
          : current.autoTranscribe,
    }));
  };

  const saveLlmProvider = async (request: Parameters<typeof api.saveLlmProvider>[0]) => {
    const saved = await api.saveLlmProvider(request);
    await refreshLlmProviders();
    return saved;
  };

  const deleteLlmProvider = async (id: string) => {
    await api.deleteLlmProvider(id);
    const [nextSettings] = await Promise.all([api.getSettings(), refreshLlmProviders()]);
    setSettings(nextSettings);
    setDraftSettings((current) => ({
      ...current,
      activeLlmProviderId:
        current.activeLlmProviderId === id
          ? nextSettings.activeLlmProviderId
          : current.activeLlmProviderId,
    }));
  };

  const setVoiceprintProvider = async (id: string | null) => {
    const next = { ...settings, voiceprintProviderId: id };
    try {
      await api.saveSettings(next);
      setSettings(next);
      setDraftSettings((current) => ({ ...current, voiceprintProviderId: id }));
      showToast("success", id ? "声纹提取服务已更新" : "已清除声纹提取服务");
    } catch (error) {
      showError(error);
    }
  };

  const updateTranscriptionSummary = (
    recordingId: string,
    summary: NonNullable<RecordingItem["transcription"]>,
  ) => {
    setRecordings((current) =>
      current.map((item) =>
        item.id === recordingId ? { ...item, transcription: summary } : item,
      ),
    );
  };

  const startTranscription = async (
    recordingId: string,
    speakerCount: number | null,
  ) => {
    try {
      const summary = await api.startTranscription(
        recordingId,
        settings.activeAsrProviderId,
        speakerCount,
      );
      updateTranscriptionSummary(recordingId, summary);
    } catch (error) {
      showError(error);
    }
  };

  const resumeTranscription = async (recordingId: string) => {
    try {
      updateTranscriptionSummary(
        recordingId,
        await api.resumeTranscription(recordingId),
      );
    } catch (error) {
      showError(error);
    }
  };

  const cancelTranscription = async (recordingId: string) => {
    try {
      updateTranscriptionSummary(
        recordingId,
        await api.cancelTranscription(recordingId),
      );
    } catch (error) {
      showError(error);
    }
  };

  const exportTranscript = async (recordingId: string, title: string) => {
    const path = await save({
      defaultPath: `${title}.txt`,
      filters: [{ name: "Text", extensions: ["txt"] }],
    });
    if (!path) return;
    try {
      await api.exportTranscript(recordingId, path);
      showToast("success", "转写文字已导出", {
        durationMs: 8_000,
        action: {
          label: "打开文件夹",
          onClick: () => {
            void api.revealTranscriptExport(path).catch(showError);
          },
        },
      });
    } catch (error) {
      showError(error);
    }
  };

  const switchLiveMicrophone = async (value: string) => {
    if (microphoneSwitching) return;
    setMicrophoneSwitching(true);
    try {
      const selection = microphoneSelectionFromValue(value);
      const nextSnapshot = await api.setMicrophoneEnabled(selection);
      applySnapshot(nextSnapshot);
      if (value !== "off") {
        setMicDeviceId(value);
      }
      const deviceName =
        value === "default"
          ? defaultMicLabel
          : micDevices.find((device) => device.id === value)?.name;
      showToast(
        "success",
        value === "off"
          ? "已关闭本次录音的麦克风"
          : `麦克风已切换至 ${deviceName ?? "所选设备"}`,
      );
    } catch (error) {
      try {
        applySnapshot(await api.getSnapshot());
      } catch {
        // Keep the last known snapshot when backend state cannot be refreshed.
      }
      showError(error);
    } finally {
      setMicrophoneSwitching(false);
    }
  };

  const togglePause = async () => {
    try {
      applySnapshot(
        snapshot.state === "paused"
          ? await api.resumeRecording()
          : await api.pauseRecording(),
      );
    } catch (error) {
      showError(error);
    }
  };

  const stopAndSave = async () => {
    try {
      applySnapshot(await api.stopRecording());
    } catch (error) {
      showError(error);
    }
  };

  const aecLabels: Record<AecMode, string> = {
    auto: "AEC 自动",
    on: "AEC 开启",
    off: "AEC 关闭",
  };
  const liveMicrophoneValue = microphoneSelectionValue(snapshot.microphoneSelection);
  const liveFixedMicrophoneId =
    snapshot.microphoneSelection?.kind === "fixed"
      ? snapshot.microphoneSelection.endpointId
      : null;
  const missingLiveMicrophone =
    liveFixedMicrophoneId !== null &&
    !micDevices.some((device) => device.id === liveFixedMicrophoneId);
  const canSwitchLiveMicrophone = ["recording", "paused", "interrupted"].includes(
    snapshot.state,
  );

  return (
    <div className="app-shell">
      <ToastRegion toasts={toasts} onDismiss={dismissToast} />
      <aside className="app-sidebar">
        <div className="sidebar-brand" title="Nota"><Radio size={20} /></div>
        <button
          className={`sidebar-item ${page === "recorder" ? "active" : ""}`}
          onClick={() => navigateTo("recorder")}
        >
          <Mic size={20} />
          <span>录音</span>
          {isActive(snapshot.state) && <i className="sidebar-recording-dot" />}
        </button>
        <button
          className={`sidebar-item ${page === "recordings" ? "active" : ""}`}
          onClick={() => navigateTo("recordings")}
        >
          <Library size={20} />
          <span>录音记录</span>
          {recoverable.length > 0 && <b>{recoverable.length}</b>}
        </button>
        <button
          className={`sidebar-item ${page === "voiceprints" ? "active" : ""}`}
          onClick={() => navigateTo("voiceprints")}
        >
          <Fingerprint size={20} />
          <span>声纹管理</span>
        </button>
        <button
          className={`sidebar-item sidebar-settings ${page === "settings" ? "active" : ""}`}
          aria-label="设置"
          onClick={() => navigateTo("settings")}
        >
          <Settings size={19} />
          <span>设置</span>
        </button>
      </aside>

      <div className="app-content">
        <header className="topbar">
          <div>
            <strong>
              {page === "recorder"
                ? "录音"
                : page === "recordings"
                  ? "录音记录"
                  : page === "voiceprints"
                    ? "声纹管理"
                    : "设置"}
            </strong>
            <span>
              {page === "recorder"
                ? "捕捉会议声音与麦克风"
                : page === "recordings"
                  ? "播放录音并查看文字转写"
                  : page === "voiceprints"
                    ? "管理本地参会人姓名与声纹样本"
                    : "管理录音偏好与语音转写服务"}
            </span>
          </div>
          <span className="local-pill">本地优先</span>
        </header>

      <main className={`page-content page-${page}`}>
        {page === "recorder" && (
          <>
          {recoverable.length > 0 && (
            <button className="global-recovery" onClick={() => setPage("recordings")}>
              <RotateCcw size={16} />
              发现 {recoverable.length} 个未完成录音，前往录音记录恢复
            </button>
          )}
        <section className={`recorder-card state-${snapshot.state}`}>
          <div className="recorder-header">
            <div>
              <p className="eyebrow">MEETING RECORDER</p>
              <h1>{isActive(snapshot.state) ? "会议录音进行中" : "准备好记录会议"}</h1>
              <p className="muted">
                {isActive(snapshot.state)
                  ? sourceDescription
                  : "选择会议应用，录音只会保存在这台电脑上。"}
              </p>
            </div>
            {isActive(snapshot.state) && (
              <div className="recording-time">
                <span className={`live-dot ${snapshot.state === "paused" ? "paused" : ""}`} />
                <strong>{formatElapsed(snapshot.activeDurationMs)}</strong>
              </div>
            )}
          </div>

          {!isActive(snapshot.state) ? (
            <>
              <div className="source-tabs">
                <button
                  className={captureMode === "process" ? "active" : ""}
                  onClick={() => {
                    setCaptureMode("process");
                    void refreshTargets().catch(showError);
                  }}
                >
                  指定应用
                </button>
                <button
                  className={captureMode === "system" ? "active" : ""}
                  onClick={() => setCaptureMode("system")}
                >
                  全部系统声音
                </button>
              </div>

              <div className="source-grid">
                <div className="field">
                  <label htmlFor="recording-source"><Volume2 size={16} />录音来源</label>
                  {captureMode === "process" ? (
                    <div className="target-picker">
                      <div className="select-wrap">
                        <select
                          id="recording-source"
                          value={targetId}
                          onFocus={() => void refreshTargets().catch(() => undefined)}
                          onPointerDown={() => void refreshTargets().catch(() => undefined)}
                          onChange={(event) => selectTarget(event.target.value)}
                        >
                          {!targetId && (
                            <option value="">
                              {targetPreferenceRef.current
                                ? `${captureTargetLabel(targetPreferenceRef.current)}（未运行）`
                                : "请选择要录制的应用"}
                            </option>
                          )}
                        {targets.map((target) => (
                          <option key={target.id} value={target.id}>
                            {captureTargetLabel(target)}
                          </option>
                        ))}
                        </select>
                        <ChevronDown size={16} />
                      </div>
                      <button
                        type="button"
                        className="refresh-targets"
                        aria-label="刷新应用列表"
                        title="刷新应用列表"
                        disabled={targetsRefreshing}
                        onClick={() =>
                          void refreshTargets().catch(showError)
                        }
                      >
                        <RefreshCw size={16} className={targetsRefreshing ? "spinning" : ""} />
                      </button>
                    </div>
                  ) : (
                    <div className="select-wrap">
                      <select
                        id="recording-source"
                        value={renderDeviceId}
                        onChange={(e) => setRenderDeviceId(e.target.value)}
                      >
                        <option value="default">{defaultRenderLabel}</option>
                        {renderDevices.map((device) => (
                          <option key={device.id} value={device.id}>{device.name}</option>
                        ))}
                      </select>
                      <ChevronDown size={16} />
                    </div>
                  )}
                </div>
                <label className="field">
                  <span><Mic size={16} />我的麦克风</span>
                  <div className="select-wrap">
                    <select
                      value={settings.microphoneEnabled ? micDeviceId : "off"}
                      onChange={(e) => {
                        const enabled = e.target.value !== "off";
                        const next = { ...settings, microphoneEnabled: enabled };
                        setSettings(next);
                        void api.saveSettings(next);
                        if (enabled) setMicDeviceId(e.target.value);
                      }}
                    >
                      <option value="default">{defaultMicLabel}</option>
                      {micDevices.map((device) => (
                        <option key={device.id} value={device.id}>{device.name}</option>
                      ))}
                      <option value="off">不录制麦克风</option>
                    </select>
                    <ChevronDown size={16} />
                  </div>
                </label>
              </div>

              {selectedTarget?.browser && captureMode === "process" && (
                <div className="inline-warning">
                  <AlertTriangle size={17} />
                  将录制此浏览器的全部声音，而不只是当前会议标签页。
                </div>
              )}
              {captureMode === "system" && (
                <div className="inline-warning">
                  <AlertTriangle size={17} />
                  系统通知和其他应用声音也会被录入，建议开启 Windows“勿扰”。
                </div>
              )}
            </>
          ) : (
            <div className="live-panel">
              <LevelMeter
                label="会议声音"
                value={levels.system}
                healthy={snapshot.system.healthy}
              />
              <LevelMeter
                label="我的麦克风"
                value={levels.microphone}
                healthy={snapshot.microphone.healthy}
              />
              <div className="live-microphone-control">
                <label htmlFor="live-microphone"><Mic size={15} />本次录音麦克风</label>
                <div className="select-wrap">
                  <select
                    id="live-microphone"
                    value={liveMicrophoneValue}
                    disabled={!canSwitchLiveMicrophone || microphoneSwitching}
                    onChange={(event) => void switchLiveMicrophone(event.target.value)}
                  >
                    <option value="default">{defaultMicLabel}</option>
                    {missingLiveMicrophone && liveFixedMicrophoneId && (
                      <option value={liveFixedMicrophoneId}>
                        当前麦克风（设备暂不可用）
                      </option>
                    )}
                    {micDevices.map((device) => (
                      <option key={device.id} value={device.id}>{device.name}</option>
                    ))}
                    <option value="off">不录制麦克风</option>
                  </select>
                  <ChevronDown size={16} />
                </div>
                <span className={snapshot.microphone.healthy ? "health-ok" : "health-bad"}>
                  {microphoneSwitching
                    ? "正在切换…"
                    : snapshot.microphoneSelection
                      ? snapshot.microphone.healthy
                        ? snapshot.microphone.detail ?? "正在录制"
                        : "设备暂不可用，正在等待恢复"
                      : "本次录音已关闭"}
                </span>
              </div>
              <div className="live-status">
                <span><Headphones size={16} />{aecLabels[settings.aecMode]}</span>
                <span>{(snapshot.bytesWritten / 1024 / 1024).toFixed(1)} MB</span>
              </div>
            </div>
          )}

          <div className="recorder-footer">
            <button className="folder-choice" onClick={chooseOutput} title={settings.outputDirectory}>
              <Folder size={17} />
              <span>{settings.outputDirectory || "默认录音目录"}</span>
            </button>
            {!isActive(snapshot.state) ? (
              <button
                className="record-button"
                disabled={captureMode === "process" && !targetId}
                onClick={() => void requestStart(captureMode)}
              >
                <span className="record-dot" />开始录音
              </button>
            ) : (
              <div className="recording-actions">
                <button
                  className="button secondary"
                  disabled={snapshot.state === "preparing" || snapshot.state === "finalizing"}
                  onClick={() => void togglePause()}
                >
                  {snapshot.state === "paused" ? <Radio size={16} /> : <Pause size={16} />}
                  {snapshot.state === "paused" ? "继续" : "暂停"}
                </button>
                <button
                  className="button stop"
                  disabled={snapshot.state === "finalizing"}
                  onClick={() => void stopAndSave()}
                >
                  <Square size={14} fill="currentColor" />停止并保存
                </button>
              </div>
            )}
          </div>
        </section>
        </>
        )}

        {page === "recordings" && (
        <RecordingsWorkspace
          items={recordings}
          recoverable={recoverable}
          selectedId={selectedRecordingId}
          transcript={transcript}
          transcriptLoading={transcriptLoading}
          recordingActive={isActive(snapshot.state)}
          hasProvider={providers.some(
            (provider) => provider.id === settings.activeAsrProviderId,
          )}
          activeProviderKind={providers.find(
            (provider) => provider.id === settings.activeAsrProviderId,
          )?.kind ?? null}
          hasVoiceprintProvider={providers.some(
            (provider) => provider.id === settings.voiceprintProviderId
              && provider.kind === "funAsr",
          )}
          llmProviders={llmProviders}
          activeLlmProviderId={settings.activeLlmProviderId}
          participants={participants}
          onSelect={setSelectedRecordingId}
          onReturnToRecorder={() => navigateTo("recorder")}
          onPreparePlayback={(id) => api.prepareRecordingPlayback(id)}
          onPlaybackError={(message) => showToast("error", message)}
          onStartTranscription={(id, speakerCount) =>
            void startTranscription(id, speakerCount)
          }
          onResumeTranscription={(id) => void resumeTranscription(id)}
          onCancelTranscription={(id) => void cancelTranscription(id)}
          onCopyTranscript={(id) =>
            void api
              .copyTranscript(id)
              .then(() => showToast("success", "转写全文已复制"))
              .catch(showError)
          }
          onExportTranscript={(id, title) => void exportTranscript(id, title)}
          onIdentifySpeakers={(id) =>
            api.identifyRecordingSpeakers(id, settings.voiceprintProviderId)
          }
          onSaveSpeakerIdentification={async (sessionId, assignments) => {
            const updated = await api.saveSpeakerIdentification(sessionId, assignments);
            setTranscript(updated);
            await refreshParticipants();
            showToast("success", "声纹与当前会议的说话人姓名已保存");
          }}
          onUpdateSpeakerAssignments={async (recordingId, assignments) => {
            const updated = await api.updateRecordingSpeakerAssignments(recordingId, assignments);
            setTranscript(updated);
            await refreshParticipants();
            showToast("success", "当前会议的说话人姓名已更新");
          }}
          onDiscardSpeakerIdentification={(sessionId) => {
            void api.discardSpeakerIdentification(sessionId);
          }}
          onOpenVoiceprintSettings={() => navigateTo("voiceprints")}
          onReveal={(id) => void api.revealRecording(id).catch(showError)}
          onDelete={(id) => {
            setDeleteAiDocuments(false);
            setRecordingDeleteRequest({ id, permanent: false });
          }}
          onRecover={(id) =>
            void api.recoverRecording(id).then(() => refreshLibrary()).catch(showError)
          }
          onDiscardRecovery={(id) => {
            if (!confirm("永久删除这个未完成的恢复文件？此操作无法撤销。")) return;
            void api.deleteRecoverable(id).then(() => refreshLibrary()).catch(showError);
          }}
          onRename={(id, currentTitle) => {
            const title = prompt("输入新的录音名称", currentTitle);
            if (!title || title === currentTitle) return;
            void api
              .renameRecording(id, title)
              .then(() => refreshLibrary())
              .catch(showError);
          }}
          onPermanentDelete={(id) => {
            setDeleteAiDocuments(false);
            setRecordingDeleteRequest({ id, permanent: true });
          }}
          onAiMessage={handleAiMessage}
        />
        )}

        {page === "voiceprints" && (
          <VoiceprintsWorkspace
            participants={participants}
            providers={providers}
            providerId={settings.voiceprintProviderId}
            loading={participantsLoading}
            onProviderChange={(id) => void setVoiceprintProvider(id)}
            onPreparePlayback={(id) => api.prepareRecordingPlayback(id)}
            onError={(message) => showToast("error", message)}
            onRename={(id, displayName) => {
              void api
                .renameParticipant(id, displayName)
                .then(setParticipants)
                .then(() => selectedRecordingId
                  ? api.getTranscript(selectedRecordingId).then(setTranscript).catch(() => undefined)
                  : undefined)
                .catch(showError);
            }}
            onDeleteParticipant={(id) => {
              if (!confirm("删除这个参会人？历史会议将恢复显示原始 speaker 标签，声纹样本也会删除。")) return;
              void api
                .deleteParticipant(id)
                .then(setParticipants)
                .then(() => selectedRecordingId
                  ? api.getTranscript(selectedRecordingId).then(setTranscript).catch(() => undefined)
                  : undefined)
                .catch(showError);
            }}
            onDeleteSample={(id) => {
              if (!confirm("删除这个声纹样本？参会人姓名和历史会议映射会保留。")) return;
              void api.deleteVoiceprint(id).then(setParticipants).catch(showError);
            }}
          />
        )}

        {page === "settings" && (
          <SettingsWorkspace
            firstRun={!settings.firstRunComplete}
            dirty={settingsDirty}
            recordingActive={isActive(snapshot.state)}
            settings={draftSettings}
            providers={providers}
            llmProviders={llmProviders}
            microphoneCount={micDevices.length}
            appVersion={appVersion}
            onChange={setDraftSettings}
            onChooseOutput={() => void chooseDraftOutput()}
            onChooseAiDocuments={() => void chooseDraftAiDocuments()}
            onOpenMicrophoneSettings={() =>
              void api.openMicrophoneSettings().catch(showError)
            }
            onSaveProvider={saveProvider}
            onDeleteProvider={deleteProvider}
            onSaveLlmProvider={saveLlmProvider}
            onDeleteLlmProvider={deleteLlmProvider}
            onTestLlmProvider={(request: LlmProviderProbeRequest) =>
              api.testLlmProvider(request)
            }
            onListLlmModels={(request: LlmProviderProbeRequest) =>
              api.listLlmModels(request)
            }
            onTestProvider={(request: AsrProviderProbeRequest) =>
              api.testAsrProvider(request)
            }
            onListModels={(request: AsrProviderProbeRequest) =>
              api.listAsrModels(request)
            }
            onDiscardChanges={() => setDraftSettings(settings)}
            onSave={() => void saveDraftSettings()}
            onSkipFirstRun={() => void skipFirstRun()}
          />
        )}
      </main>

      {recordingDeleteRequest && (
        <div className="modal-backdrop" role="presentation">
          <section
            className="modal recording-delete-modal"
            role="dialog"
            aria-modal="true"
            aria-label={recordingDeleteRequest.permanent ? "永久删除录音" : "删除录音"}
          >
            <div className="modal-icon"><AlertTriangle size={21} /></div>
            <h3>{recordingDeleteRequest.permanent ? "永久删除这条录音？" : "将这条录音移入回收站？"}</h3>
            <p>
              {recordingDeleteRequest.permanent
                ? "录音文件将被永久删除，无法撤销。"
                : "录音文件将移入 Windows 回收站。"}
            </p>
            <label className="delete-ai-documents-option">
              <input
                type="checkbox"
                checked={deleteAiDocuments}
                onChange={(event) => setDeleteAiDocuments(event.target.checked)}
              />
              <span>
                <strong>同时删除关联的 AI Markdown 文件</strong>
                <small>默认保留，便于继续分享；勾选后只删除 Nota 当前仍能关联到的版本文件。</small>
              </span>
            </label>
            <div className="modal-actions">
              <button
                className="button secondary"
                disabled={recordingDeleteBusy}
                onClick={() => setRecordingDeleteRequest(null)}
              >
                取消
              </button>
              <button
                className="button stop"
                disabled={recordingDeleteBusy}
                onClick={() => {
                  const request = recordingDeleteRequest;
                  setRecordingDeleteBusy(true);
                  void api
                    .deleteRecording(request.id, request.permanent, deleteAiDocuments)
                    .then(() => refreshLibrary({ clearSelectionId: request.id }))
                    .then(() => {
                      setRecordingDeleteRequest(null);
                      showToast("success", request.permanent ? "录音已永久删除" : "录音已移至回收站");
                    })
                    .catch(showError)
                    .finally(() => setRecordingDeleteBusy(false));
                }}
              >
                {recordingDeleteBusy ? "正在删除…" : recordingDeleteRequest.permanent ? "永久删除" : "移入回收站"}
              </button>
            </div>
          </section>
        </div>
      )}

      <footer className="app-footer">
        <span><span className="privacy-dot" />本地录音；仅在转写或手动生成 AI 文档时连接所选服务</span>
        <span>Ctrl + Alt + F9 开始/暂停 · F10 停止</span>
      </footer>
      </div>

    </div>
  );
}
