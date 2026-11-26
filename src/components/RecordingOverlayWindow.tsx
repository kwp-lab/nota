import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { ExternalLink, GripVertical, Pause, Play, Square } from "lucide-react";
import { api, type UnlistenFn } from "../api";
import type { LevelEvent, RecordingSnapshot } from "../types";
import { AppTooltip } from "./AppTooltip";
import { AudioActivityWaveform } from "./AudioActivityWaveform";

const formatDuration = (durationMs: number) => {
  const seconds = Math.floor(Math.max(0, durationMs) / 1000);
  return [Math.floor(seconds / 3600), Math.floor(seconds / 60) % 60, seconds % 60]
    .map((value) => String(value).padStart(2, "0")).join(":");
};

export function RecordingOverlayWindow() {
  const [snapshot, setSnapshot] = useState<RecordingSnapshot | null>(null);
  const [levels, setLevels] = useState<LevelEvent>({ system: 0, microphone: 0 });
  const [updatedAt, setUpdatedAt] = useState(0);
  const [expanded, setExpanded] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [retry, setRetry] = useState(0);
  const [connected, setConnected] = useState(false);
  const readySessionRef = useRef<string | null>(null);
  const snapshotRevisionRef = useRef(0);
  const snapshotRef = useRef(snapshot);
  snapshotRef.current = snapshot;
  const signalRef = useRef<HTMLButtonElement>(null);
  const mountedRef = useRef(false);
  const operationRef = useRef(false);

  useEffect(() => {
    let disposed = false;
    const listeners: UnlistenFn[] = [];
    mountedRef.current = true;
    document.body.classList.add("recording-overlay-body");
    const subscribe = async () => {
      try {
        const stopSnapshots = await api.onSnapshot((next) => {
          if (disposed) return;
          snapshotRevisionRef.current++;
          setSnapshot(next);
          if (next.state !== "recording") {
            setLevels({ system: 0, microphone: 0 });
          }
        });
        if (disposed) {
          stopSnapshots();
          return;
        }
        listeners.push(stopSnapshots);
        const stopLevels = await api.onLevels((next) => {
          if (disposed) return;
          setLevels(next);
          setUpdatedAt(Date.now());
        });
        if (disposed) {
          stopLevels();
          return;
        }
        listeners.push(stopLevels);
        const revision = snapshotRevisionRef.current;
        const current = await api.getSnapshot();
        if (disposed) return;
        if (revision === snapshotRevisionRef.current) setSnapshot(current);
        setConnected(true);
      } catch (failure) {
        if (!disposed) {
          setError(String(failure));
          setExpanded(true);
        }
      }
    };
    void subscribe();
    return () => {
      disposed = true;
      mountedRef.current = false;
      listeners.forEach((unlisten) => unlisten());
      document.body.classList.remove("recording-overlay-body");
    };
  }, [retry]);

  useEffect(() => {
    if (!connected || !snapshot?.sessionId || readySessionRef.current === snapshot.sessionId) return;
    let disposed = false;
    const sessionId = snapshot.sessionId;
    if (readySessionRef.current !== null) setExpanded(false);
    setError("");
    setLevels({ system: 0, microphone: 0 });
    void api.recordingOverlayReady(sessionId).then(() => {
      if (!disposed) readySessionRef.current = sessionId;
    }).catch((failure) => {
      if (!disposed) {
        setError(String(failure));
        setExpanded(true);
      }
    });
    return () => { disposed = true; };
  }, [connected, snapshot?.sessionId]);

  const state = snapshot?.state;
  const paused = state === "paused";
  const finalizing = state === "finalizing";
  const canToggle = state === "recording" || paused || state === "interrupted";
  const canStop = canToggle;
  const failed = state === "error" || Boolean(snapshot?.fault);
  const hasError = Boolean(error) || failed;
  const stateLabel = state === "recording"
    ? "正在录音"
    : paused ? "已暂停"
      : finalizing ? "正在保存"
        : state === "interrupted" ? "音源中断"
          : state === "error" ? "保存异常"
            : state === "completed" ? "已保存"
              : "连接中";
  const compactLabel = failed ? "需查看" : stateLabel;
  const systemLevel = snapshot?.system.healthy ? levels.system : 0;
  const microphoneLevel = snapshot?.microphone.healthy && snapshot.microphoneSelection
    ? levels.microphone : 0;
  const effectiveLevel = Math.max(systemLevel, microphoneLevel);

  useLayoutEffect(() => {
    void api.resizeRecordingOverlay(expanded || hasError, hasError).catch((failure) => {
      setError(String(failure));
    });
  }, [expanded, hasError]);

  const runAction = async (action: () => Promise<RecordingSnapshot>) => {
    if (operationRef.current) return;
    operationRef.current = true;
    setBusy(true);
    setError("");
    const sessionId = snapshotRef.current?.sessionId;
    const revision = snapshotRevisionRef.current;
    try {
      const next = await action();
      if (
        mountedRef.current &&
        sessionId === snapshotRef.current?.sessionId &&
        revision === snapshotRevisionRef.current
      ) setSnapshot(next);
    } catch (failure) {
      if (mountedRef.current) {
        setError(String(failure));
        setExpanded(true);
      }
    } finally {
      operationRef.current = false;
      if (mountedRef.current) setBusy(false);
    }
  };

  const openMain = () => {
    void api.showRecordingMainWindow().catch((failure) => setError(String(failure)));
  };

  return (
    <section
      className={`recording-overlay${hasError ? " has-error" : ""}`}
      aria-label="Nota 录音状态"
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          setExpanded(false);
          signalRef.current?.focus();
        }
      }}
    >
      <div className="recording-overlay-header">
        <AppTooltip content="拖动状态卡片" side="right">
          <button
            type="button"
            className="icon-button compact recording-overlay-drag"
            aria-label="拖动状态卡片"
            aria-description="鼠标拖动，或使用方向键移动"
            onPointerDown={(event) => {
              if (event.button === 0) {
                event.preventDefault();
                void api.dragRecordingOverlay().catch((failure) => setError(String(failure)));
              }
            }}
            onKeyDown={(event) => {
              const directions = {
                ArrowLeft: "left", ArrowRight: "right", ArrowUp: "up", ArrowDown: "down",
              } as const;
              const direction = directions[event.key as keyof typeof directions];
              if (direction) {
                event.preventDefault();
                void api.nudgeRecordingOverlay(direction).catch((failure) => setError(String(failure)));
              }
            }}
          ><GripVertical /></button>
        </AppTooltip>
        <AppTooltip content={`${stateLabel}，点击${expanded ? "收起" : "展开"}操作`} side="bottom">
          <button
            ref={signalRef}
            type="button"
            className="recording-overlay-signal"
            aria-label={`${stateLabel}，${expanded ? "收起" : "展开"}录音操作`}
            aria-expanded={expanded || hasError}
            aria-controls="recording-overlay-actions"
            onClick={() => setExpanded((current) => !current)}
          >
            <AudioActivityWaveform
              level={effectiveLevel}
              updatedAt={updatedAt}
              active={state === "recording"}
              paused={paused}
            />
            <span className="recording-overlay-value">
              {state !== "recording" || failed
                ? compactLabel : formatDuration(snapshot?.activeDurationMs ?? 0)}
            </span>
          </button>
        </AppTooltip>
      </div>
      {(expanded || hasError) && (
        <div className="recording-overlay-expanded" id="recording-overlay-actions">
          <div className="recording-overlay-status" role="status">
            <span>{stateLabel}</span>
            <time>{formatDuration(snapshot?.activeDurationMs ?? 0)}</time>
          </div>
          {hasError && (
            <p className="recording-overlay-error" role="alert">
              {error || "录音状态需要确认，请打开主窗口查看详情。"}
              {!connected && <button type="button" className="button secondary compact" onClick={() => {
                setError("");
                setConnected(false);
                setRetry((value) => value + 1);
              }}>重试连接</button>}
            </p>
          )}
          <div className="recording-overlay-actions">
            <AppTooltip content={paused ? "继续录音" : "暂停录音"} side="top" wrapDisabled>
              <button
                type="button"
                className="icon-button"
                aria-label={paused ? "继续录音" : "暂停录音"}
                disabled={busy || !canToggle}
                onClick={() => void runAction(paused ? api.resumeRecording : api.pauseRecording)}
              >{paused ? <Play /> : <Pause />}</button>
            </AppTooltip>
            <AppTooltip content={finalizing ? "正在保存" : "停止并保存"} side="top" wrapDisabled>
              <button
                type="button"
                className="icon-button"
                aria-label="停止并保存"
                disabled={busy || !canStop}
                onClick={() => void runAction(api.stopRecording)}
              ><Square /></button>
            </AppTooltip>
            <AppTooltip content="打开主窗口" side="top">
              <button type="button" className="icon-button" aria-label="打开主窗口" onClick={openMain}>
                <ExternalLink />
              </button>
            </AppTooltip>
          </div>
        </div>
      )}
    </section>
  );
}
