import { convertFileSrc } from "@tauri-apps/api/core";
import {
  AlertCircle,
  Check,
  Clipboard,
  Download,
  FileAudio,
  FolderOpen,
  LoaderCircle,
  MoreHorizontal,
  Pause,
  Play,
  RotateCcw,
  Search,
  Sparkles,
  Square,
  Trash2,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type {
  RecordingItem,
  TranscriptDocument,
  TranscriptionStatus,
  TranscriptionSummary,
} from "../types";

interface RecordingsWorkspaceProps {
  items: RecordingItem[];
  recoverable: RecordingItem[];
  selectedId: string | null;
  transcript: TranscriptDocument | null;
  transcriptLoading: boolean;
  recordingActive: boolean;
  hasProvider: boolean;
  onSelect: (id: string) => void;
  onReturnToRecorder: () => void;
  onPreparePlayback: (id: string) => Promise<string>;
  onPlaybackError: (message: string) => void;
  onStartTranscription: (id: string) => void;
  onResumeTranscription: (id: string) => void;
  onCancelTranscription: (id: string) => void;
  onCopyTranscript: (id: string) => void;
  onExportTranscript: (id: string, title: string) => void;
  onReveal: (id: string) => void;
  onDelete: (id: string) => void;
  onRecover: (id: string) => void;
  onDiscardRecovery: (id: string) => void;
  onRename: (id: string, currentTitle: string) => void;
  onPermanentDelete: (id: string) => void;
}

interface RecordingActionMenu {
  recordingId: string;
  left: number;
  top: number;
  source: "context" | "detail";
}

const actionMenuWidth = 148;
const actionMenuHeight = 126;
const actionMenuMargin = 8;

const constrainActionMenuPosition = (left: number, top: number) => ({
  left: Math.max(
    actionMenuMargin,
    Math.min(left, window.innerWidth - actionMenuWidth - actionMenuMargin),
  ),
  top: Math.max(
    actionMenuMargin,
    Math.min(top, window.innerHeight - actionMenuHeight - actionMenuMargin),
  ),
});

const formatDuration = (milliseconds: number) => {
  const seconds = Math.round(milliseconds / 1000);
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const rest = seconds % 60;
  return hours > 0
    ? `${hours}:${minutes.toString().padStart(2, "0")}:${rest.toString().padStart(2, "0")}`
    : `${minutes}:${rest.toString().padStart(2, "0")}`;
};

const formatSize = (bytes: number) =>
  bytes < 1024 * 1024
    ? `${Math.max(1, Math.round(bytes / 1024))} KB`
    : `${(bytes / 1024 / 1024).toFixed(1)} MB`;

const statusLabels: Record<TranscriptionStatus, string> = {
  queued: "排队中",
  preparing: "准备音频",
  transcribing: "转写中",
  completed: "已转写",
  failed: "转写失败",
  interrupted: "已中断",
  cancelled: "已取消",
};

const processingStatuses: TranscriptionStatus[] = ["queued", "preparing", "transcribing"];
const resumableStatuses: TranscriptionStatus[] = ["failed", "interrupted", "cancelled"];

const batchPhaseLabels = {
  preparing: "准备录音",
  uploading: "上传录音",
  queued: "服务器排队",
  transcribing: "处理音频窗口",
  diarizing: "统一说话人",
  finalizing: "整理结果",
} as const;

const transcriptionLabel = (transcription: TranscriptionSummary) =>
  transcription.protocol === "nota_batch_v1"
  && processingStatuses.includes(transcription.status)
  && transcription.progressPhase
    ? batchPhaseLabels[transcription.progressPhase]
    : statusLabels[transcription.status];

const transcriptionProgress = (transcription: TranscriptionSummary) => {
  if (transcription.protocol !== "nota_batch_v1") {
    return transcription.totalChunks
      ? `已完成 ${transcription.completedChunks} / ${transcription.totalChunks} 个分块`
      : "正在准备 16 kHz 音频分块";
  }
  if (transcription.progressPhase === "queued") return "录音已上传，等待服务器处理";
  if (transcription.progressUnit === "bytes" && transcription.progressTotal > 0) {
    return `已上传 ${formatSize(transcription.progressCurrent)} / ${formatSize(transcription.progressTotal)}`;
  }
  if (transcription.progressUnit === "windows" && transcription.progressTotal > 0) {
    return `已处理 ${transcription.progressCurrent} / ${transcription.progressTotal} 个音频窗口`;
  }
  if (transcription.progressPhase === "diarizing") return "正在为整场会议统一说话人";
  if (transcription.progressPhase === "finalizing") return "正在生成最终转写结果";
  return "正在准备整场会议转写";
};

const transcriptionProgressSuffix = (transcription: TranscriptionSummary) => {
  if (!processingStatuses.includes(transcription.status)) return "";
  if (transcription.protocol === "nota_batch_v1") {
    if (transcription.progressTotal <= 0) return "";
    if (transcription.progressUnit === "bytes") {
      return ` ${Math.min(100, Math.round(
        (transcription.progressCurrent / transcription.progressTotal) * 100,
      ))}%`;
    }
    if (transcription.progressUnit === "windows") {
      return ` ${transcription.progressCurrent}/${transcription.progressTotal}`;
    }
    return "";
  }
  return transcription.totalChunks > 0
    ? ` ${transcription.completedChunks}/${transcription.totalChunks}`
    : "";
};

export function RecordingsWorkspace(props: RecordingsWorkspaceProps) {
  const [query, setQuery] = useState("");
  const [audioSource, setAudioSource] = useState("");
  const [audioLoading, setAudioLoading] = useState(false);
  const [playing, setPlaying] = useState(false);
  const [pendingPlayId, setPendingPlayId] = useState<string | null>(null);
  const [pendingSeekMs, setPendingSeekMs] = useState<number | null>(null);
  const [actionMenu, setActionMenu] = useState<RecordingActionMenu | null>(null);
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const actionMenuRef = useRef<HTMLDivElement | null>(null);
  const detailMenuButtonRef = useRef<HTMLButtonElement | null>(null);
  const selected = props.items.find((item) => item.id === props.selectedId) ?? null;
  const actionMenuItem = props.items.find((item) => item.id === actionMenu?.recordingId) ?? null;
  const filtered = useMemo(() => {
    const normalized = query.trim().toLocaleLowerCase();
    return normalized
      ? props.items.filter((item) => item.title.toLocaleLowerCase().includes(normalized))
      : props.items;
  }, [props.items, query]);

  useEffect(() => {
    if (!actionMenu) return;
    if (!actionMenuItem) {
      setActionMenu(null);
      return;
    }

    const closeMenu = () => setActionMenu(null);
    const handlePointerDown = (event: PointerEvent) => {
      const target = event.target as Node;
      if (
        !actionMenuRef.current?.contains(target)
        && !detailMenuButtonRef.current?.contains(target)
      ) {
        closeMenu();
      }
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      closeMenu();
      if (actionMenu.source === "detail") detailMenuButtonRef.current?.focus();
    };

    document.addEventListener("pointerdown", handlePointerDown);
    document.addEventListener("keydown", handleKeyDown);
    window.addEventListener("resize", closeMenu);
    window.addEventListener("scroll", closeMenu, true);
    actionMenuRef.current?.querySelector<HTMLButtonElement>("button")?.focus();
    return () => {
      document.removeEventListener("pointerdown", handlePointerDown);
      document.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("resize", closeMenu);
      window.removeEventListener("scroll", closeMenu, true);
    };
  }, [actionMenu, actionMenuItem]);

  useEffect(() => {
    let cancelled = false;
    setAudioSource("");
    setPlaying(false);
    if (audioRef.current?.getAttribute("src")) {
      audioRef.current.pause();
      audioRef.current.removeAttribute("src");
      audioRef.current.load();
    }
    if (!selected) return () => { cancelled = true; };
    setAudioLoading(true);
    void props
      .onPreparePlayback(selected.id)
      .then((path) => {
        if (!cancelled) setAudioSource(convertFileSrc(path));
      })
      .catch((error) => {
        if (!cancelled) props.onPlaybackError(`无法打开“${selected.title}”：${String(error)}`);
      })
      .finally(() => {
        if (!cancelled) setAudioLoading(false);
      });
    return () => { cancelled = true; };
    // The selected id is the identity of the player source.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.selectedId]);

  useEffect(() => {
    const audio = audioRef.current;
    if (!audio || !audioSource || !selected) return;
    if (pendingSeekMs !== null) {
      audio.currentTime = pendingSeekMs / 1_000;
      setPendingSeekMs(null);
      void audio.play().catch((error) => props.onPlaybackError(String(error)));
      return;
    }
    if (pendingPlayId === selected.id) {
      setPendingPlayId(null);
      void audio.play().catch((error) => props.onPlaybackError(String(error)));
    }
  }, [audioSource, pendingPlayId, pendingSeekMs, props, selected]);

  const toggleQuickPlayback = (item: RecordingItem) => {
    const audio = audioRef.current;
    if (selected?.id === item.id && audioSource && audio) {
      if (audio.paused) {
        void audio.play().catch((error) => props.onPlaybackError(String(error)));
      } else {
        audio.pause();
      }
      return;
    }
    setPendingPlayId(item.id);
    props.onSelect(item.id);
  };

  const seekTo = (milliseconds: number) => {
    const audio = audioRef.current;
    if (!audio || !audioSource) {
      setPendingSeekMs(milliseconds);
      return;
    }
    audio.currentTime = milliseconds / 1_000;
    void audio.play().catch((error) => props.onPlaybackError(String(error)));
  };

  const openContextMenu = (
    item: RecordingItem,
    clientX: number,
    clientY: number,
  ) => {
    setActionMenu({
      recordingId: item.id,
      ...constrainActionMenuPosition(clientX, clientY),
      source: "context",
    });
  };

  const toggleDetailMenu = () => {
    if (!selected) return;
    if (actionMenu?.source === "detail" && actionMenu.recordingId === selected.id) {
      setActionMenu(null);
      return;
    }
    const bounds = detailMenuButtonRef.current?.getBoundingClientRect();
    if (!bounds) return;
    setActionMenu({
      recordingId: selected.id,
      ...constrainActionMenuPosition(bounds.right - actionMenuWidth, bounds.bottom + 6),
      source: "detail",
    });
  };

  const runMenuAction = (action: (item: RecordingItem) => void) => {
    if (!actionMenuItem) return;
    const item = actionMenuItem;
    setActionMenu(null);
    action(item);
  };

  const transcription = selected?.transcription;
  const isProcessing = !!transcription && processingStatuses.includes(transcription.status);

  return (
    <section className="library-workspace">
      <aside className="history-pane">
        <div className="history-header">
          <div>
            <p className="eyebrow">RECORDINGS</p>
            <h1>录音记录</h1>
          </div>
          <span className="count-pill">{props.items.length}</span>
        </div>
        <label className="history-search">
          <Search size={15} />
          <input
            aria-label="搜索录音"
            value={query}
            placeholder="搜索标题"
            onChange={(event) => setQuery(event.target.value)}
          />
        </label>
        {props.recoverable.length > 0 && (
          <div className="compact-recovery">
            <RotateCcw size={16} />
            <span>{props.recoverable.length} 个录音可恢复</span>
            <button onClick={() => props.onRecover(props.recoverable[0].id)}>恢复</button>
            <button
              aria-label="删除恢复文件"
              onClick={() => props.onDiscardRecovery(props.recoverable[0].id)}
            >
              <Trash2 size={14} />
            </button>
          </div>
        )}
        <div
          className="history-list"
          aria-label="录音列表"
          style={{ overflowY: "auto" }}
        >
          {filtered.length === 0 ? (
            <div className="history-empty">
              <FileAudio size={26} />
              <strong>{props.items.length ? "没有匹配的录音" : "还没有录音"}</strong>
              <span>完成的会议会显示在这里。</span>
            </div>
          ) : (
            filtered.map((item) => (
              <article
                key={item.id}
                className={`history-item ${item.id === props.selectedId ? "selected" : ""} ${item.id === actionMenu?.recordingId ? "menu-target" : ""}`}
                onContextMenu={(event) => {
                  event.preventDefault();
                  openContextMenu(item, event.clientX, event.clientY);
                }}
              >
                <button className="history-select" onClick={() => props.onSelect(item.id)}>
                  <span className="history-item-icon">
                    {item.transcription?.status === "completed" ? <Check size={15} /> : <FileAudio size={15} />}
                  </span>
                  <span className="history-item-main">
                    <strong>{item.title}</strong>
                    <small>
                      {new Date(item.createdAt).toLocaleDateString("zh-CN", {
                        month: "short",
                        day: "numeric",
                      })}
                      {" · "}{formatDuration(item.durationMs)}{" · "}{formatSize(item.sizeBytes)}
                    </small>
                    <span className={`transcription-badge status-${item.transcription?.status ?? "none"}`}>
                      {item.transcription ? transcriptionLabel(item.transcription) : "未转写"}
                      {item.transcription ? transcriptionProgressSuffix(item.transcription) : ""}
                    </span>
                  </span>
                </button>
                <button
                  className="history-quick-play"
                  aria-label={playing && selected?.id === item.id ? `暂停 ${item.title}` : `播放 ${item.title}`}
                  onClick={() => toggleQuickPlayback(item)}
                >
                  {playing && selected?.id === item.id
                    ? <Pause size={14} fill="currentColor" />
                    : <Play size={14} fill="currentColor" />}
                </button>
              </article>
            ))
          )}
        </div>
      </aside>

      <article className="transcript-pane">
        {props.recordingActive && (
          <div className="recording-active-banner">
            <span><i />录音仍在进行</span>
            <button onClick={props.onReturnToRecorder}>返回录音控制</button>
          </div>
        )}
        {!selected ? (
          <div className="transcript-empty">
            <Sparkles size={30} />
            <h2>选择一条录音</h2>
            <p>可以播放录音，并按需发送到你配置的语音转写服务。</p>
          </div>
        ) : (
          <>
            <header className="record-detail-header">
              <div>
                <p className="eyebrow">MEETING RECORD</p>
                <h2>{selected.title}</h2>
                <p>
                  {new Date(selected.createdAt).toLocaleString("zh-CN")}
                  {" · "}{formatDuration(selected.durationMs)}{" · "}{formatSize(selected.sizeBytes)}
                </p>
              </div>
              <div className="detail-actions">
                <button className="icon-button" title="打开所在文件夹" onClick={() => props.onReveal(selected.id)}>
                  <FolderOpen size={17} />
                </button>
                <button
                  ref={detailMenuButtonRef}
                  className="icon-button"
                  title="更多"
                  aria-haspopup="menu"
                  aria-expanded={actionMenu?.source === "detail" && actionMenu.recordingId === selected.id}
                  onClick={toggleDetailMenu}
                >
                  <MoreHorizontal size={17} />
                </button>
              </div>
            </header>

            <div className="unified-player">
              {audioLoading && <LoaderCircle className="spin player-loader" size={18} />}
              <audio
                ref={audioRef}
                src={audioSource || undefined}
                controls
                preload="metadata"
                onPlay={() => setPlaying(true)}
                onPause={() => setPlaying(false)}
                onEnded={() => setPlaying(false)}
                onError={() => audioSource && props.onPlaybackError("无法播放录音，文件可能已移动或格式不可用。")}
              />
            </div>

            <div className="transcript-toolbar">
              <div>
                <strong>文字转写</strong>
                {transcription && (
                  <span className={`transcription-badge status-${transcription.status}`}>
                    {transcriptionLabel(transcription)}
                  </span>
                )}
                {transcription?.providerName && (
                  <small>{transcription.providerName} · {transcription.modelId}</small>
                )}
              </div>
              <div className="transcript-actions">
                {isProcessing ? (
                  <button className="button secondary" onClick={() => props.onCancelTranscription(selected.id)}>
                    <Square size={13} fill="currentColor" />中断
                  </button>
                ) : transcription && resumableStatuses.includes(transcription.status) ? (
                  <button className="button primary" onClick={() => props.onResumeTranscription(selected.id)}>
                    <RotateCcw size={15} />继续转写
                  </button>
                ) : transcription?.status === "completed" ? (
                  <>
                    <button className="button secondary" onClick={() => props.onCopyTranscript(selected.id)}>
                      <Clipboard size={15} />复制全文
                    </button>
                    <button className="button secondary" onClick={() => props.onExportTranscript(selected.id, selected.title)}>
                      <Download size={15} />导出 TXT
                    </button>
                    <button className="text-button" onClick={() => props.onStartTranscription(selected.id)}>重新转写</button>
                  </>
                ) : (
                  <button
                    className="button primary"
                    disabled={!props.hasProvider}
                    title={props.hasProvider ? "" : "请先在设置中配置语音转写服务"}
                    onClick={() => props.onStartTranscription(selected.id)}
                  >
                    <Sparkles size={15} />开始转写
                  </button>
                )}
              </div>
            </div>

            {isProcessing && (
              <div className="transcription-progress">
                <LoaderCircle className="spin" size={18} />
                <div>
                  <strong>{transcriptionLabel(transcription!)}</strong>
                  <span>{transcriptionProgress(transcription!)}</span>
                </div>
                <progress
                  max={Math.max(
                    1,
                    transcription!.protocol === "nota_batch_v1"
                      ? transcription!.progressTotal
                      : transcription!.totalChunks,
                  )}
                  value={
                    transcription!.protocol === "nota_batch_v1"
                      ? transcription!.progressCurrent
                      : transcription!.completedChunks
                  }
                />
              </div>
            )}

            {transcription?.errorMessage && resumableStatuses.includes(transcription.status) && (
              <div className="transcript-error">
                <AlertCircle size={17} />
                <span>{transcription.errorMessage}</span>
              </div>
            )}

            <div className="transcript-body">
              {props.transcriptLoading ? (
                <div className="transcript-loading"><LoaderCircle className="spin" />读取转写结果…</div>
              ) : props.transcript?.segments.length ? (
                props.transcript.segments.map((segment, index) => (
                  <div className="transcript-segment" key={`${segment.startMs}-${index}`}>
                    <button onClick={() => seekTo(segment.startMs)}>
                      {formatDuration(segment.startMs)}
                    </button>
                    <div>
                      {segment.speaker && <strong>{segment.speaker}</strong>}
                      <p>{segment.text}</p>
                    </div>
                  </div>
                ))
              ) : props.transcript?.text ? (
                <div className="plain-transcript">{props.transcript.text}</div>
              ) : (
                <div className="transcript-placeholder">
                  <Sparkles size={24} />
                  <strong>{transcription ? statusLabels[transcription.status] : "尚未转写"}</strong>
                  <p>
                    {props.hasProvider
                      ? "点击“开始转写”后，Nota 会把录音发送到默认服务。"
                      : "请先在设置中添加并选择语音转写服务。"}
                  </p>
                </div>
              )}
            </div>
          </>
        )}
      </article>
      {actionMenu && actionMenuItem && (
        <div
          ref={actionMenuRef}
          className="recording-actions-menu"
          role="menu"
          aria-label={`${actionMenuItem.title} 操作`}
          style={{ left: actionMenu.left, top: actionMenu.top }}
          onContextMenu={(event) => event.preventDefault()}
        >
          <button
            role="menuitem"
            onClick={() => runMenuAction((item) => props.onRename(item.id, item.title))}
          >
            重命名
          </button>
          <button
            role="menuitem"
            onClick={() => runMenuAction((item) => props.onDelete(item.id))}
          >
            移至回收站
          </button>
          <button
            className="danger"
            role="menuitem"
            onClick={() => runMenuAction((item) => props.onPermanentDelete(item.id))}
          >
            永久删除
          </button>
        </div>
      )}
    </section>
  );
}
