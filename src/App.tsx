import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  ChevronDown,
  Folder,
  Fingerprint,
  BookMarked,
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
import { AppTooltip } from "./components/AppTooltip";
import { RecordingsWorkspace } from "./components/RecordingsWorkspace";
import {
  SettingsWorkspace,
  type SettingsRoute,
} from "./components/settings";
import { VoiceprintsWorkspace } from "./components/VoiceprintsWorkspace";
import { HotwordLibraryWorkspace } from "./components/HotwordLibraryWorkspace";
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
  AudioDevice,
  AudioImportBatchSnapshot,
  CaptureSelection,
  CaptureTarget,
  DeviceSelection,
  LevelEvent,
  LlmProvider,
  HotwordListSummary,
  ParticipantProfile,
  RecordingItem,
  RecordingSnapshot,
  TranscriptDocument,
  TranscriptionOptions,
  TranscriptionVersionSummary,
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
type AppPage = "recorder" | "recordings" | "hotwords" | "voiceprints" | "settings";
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
  const [audioImport, setAudioImport] = useState<AudioImportBatchSnapshot | null>(null);
  const [recoverable, setRecoverable] = useState<RecordingItem[]>([]);
  const [providers, setProviders] = useState<AsrProvider[]>([]);
  const [hotwordLists, setHotwordLists] = useState<HotwordListSummary[]>([]);
  const [hotwordLibraryDirty, setHotwordLibraryDirty] = useState(false);
  const [activeTranscriptionOptions, setActiveTranscriptionOptions] = useState<TranscriptionOptions | null>(null);
  const [llmProviders, setLlmProviders] = useState<LlmProvider[]>([]);
  const [participants, setParticipants] = useState<ParticipantProfile[]>([]);
  const [participantsLoading, setParticipantsLoading] = useState(false);
  const [page, setPage] = useState<AppPage>("recorder");
  const [selectedRecordingId, setSelectedRecordingId] = useState<string | null>(null);
  const [transcript, setTranscript] = useState<TranscriptDocument | null>(null);
  const [transcriptionVersions, setTranscriptionVersions] = useState<TranscriptionVersionSummary[]>([]);
  const [transcriptLoading, setTranscriptLoading] = useState(false);
  const [recordingDeleteRequest, setRecordingDeleteRequest] = useState<RecordingDeleteRequest | null>(null);
  const [deleteAiDocuments, setDeleteAiDocuments] = useState(false);
  const [recordingDeleteBusy, setRecordingDeleteBusy] = useState(false);
  const [toasts, setToasts] = useState<AppToast[]>([]);
  const [appVersion, setAppVersion] = useState("…");
  const [settingsRoute, setSettingsRoute] = useState<SettingsRoute>("recording");
  const [settingsEditorDirty, setSettingsEditorDirty] = useState(false);
  const targetIdRef = useRef("");
  const targetPreferenceRef = useRef<CaptureTargetPreference | null>(null);
  const refreshTargetsPromiseRef = useRef<Promise<CaptureTarget[]> | null>(null);
  const requestStartRef = useRef<(mode?: StartRequestMode) => void>(() => undefined);
  const lastCompletedSessionRef = useRef<string | null>(null);
  const lastHandledImportRef = useRef<string | null>(null);
  const selectedRecordingIdRef = useRef<string | null>(null);
  const nextToastIdRef = useRef(1);
  const seenFaultKeysRef = useRef(new Set<string>());
  const autoHotwordList = hotwordLists.find(
    (list) => list.id === settings.autoTranscribeHotwordListId,
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

  const applyAudioImportSnapshot = useCallback((next: AudioImportBatchSnapshot) => {
    setAudioImport(next);
    if (next.status === "running") return;
    const terminalKey = `${next.id}:${next.status}`;
    if (lastHandledImportRef.current === terminalKey) return;
    lastHandledImportRef.current = terminalKey;
    const firstAvailableId = next.items.find(
      (item) => ["completed", "skipped"].includes(item.status) && item.recordingId,
    )?.recordingId;
    void refreshLibrary()
      .then(() => {
        if (firstAvailableId) {
          setSelectedRecordingId(firstAvailableId);
          setPage("recordings");
        }
        if (next.status === "cancelled") {
          showToast("info", "音频导入已停止；已完成的录音仍然保留。");
        } else if (next.failed > 0) {
          showToast(
            "warning",
            `导入完成：成功 ${next.completed}，跳过 ${next.skipped}，失败 ${next.failed}。`,
            { durationMs: 6_000 },
          );
        } else if (next.completed === 0 && next.skipped > 0) {
          showToast("info", "所选录音已经导入，无需重复添加。");
        } else {
          showToast(
            "success",
            next.completed === 1
              ? next.skipped > 0
                ? `已导入 1 个录音，另有 ${next.skipped} 个重复文件已跳过。`
                : "录音已导入，可以开始转写。"
              : next.skipped > 0
                ? `已导入 ${next.completed} 个录音，另有 ${next.skipped} 个重复文件已跳过。`
                : `已导入 ${next.completed} 个录音。`,
          );
        }
      })
      .catch(showError);
  }, [refreshLibrary, showError, showToast]);

  const refreshHotwordLists = useCallback(async () => {
    const [next, savedSettings] = await Promise.all([
      api.listHotwordLists(),
      api.getSettings(),
    ]);
    setHotwordLists(next);
    setSettings(savedSettings);
  }, []);

  useEffect(() => {
    if (!settings.activeAsrProviderId || !settings.autoTranscribeHotwordListId) {
      setActiveTranscriptionOptions(null);
      return;
    }
    void api.getTranscriptionOptions(settings.activeAsrProviderId)
      .then(setActiveTranscriptionOptions)
      .catch(() => setActiveTranscriptionOptions(null));
  }, [settings.activeAsrProviderId, settings.autoTranscribeHotwordListId]);

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
    let unlistenImport: UnlistenFn | undefined;
    void Promise.all([
      refreshTargets(),
      refreshDevices(),
      api.getSettings(),
      api.getSnapshot(),
      api.getAppVersion().catch(() => "未知"),
      api.listAsrProviders(),
      api.listLlmProviders(),
      api.listParticipants(),
      api.getAudioImportSnapshot(),
      api.listHotwordLists?.() ?? Promise.resolve([]),
    ])
      .then(async ([targetList, deviceList, savedSettings, current, version, savedProviders, savedLlmProviders, savedParticipants, savedImport, savedHotwordLists]) => {
        if (!mounted) return;
        applyCaptureTargets(targetList);
        setDevices(deviceList);
        setSettings(savedSettings);
        if (!savedSettings.firstRunComplete) {
          setSettingsRoute("setup");
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
        setAudioImport(savedImport);
        setHotwordLists(savedHotwordLists);
        if (savedImport && savedImport.status !== "running") {
          lastHandledImportRef.current = `${savedImport.id}:${savedImport.status}`;
        }
        await refreshLibrary();
        unlistenSnapshot = await api.onSnapshot(applySnapshot);
        unlistenLevels = await api.onLevels(setLevels);
        unlistenStart = await api.onRequestStart((mode) => requestStartRef.current(mode));
        unlistenExit = await api.onRequestExit(() => {
          if (confirm("录音、音频导入、语音转写或 AI 文档任务仍在进行。中断任务（录音会先保存）后退出应用？")) {
            void api.quitApplication(true);
          }
        });
        unlistenImport = await api.onAudioImportStatus(applyAudioImportSnapshot);
        unlistenAsr = await api.onAsrStatus((event) => {
          setRecordings((currentItems) =>
            currentItems.map((item) =>
              item.id === event.recordingId
                ? { ...item, transcription: event.summary }
                : item,
            ),
          );
          if (event.recordingId === selectedRecordingIdRef.current && event.summary.status === "completed") {
            void Promise.all([
              api.getTranscript(event.recordingId),
              api.listTranscriptionVersions(event.recordingId),
            ]).then(([document, versions]) => {
              setTranscript(document);
              setTranscriptionVersions(versions);
            }).catch(() => undefined);
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
      unlistenImport?.();
    };
  }, [
    applyCaptureTargets,
    applySnapshot,
    applyAudioImportSnapshot,
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
          try {
            if (typeof api.listHotwordLists !== "function") {
              await api.startTranscription(recordingId, settings.activeAsrProviderId, null);
            } else {
              await api.startTranscription(
                recordingId,
                settings.activeAsrProviderId,
                null,
                settings.autoTranscribeHotwordListId ?? null,
              );
            }
            showToast("success", "录音已保存，并已加入语音转写队列。");
          } catch (error) {
            showToast(
              "warning",
              `录音已保存，但自动转写未启动：${String(error)}`,
              { durationMs: 8_000 },
            );
          }
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
    settings.autoTranscribeHotwordListId,
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
      setTranscriptionVersions([]);
      return;
    }
    setTranscriptLoading(true);
    void Promise.all([
      api.getTranscript(item.id),
      api.listTranscriptionVersions(item.id),
    ])
      .then(([document, versions]) => {
        setTranscript(document);
        setTranscriptionVersions(versions);
      })
      .catch(() => {
        setTranscript(null);
        setTranscriptionVersions([]);
      })
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

  const chooseAudioFiles = async () => {
    try {
      const selected = await open({
        directory: false,
        multiple: true,
        filters: [{
          name: "会议录音",
          extensions: ["mp3", "m4a", "wav", "flac"],
        }],
      });
      const paths = typeof selected === "string" ? [selected] : selected ?? [];
      if (paths.length === 0) return;
      const next = await api.startAudioImport(paths);
      lastHandledImportRef.current = null;
      setAudioImport(next);
      setPage("recordings");
    } catch (error) {
      showError(error);
    }
  };

  const cancelAudioImport = async () => {
    try {
      setAudioImport(await api.cancelAudioImport());
    } catch (error) {
      showError(error);
    }
  };

  const navigateTo = (nextPage: AppPage) => {
    if (nextPage === page) return;
    if (
      page === "settings" &&
      settingsEditorDirty &&
      !confirm("当前编辑内容尚未保存。放弃这些更改并离开设置页吗？")
    ) {
      return;
    }
    if (page === "settings") setSettingsEditorDirty(false);
    if (
      page === "hotwords" &&
      hotwordLibraryDirty &&
      !confirm("热词列表尚未保存。放弃这些更改并离开热词库吗？")
    ) return;
    setPage(nextPage);
  };

  const setVoiceprintProvider = async (id: string | null) => {
    const next = { ...settings, voiceprintProviderId: id };
    try {
      await api.saveSettings(next);
      setSettings(next);
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
    hotwordListId: string | null,
  ) => {
    try {
      const summary = await api.startTranscription(
        recordingId,
        settings.activeAsrProviderId,
        speakerCount,
        hotwordListId,
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
  const importActive = audioImport?.status === "running";
  const recordingDeleteTarget = recordingDeleteRequest
    ? recordings.find((item) => item.id === recordingDeleteRequest.id) ?? null
    : null;
  const settingsModel = useMemo(() => ({
    settings,
    asrProviders: providers,
    llmProviders,
    microphoneCount: micDevices.length,
    appVersion,
    recordingActive: isActive(snapshot.state),
  }), [appVersion, llmProviders, micDevices.length, providers, settings, snapshot.state]);
  const settingsActions = useMemo(() => ({
    onSettingsChange: setSettings,
    onAsrProvidersChange: setProviders,
    onLlmProvidersChange: setLlmProviders,
    onFirstRunComplete: () => {
      setSettingsRoute("recording");
      setPage("recorder");
    },
    onEditorDirtyChange: setSettingsEditorDirty,
    onToast: showToast,
    onError: showError,
  }), [showError, showToast]);

  return (
    <div className="app-shell">
      <ToastRegion toasts={toasts} onDismiss={dismissToast} />
      <aside className="app-sidebar">
        <div className="sidebar-brand" aria-label="Nota"><Radio size={20} /></div>
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
          className={`sidebar-item ${page === "hotwords" ? "active" : ""}`}
          onClick={() => navigateTo("hotwords")}
        >
          <BookMarked size={20} />
          <span>热词库</span>
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

      <div className={`app-content ${page === "settings" ? "settings-mode" : ""}`}>
        {page !== "settings" && <header className="topbar">
          <div>
            <strong>
              {page === "recorder"
                ? "录音"
                : page === "recordings"
                  ? "录音记录"
                  : page === "hotwords"
                    ? "热词库"
                  : page === "voiceprints"
                    ? "声纹管理"
                    : "设置"}
            </strong>
            <span>
              {page === "recorder"
                ? "捕捉会议声音与麦克风"
                : page === "recordings"
                  ? "播放录音并查看文字转写"
                  : page === "hotwords"
                    ? "为不同会议场景管理本地热词列表"
                  : page === "voiceprints"
                    ? "管理本地参会人姓名与声纹样本"
                    : "管理录音偏好与语音转写服务"}
            </span>
          </div>
          <span className="local-pill">本地优先</span>
        </header>}

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
                      <AppTooltip content={targetsRefreshing ? "正在刷新应用列表" : "刷新应用列表"} wrapDisabled={targetsRefreshing}>
                        <button
                          type="button"
                          className="refresh-targets"
                          aria-label="刷新应用列表"
                          disabled={targetsRefreshing}
                          onClick={() =>
                            void refreshTargets().catch(showError)
                          }
                        >
                          <RefreshCw size={16} className={targetsRefreshing ? "spinning" : ""} />
                        </button>
                      </AppTooltip>
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

          {settings.autoTranscribe && (
            <div className="auto-hotword-setting">
              <label>
                <span>自动转写热词</span>
                <select
                  value={settings.autoTranscribeHotwordListId ?? ""}
                  onChange={(event) => {
                    const next = {
                      ...settings,
                      autoTranscribeHotwordListId: event.target.value || null,
                    };
                    setSettings(next);
                    void api.saveSettings(next).catch(showError);
                  }}
                >
                  <option value="">不使用热词</option>
                  {hotwordLists.map((list) => (
                    <option key={list.id} value={list.id} disabled={list.entryCount === 0}>
                      {list.name}（{list.entryCount} 个词）{list.entryCount === 0 ? " · 空列表" : ""}
                      {list.superHotwordCount > 0 ? ` · ${list.superHotwordCount} 个超级热词` : ""}
                    </option>
                  ))}
                </select>
              </label>
              {settings.autoTranscribeHotwordListId && activeTranscriptionOptions && !activeTranscriptionOptions.hotwords.supported && (
                <span className="field-error">
                  {activeTranscriptionOptions.hotwords.mode === "serverUpgradeRequired"
                    ? "当前 Nota ASR Server 需要升级后才能使用热词；录音会保存，但自动转写不会启动。"
                    : "当前 Provider 或模型不支持热词；录音会保存，但自动转写不会启动。"}
                </span>
              )}
              {autoHotwordList && activeTranscriptionOptions?.hotwords.supported
                && !activeTranscriptionOptions.hotwords.weightsSupported
                && autoHotwordList.weightedEntryCount > 0 && (
                <span>
                  当前 Provider 不支持自定义权重；自动转写时会忽略权重，并使用全部 {autoHotwordList.entryCount} 个普通热词。
                </span>
              )}
              {autoHotwordList && activeTranscriptionOptions?.hotwords.weightsSupported
                && autoHotwordList.superHotwordCount > 0 && (
                <span>
                  自动转写将使用 {autoHotwordList.superHotwordCount} 个超级热词；权重过高可能增加相近发音的误识别。
                </span>
              )}
            </div>
          )}

          <div className="recorder-footer">
            <AppTooltip content={settings.outputDirectory || "使用默认录音目录"} side="top" align="start">
              <button className="folder-choice" onClick={chooseOutput}>
                <Folder size={17} />
                <span>{settings.outputDirectory || "默认录音目录"}</span>
              </button>
            </AppTooltip>
            {!isActive(snapshot.state) ? (
              <AppTooltip
                content={importActive ? "停止或等待音频导入完成后才能开始录音" : ""}
                wrapDisabled={importActive}
              >
                <button
                  className="record-button"
                  disabled={importActive || (captureMode === "process" && !targetId)}
                  onClick={() => void requestStart(captureMode)}
                >
                  <span className="record-dot" />开始录音
                </button>
              </AppTooltip>
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
          transcriptionVersions={transcriptionVersions}
          transcriptLoading={transcriptLoading}
          recordingActive={isActive(snapshot.state)}
          audioImport={audioImport}
          hasProvider={providers.some(
            (provider) => provider.id === settings.activeAsrProviderId,
          )}
          activeProviderKind={providers.find(
            (provider) => provider.id === settings.activeAsrProviderId,
          )?.kind ?? null}
          activeProviderName={providers.find(
            (provider) => provider.id === settings.activeAsrProviderId,
          )?.name ?? null}
          activeProviderId={settings.activeAsrProviderId}
          hotwordLists={hotwordLists}
          hasVoiceprintProvider={providers.some(
            (provider) => provider.id === settings.voiceprintProviderId
              && provider.kind === "funAsr",
          )}
          llmProviders={llmProviders}
          activeLlmProviderId={settings.activeLlmProviderId}
          participants={participants}
          onSelect={setSelectedRecordingId}
          onReturnToRecorder={() => navigateTo("recorder")}
          onImportAudio={() => void chooseAudioFiles()}
          onCancelAudioImport={() => void cancelAudioImport()}
          onDismissAudioImport={() => setAudioImport(null)}
          onPreparePlayback={(id) => api.prepareRecordingPlayback(id)}
          onPlaybackError={(message) => showToast("error", message)}
          onStartTranscription={(id, speakerCount, hotwordListId) =>
            void startTranscription(id, speakerCount, hotwordListId)
          }
          onGetTranscriptionOptions={(providerId) => api.getTranscriptionOptions(providerId)}
          onOpenHotwordLibrary={() => navigateTo("hotwords")}
          onResumeTranscription={(id) => void resumeTranscription(id)}
          onCancelTranscription={(id) => void cancelTranscription(id)}
          onSelectTranscriptionVersion={async (id, generation) => {
            setTranscriptLoading(true);
            try {
              const document = await api.selectTranscriptionVersion(id, generation);
              setTranscript(document);
              setTranscriptionVersions(await api.listTranscriptionVersions(id));
            } catch (error) {
              showError(error);
            } finally {
              setTranscriptLoading(false);
            }
          }}
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

        {page === "hotwords" && (
          <HotwordLibraryWorkspace
            lists={hotwordLists}
            onRefresh={refreshHotwordLists}
            onDirtyChange={setHotwordLibraryDirty}
            onMessage={handleAiMessage}
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
            route={settingsRoute}
            onRouteChange={setSettingsRoute}
            model={settingsModel}
            actions={settingsActions}
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
              {recordingDeleteTarget?.origin === "imported"
                ? recordingDeleteRequest.permanent
                  ? "Nota 管理的音频副本将被永久删除，最初选择的文件不受影响。"
                  : "Nota 管理的音频副本将移入 Windows 回收站，最初选择的文件不受影响。"
                : recordingDeleteRequest.permanent
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

      {page !== "settings" && <footer className="app-footer">
        <span><span className="privacy-dot" />本地录音；仅在转写或手动生成 AI 文档时连接所选服务</span>
        <span>Ctrl + Alt + F9 开始/暂停 · F10 停止</span>
      </footer>}
      </div>

    </div>
  );
}
