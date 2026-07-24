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
  Settings,
  Square,
  Volume2,
} from "lucide-react";
import { api, type UnlistenFn } from "./api";
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

export default function App() {
  const [targets, setTargets] = useState<CaptureTarget[]>([]);
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  const [settings, setSettings] = useState(defaultSettings);
  const [captureMode, setCaptureMode] = useState<"process" | "system">("process");
  const [targetId, setTargetId] = useState("");
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
  const [draftSettings, setDraftSettings] = useState(defaultSettings);
  const requestStartRef = useRef<() => void>(() => undefined);

  const refreshLibrary = useCallback(async () => {
    const [items, recoverableItems] = await Promise.all([
      api.listRecordings(),
      api.listRecoverable(),
    ]);
    setRecordings(items);
    setRecoverable(recoverableItems);
  }, []);

  useEffect(() => {
    let mounted = true;
    let unlistenSnapshot: UnlistenFn | undefined;
    let unlistenLevels: UnlistenFn | undefined;
    let unlistenStart: UnlistenFn | undefined;
    let unlistenExit: UnlistenFn | undefined;
    void Promise.all([
      api.listCaptureTargets(),
      api.listAudioDevices(),
      api.getSettings(),
      api.getSnapshot(),
    ])
      .then(async ([targetList, deviceList, savedSettings, current]) => {
        if (!mounted) return;
        setTargets(targetList);
        setDevices(deviceList);
        setSettings(savedSettings);
        setDraftSettings(savedSettings);
        setSettingsOpen(!savedSettings.firstRunComplete);
        setSnapshot(current);
        if (current.fault) setNotice(current.fault.userMessage);
        if (targetList[0]) setTargetId(targetList[0].id);
        await refreshLibrary();
        unlistenSnapshot = await api.onSnapshot(setSnapshot);
        unlistenLevels = await api.onLevels(setLevels);
        unlistenStart = await api.onRequestStart(() => requestStartRef.current());
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
  }, [refreshLibrary]);

  useEffect(() => {
    if (snapshot.state === "completed") void refreshLibrary();
  }, [snapshot.state, refreshLibrary]);

  const selectedTarget = targets.find((target) => target.id === targetId);
  const renderDevices = devices.filter((device) => device.direction === "render");
  const micDevices = devices.filter((device) => device.direction === "capture");

  const capture = useMemo<CaptureSelection>(() => {
    if (captureMode === "process") return { kind: "process", targetId };
    return {
      kind: "system",
      device:
        renderDeviceId === "default"
          ? { kind: "followDefaultCommunications" }
          : { kind: "fixed", endpointId: renderDeviceId },
    };
  }, [captureMode, targetId, renderDeviceId]);

  const sourceDescription =
    captureMode === "process"
      ? selectedTarget?.displayName ?? "未选择应用"
      : renderDeviceId === "default"
        ? "全部系统声音 · 跟随默认通信设备"
        : `全部系统声音 · ${renderDevices.find((device) => device.id === renderDeviceId)?.name ?? ""}`;

  const start = async (noticeAcknowledged: boolean) => {
    try {
      const next = await api.startRecording({
        capture,
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
    } catch (error) {
      setNotice(String(error));
    }
  };

  const requestStart = () => {
    if (settings.recordingNoticeAcknowledged) {
      void start(true);
      return;
    }
    setConsentConfirmed(false);
    setConsentOpen(true);
  };
  requestStartRef.current = requestStart;

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
      await start(true);
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
                  onClick={() => setCaptureMode("process")}
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
                <label className="field">
                  <span><Volume2 size={16} />录音来源</span>
                  <div className="select-wrap">
                    {captureMode === "process" ? (
                      <select value={targetId} onChange={(e) => setTargetId(e.target.value)}>
                        {targets.map((target) => (
                          <option key={target.id} value={target.id}>{target.displayName}</option>
                        ))}
                      </select>
                    ) : (
                      <select value={renderDeviceId} onChange={(e) => setRenderDeviceId(e.target.value)}>
                        <option value="default">跟随默认通信设备</option>
                        {renderDevices.map((device) => (
                          <option key={device.id} value={device.id}>{device.name}</option>
                        ))}
                      </select>
                    )}
                    <ChevronDown size={16} />
                  </div>
                </label>
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
                      <option value="default">跟随默认通信设备</option>
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
                onClick={requestStart}
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
        description={sourceDescription}
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
        }}
        onStart={() => void acknowledgeNoticeAndStart()}
      />
      <SettingsDialog
        open={settingsOpen}
        firstRun={!settings.firstRunComplete}
        settings={draftSettings}
        microphoneCount={micDevices.length}
        onChange={setDraftSettings}
        onChooseOutput={() => void chooseDraftOutput()}
        onOpenMicrophoneSettings={() => void api.openMicrophoneSettings()}
        onCancel={() => setSettingsOpen(false)}
        onSave={() => void saveDraftSettings()}
      />
    </div>
  );
}
