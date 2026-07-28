import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  ChevronDown,
  Folder,
  Headphones,
  Mic,
  Pause,
  Radio,
  RefreshCw,
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
import { ConsentDialog } from "./components/ConsentDialog";
import { LevelMeter } from "./components/LevelMeter";
import { RecordingList } from "./components/RecordingList";
import { SettingsDialog } from "./components/SettingsDialog";
import type {
  AecMode,
  AppSettings,
  AudioDevice,
  CaptureSelection,
  CaptureTarget,
  LevelEvent,
  RecordingItem,
  RecordingSnapshot,
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
  aecStatus: "disabled",
  fault: null,
};

const defaultSettings: AppSettings = {
  outputDirectory: "",
  aecMode: "auto",
  microphoneEnabled: true,
  consentTemplate:
    "提示：为了整理本次会议内容，我将在本地录音。录音仅保存在我的电脑中，如有异议请随时告知。",
  firstRunComplete: false,
  recordingNoticeAcknowledged: false,
  shortcutsEnabled: true,
  toggleShortcut: "Ctrl+Alt+F9",
  stopShortcut: "Ctrl+Alt+F10",
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

type CaptureMode = "process" | "system";
type StartRequestMode = CaptureMode | "current";

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
  const [levels, setLevels] = useState<LevelEvent>({ system: 0, microphone: 0 });
  const [recordings, setRecordings] = useState<RecordingItem[]>([]);
  const [recoverable, setRecoverable] = useState<RecordingItem[]>([]);
  const [consentOpen, setConsentOpen] = useState(false);
  const [consentConfirmed, setConsentConfirmed] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [appVersion, setAppVersion] = useState("…");
  const [draftSettings, setDraftSettings] = useState(defaultSettings);
  const [pendingCapture, setPendingCapture] = useState<CaptureSelection | null>(null);
  const [pendingSourceDescription, setPendingSourceDescription] = useState("");
  const targetIdRef = useRef("");
  const targetPreferenceRef = useRef<CaptureTargetPreference | null>(null);
  const refreshTargetsPromiseRef = useRef<Promise<CaptureTarget[]> | null>(null);
  const requestStartRef = useRef<(mode?: StartRequestMode) => void>(() => undefined);

  const refreshLibrary = useCallback(async () => {
    const [items, recoverableItems] = await Promise.all([
      api.listRecordings(),
      api.listRecoverable(),
    ]);
    setRecordings(items);
    setRecoverable(recoverableItems);
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
    void Promise.all([
      refreshTargets(),
      refreshDevices(),
      api.getSettings(),
      api.getSnapshot(),
      api.getAppVersion().catch(() => "未知"),
    ])
      .then(async ([targetList, deviceList, savedSettings, current, version]) => {
        if (!mounted) return;
        applyCaptureTargets(targetList);
        setDevices(deviceList);
        setSettings(savedSettings);
        setDraftSettings(savedSettings);
        setSettingsOpen(!savedSettings.firstRunComplete);
        setSnapshot(current);
        setAppVersion(version);
        if (current.fault) setNotice(current.fault.userMessage);
        await refreshLibrary();
        unlistenSnapshot = await api.onSnapshot(setSnapshot);
        unlistenLevels = await api.onLevels(setLevels);
        unlistenStart = await api.onRequestStart((mode) => requestStartRef.current(mode));
        unlistenExit = await api.onRequestExit(() => {
          if (confirm("录音仍在进行。停止并保存后退出应用？")) {
            void api.quitApplication(true);
          }
        });
      })
      .catch((error) => setNotice(String(error)));
    return () => {
      mounted = false;
      unlistenSnapshot?.();
      unlistenLevels?.();
      unlistenStart?.();
      unlistenExit?.();
    };
  }, [applyCaptureTargets, refreshDevices, refreshLibrary, refreshTargets]);

  useEffect(() => {
    if (snapshot.state === "completed") void refreshLibrary();
  }, [snapshot.state, refreshLibrary]);

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

  const capture = useMemo<CaptureSelection>(() => {
    if (captureMode === "process") return { kind: "process", targetId };
    return systemCapture;
  }, [captureMode, systemCapture, targetId]);

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

  const start = async (
    requestedCapture: CaptureSelection,
    noticeAcknowledged: boolean,
  ) => {
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
        consentConfirmed: noticeAcknowledged,
      });
      setSnapshot(next);
      setConsentOpen(false);
      setConsentConfirmed(false);
      setPendingCapture(null);
      setPendingSourceDescription("");
    } catch (error) {
      setNotice(String(error));
    }
  };

  const requestStart = async (requestedMode: CaptureMode = captureMode) => {
    try {
      let requestedCapture: CaptureSelection;
      let requestedDescription: string;
      setCaptureMode(requestedMode);

      if (requestedMode === "process") {
        const refreshedTargets = await refreshTargets();
        const target = resolveCaptureTarget(
          refreshedTargets,
          targetIdRef.current,
          targetPreferenceRef.current,
        );
        if (!target) {
          setNotice("没有找到可录制的应用。请启动会议应用后刷新并选择录音来源。");
          return;
        }
        selectTarget(target.id, target);
        requestedCapture = { kind: "process", targetId: target.id };
        requestedDescription = target.displayName;
      } else {
        await refreshDevices();
        requestedCapture = systemCapture;
        requestedDescription =
          renderDeviceId === "default"
            ? `全部系统声音 · ${defaultRenderLabel}`
            : `全部系统声音 · ${renderDevices.find((device) => device.id === renderDeviceId)?.name ?? ""}`;
      }

      setPendingCapture(requestedCapture);
      setPendingSourceDescription(requestedDescription);
      if (settings.recordingNoticeAcknowledged) {
        await start(requestedCapture, true);
        return;
      }
      setConsentConfirmed(false);
      setConsentOpen(true);
    } catch (error) {
      setNotice(String(error));
    }
  };
  requestStartRef.current = (mode = "current") => {
    void requestStart(mode === "current" ? captureMode : mode);
  };

  const acknowledgeNoticeAndStart = async () => {
    if (!consentConfirmed) return;
    const nextSettings = {
      ...settings,
      recordingNoticeAcknowledged: true,
    };
    try {
      await api.saveSettings(nextSettings);
      setSettings(nextSettings);
      setDraftSettings(nextSettings);
      await start(pendingCapture ?? capture, true);
    } catch (error) {
      setNotice(String(error));
    }
  };

  const chooseOutput = async () => {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected !== "string") return;
    const next = { ...settings, outputDirectory: selected };
    setSettings(next);
    await api.saveSettings(next);
  };

  const chooseDraftOutput = async () => {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected === "string") {
      setDraftSettings((current) => ({ ...current, outputDirectory: selected }));
    }
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
      setSettingsOpen(false);
    } catch (error) {
      setNotice(String(error));
    }
  };

  const toggleLiveMicrophone = async () => {
    try {
      const enabled = !settings.microphoneEnabled;
      const nextSnapshot = await api.setMicrophoneEnabled(
        enabled
          ? micDeviceId === "default"
            ? { kind: "followDefaultCommunications" }
            : { kind: "fixed", endpointId: micDeviceId }
          : null,
      );
      setSnapshot(nextSnapshot);
      setSettings((current) => ({ ...current, microphoneEnabled: enabled }));
    } catch (error) {
      setNotice(String(error));
    }
  };

  const aecLabels: Record<AecMode, string> = {
    auto: "AEC 自动",
    on: "AEC 开启",
    off: "AEC 关闭",
  };

  return (
    <div className="app-shell">
      <header className="topbar">
        <div className="brand">
          <span className="brand-mark"><Radio size={18} /></span>
          <span>Nota</span>
          <span className="local-pill">仅本地</span>
        </div>
        <button
          className="icon-button"
          title="设置"
          onClick={() => {
            setDraftSettings(settings);
            setSettingsOpen(true);
          }}
        >
          <Settings size={19} />
        </button>
      </header>

      <main>
        {notice && (
          <div className="toast" role="alert">
            <AlertTriangle size={17} />
            <span>{notice}</span>
            <button onClick={() => setNotice(null)}>关闭</button>
          </div>
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
                    void refreshTargets().catch((error) => setNotice(String(error)));
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
                          void refreshTargets().catch((error) => setNotice(String(error)))
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
              <div className="live-status">
                <span><Headphones size={16} />{aecLabels[settings.aecMode]}</span>
                <button className="text-button" onClick={() => void toggleLiveMicrophone()}>
                  <Mic size={14} />
                  {settings.microphoneEnabled ? "关闭麦克风" : "开启麦克风"}
                </button>
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
                  onClick={async () =>
                    setSnapshot(
                      snapshot.state === "paused"
                        ? await api.resumeRecording()
                        : await api.pauseRecording(),
                    )
                  }
                >
                  {snapshot.state === "paused" ? <Radio size={16} /> : <Pause size={16} />}
                  {snapshot.state === "paused" ? "继续" : "暂停"}
                </button>
                <button
                  className="button stop"
                  disabled={snapshot.state === "finalizing"}
                  onClick={async () => setSnapshot(await api.stopRecording())}
                >
                  <Square size={14} fill="currentColor" />停止并保存
                </button>
              </div>
            )}
          </div>
        </section>

        <RecordingList
          items={recordings}
          recoverable={recoverable}
          onPreparePlayback={(id) => api.prepareRecordingPlayback(id)}
          onPlaybackError={setNotice}
          onReveal={(id) => void api.revealRecording(id)}
          onDelete={(id) => {
            if (!confirm("将此录音移入回收站？")) return;
            void api.deleteRecording(id, false).then(refreshLibrary);
          }}
          onRecover={(id) => void api.recoverRecording(id).then(refreshLibrary)}
          onDiscardRecovery={(id) => {
            if (!confirm("永久删除这个未完成的恢复文件？此操作无法撤销。")) return;
            void api.deleteRecoverable(id).then(refreshLibrary);
          }}
          onRename={(id, currentTitle) => {
            const title = prompt("输入新的录音名称", currentTitle);
            if (!title || title === currentTitle) return;
            void api.renameRecording(id, title).then(refreshLibrary).catch((error) => setNotice(String(error)));
          }}
          onPermanentDelete={(id) => {
            if (!confirm("永久删除此录音？此操作无法撤销。")) return;
            void api.deleteRecording(id, true).then(refreshLibrary);
          }}
        />
      </main>

      <footer className="app-footer">
        <span><span className="privacy-dot" />离线工作 · 无上传</span>
        <span>Ctrl + Alt + F9 开始/暂停 · F10 停止</span>
      </footer>

      <ConsentDialog
        open={consentOpen}
        description={pendingSourceDescription || sourceDescription}
        outputDirectory={settings.outputDirectory}
        template={settings.consentTemplate}
        confirmed={consentConfirmed}
        onConfirmedChange={setConsentConfirmed}
        onCopy={() =>
          void api.copyConsentTemplate(settings.consentTemplate).then(() => setNotice("告知话术已复制"))
        }
        onCancel={() => {
          setConsentOpen(false);
          setConsentConfirmed(false);
          setPendingCapture(null);
          setPendingSourceDescription("");
        }}
        onStart={() => void acknowledgeNoticeAndStart()}
      />
      <SettingsDialog
        open={settingsOpen}
        firstRun={!settings.firstRunComplete}
        settings={draftSettings}
        microphoneCount={micDevices.length}
        appVersion={appVersion}
        onChange={setDraftSettings}
        onChooseOutput={() => void chooseDraftOutput()}
        onOpenMicrophoneSettings={() => void api.openMicrophoneSettings()}
        onCancel={() => setSettingsOpen(false)}
        onSave={() => void saveDraftSettings()}
      />
    </div>
  );
}
