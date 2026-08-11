import { convertFileSrc } from "@tauri-apps/api/core";
import {
  AlertCircle,
  Check,
  Clipboard,
  Download,
  FileAudio,
  FileUp,
  Fingerprint,
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
  X,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type {
  AsrProviderKind,
  AudioImportBatchSnapshot,
  LlmProvider,
  ParticipantProfile,
  RecordingItem,
  SpeakerIdentificationAssignment,
  SpeakerIdentificationSession,
  TranscriptDocument,
  TranscriptionStatus,
  TranscriptionSummary,
} from "../types";
import { AiDocumentsPanel } from "./AiDocumentsPanel";
import { AppTooltip } from "./AppTooltip";
import {
  SpeakerIdentificationModal,
  type SpeakerAnalysisStatus,
  type SpeakerManagementSpeaker,
  type SpeakerPreviewRequest,
} from "./SpeakerIdentificationModal";
import { TranscriptionOptionsModal } from "./TranscriptionOptionsModal";

interface RecordingsWorkspaceProps {
  items: RecordingItem[];
  recoverable: RecordingItem[];
  selectedId: string | null;
  transcript: TranscriptDocument | null;
  transcriptLoading: boolean;
  recordingActive: boolean;
  audioImport: AudioImportBatchSnapshot | null;
  hasProvider: boolean;
  activeProviderKind: AsrProviderKind | null;
  hasVoiceprintProvider: boolean;
  llmProviders: LlmProvider[];
  activeLlmProviderId: string | null;
  participants: ParticipantProfile[];
  onSelect: (id: string) => void;
  onReturnToRecorder: () => void;
  onImportAudio: () => void;
  onCancelAudioImport: () => void;
  onDismissAudioImport: () => void;
  onPreparePlayback: (id: string) => Promise<string>;
  onPlaybackError: (message: string) => void;
  onStartTranscription: (id: string, speakerCount: number | null) => void;
  onResumeTranscription: (id: string) => void;
  onCancelTranscription: (id: string) => void;
  onCopyTranscript: (id: string) => void;
  onExportTranscript: (id: string, title: string) => void;
  onIdentifySpeakers: (id: string) => Promise<SpeakerIdentificationSession>;
  onSaveSpeakerIdentification: (
    sessionId: string,
    assignments: SpeakerIdentificationAssignment[],
  ) => Promise<void>;
  onUpdateSpeakerAssignments: (
    recordingId: string,
    assignments: SpeakerIdentificationAssignment[],
  ) => Promise<void>;
  onDiscardSpeakerIdentification: (sessionId: string) => void;
  onOpenVoiceprintSettings: () => void;
  onReveal: (id: string) => void;
  onDelete: (id: string) => void;
  onRecover: (id: string) => void;
  onDiscardRecovery: (id: string) => void;
  onRename: (id: string, currentTitle: string) => void;
  onPermanentDelete: (id: string) => void;
  onAiMessage: (type: "success" | "error", message: string) => void;
}

interface RecordingActionMenu {
  recordingId: string;
  left: number;
  top: number;
  source: "context" | "detail";
}

interface SpeakerIdentificationDialog {
  recordingId: string;
  initialSpeaker: string | null;
  session: SpeakerIdentificationSession | null;
  analysisStatus: SpeakerAnalysisStatus;
  analysisError: string | null;
}

const actionMenuWidth = 148;
const actionMenuMargin = 8;
const actionMenuHeight = (source: RecordingActionMenu["source"]) =>
  source === "context" ? 164 : 126;

const constrainActionMenuPosition = (
  left: number,
  top: number,
  source: RecordingActionMenu["source"],
) => ({
  left: Math.max(
    actionMenuMargin,
    Math.min(left, window.innerWidth - actionMenuWidth - actionMenuMargin),
  ),
  top: Math.max(
    actionMenuMargin,
    Math.min(top, window.innerHeight - actionMenuHeight(source) - actionMenuMargin),
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

const formatTranscriptTimestamp = (milliseconds: number) => {
  const seconds = Math.floor(Math.max(milliseconds, 0) / 1000);
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const rest = seconds % 60;
  return [hours, minutes, rest]
    .map((value) => value.toString().padStart(2, "0"))
    .join(":");
};

const formatSize = (bytes: number) =>
  bytes < 1024 * 1024
    ? `${Math.max(1, Math.round(bytes / 1024))} KB`
    : `${(bytes / 1024 / 1024).toFixed(1)} MB`;

const audioImportStatusLabel = (status: AudioImportBatchSnapshot["items"][number]["status"]) => {
  switch (status) {
    case "queued": return "等待导入";
    case "probing": return "正在检查文件";
    case "decoding": return "正在转换音频";
    case "finalizing": return "正在安全保存";
    case "completed": return "导入完成";
    case "failed": return "导入失败";
    case "skipped": return "已存在，已跳过";
    case "cancelled": return "已取消";
  }
};

const selectRepresentativeUtterances = (
  utterances: SpeakerManagementSpeaker["utterances"],
) => [...utterances]
  .sort((left, right) => (right.endMs - right.startMs) - (left.endMs - left.startMs))
  .slice(0, 5)
  .sort((left, right) => left.startMs - right.startMs);

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
  const [activeSpeakerPreview, setActiveSpeakerPreview] = useState<SpeakerPreviewRequest | null>(null);
  const [actionMenu, setActionMenu] = useState<RecordingActionMenu | null>(null);
  const [identification, setIdentification] = useState<SpeakerIdentificationDialog | null>(null);
  const [identificationSaving, setIdentificationSaving] = useState(false);
  const [transcriptionOptions, setTranscriptionOptions] = useState<{
    recordingId: string;
    recordingTitle: string;
    retranscription: boolean;
  } | null>(null);
  const [detailTab, setDetailTab] = useState<"transcript" | "ai">("transcript");
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const actionMenuRef = useRef<HTMLDivElement | null>(null);
  const detailMenuButtonRef = useRef<HTMLButtonElement | null>(null);
  const identificationRequestRef = useRef(0);
  const selected = props.items.find((item) => item.id === props.selectedId) ?? null;
  const actionMenuItem = props.items.find((item) => item.id === actionMenu?.recordingId) ?? null;
  const filtered = useMemo(() => {
    const normalized = query.trim().toLocaleLowerCase();
    return normalized
      ? props.items.filter((item) => item.title.toLocaleLowerCase().includes(normalized))
      : props.items;
  }, [props.items, query]);
  const managementSpeakers = useMemo(() => {
    if (!props.transcript || props.transcript.recordingId !== selected?.id) return [];
    const grouped = new Map<string, SpeakerManagementSpeaker>();
    for (const segment of props.transcript.segments) {
      const rawSpeaker = segment.speaker?.trim();
      if (!rawSpeaker) continue;
      const assignment = props.transcript.speakerAssignments[rawSpeaker];
      const current = grouped.get(rawSpeaker) ?? {
        rawSpeaker,
        currentParticipantId: assignment?.participantId ?? null,
        currentDisplayName: assignment?.displayName
          ?? props.transcript.speakerNames[rawSpeaker]
          ?? null,
        totalSpeechMs: 0,
        utterances: [],
      };
      const duration = Math.max(0, segment.endMs - segment.startMs);
      current.totalSpeechMs += duration;
      if (duration > 0 && segment.text.trim()) {
        current.utterances.push({
          startMs: segment.startMs,
          endMs: segment.endMs,
          text: segment.text.trim(),
        });
      }
      grouped.set(rawSpeaker, current);
    }
    return [...grouped.values()].map((speaker) => ({
      ...speaker,
      utterances: selectRepresentativeUtterances(speaker.utterances),
    }));
  }, [props.transcript, selected?.id]);

  useEffect(() => {
    setDetailTab("transcript");
    identificationRequestRef.current += 1;
    setIdentification((current) => {
      if (current?.session) props.onDiscardSpeakerIdentification(current.session.id);
      return null;
    });
    // A different recording cannot reuse the previous extraction session.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.selectedId]);

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
    setActiveSpeakerPreview(null);
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
    // Renaming keeps the recording id but moves the managed file, so the
    // authoritative path is also part of the player source identity.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.selectedId, selected?.path]);

  useEffect(() => {
    const audio = audioRef.current;
    if (!audio || !audioSource || !selected) return;
    if (pendingSeekMs !== null) {
      audio.currentTime = pendingSeekMs / 1_000;
      setPendingSeekMs(null);
      void audio.play().catch((error) => {
        setActiveSpeakerPreview(null);
        props.onPlaybackError(String(error));
      });
      return;
    }
    if (pendingPlayId === selected.id) {
      setPendingPlayId(null);
      void audio.play().catch((error) => props.onPlaybackError(String(error)));
    }
  }, [audioSource, pendingPlayId, pendingSeekMs, props, selected]);

  const toggleQuickPlayback = (item: RecordingItem) => {
    const audio = audioRef.current;
    setActiveSpeakerPreview(null);
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
    setActiveSpeakerPreview(null);
    const audio = audioRef.current;
    if (!audio || !audioSource) {
      setPendingSeekMs(milliseconds);
      return;
    }
    audio.currentTime = milliseconds / 1_000;
    void audio.play().catch((error) => props.onPlaybackError(String(error)));
  };

  const stopSpeakerPreview = () => {
    if (activeSpeakerPreview && audioRef.current && !audioRef.current.paused) {
      audioRef.current.pause();
    }
    setActiveSpeakerPreview(null);
  };

  const toggleSpeakerPreview = (preview: SpeakerPreviewRequest) => {
    const audio = audioRef.current;
    const samePreview = activeSpeakerPreview?.id === preview.id;
    if (samePreview && audio && playing) {
      audio.pause();
      return;
    }
    setActiveSpeakerPreview(preview);
    if (!audio || !audioSource) {
      setPendingSeekMs(preview.startMs);
      return;
    }
    const currentMs = audio.currentTime * 1_000;
    if (!samePreview || currentMs < preview.startMs || currentMs >= preview.endMs) {
      audio.currentTime = preview.startMs / 1_000;
    }
    void audio.play().catch((error) => {
      setActiveSpeakerPreview((current) => current?.id === preview.id ? null : current);
      props.onPlaybackError(String(error));
    });
  };

  const analyzeSpeakers = async (recordingId: string) => {
    if (!props.hasVoiceprintProvider) return;
    const previousSession = identification?.recordingId === recordingId
      ? identification.session
      : null;
    if (previousSession) {
      props.onDiscardSpeakerIdentification(previousSession.id);
    }
    const requestId = ++identificationRequestRef.current;
    setIdentification((current) => current?.recordingId === recordingId
      ? { ...current, session: null, analysisStatus: "loading", analysisError: null }
      : current);
    try {
      const session = await props.onIdentifySpeakers(recordingId);
      if (identificationRequestRef.current !== requestId) {
        props.onDiscardSpeakerIdentification(session.id);
        return;
      }
      setIdentification((current) => current?.recordingId === recordingId
        ? { ...current, session, analysisStatus: "ready", analysisError: null }
        : current);
    } catch (error) {
      if (identificationRequestRef.current !== requestId) return;
      setIdentification((current) => current?.recordingId === recordingId
        ? { ...current, session: null, analysisStatus: "failed", analysisError: String(error) }
        : current);
    }
  };

  const openSpeakerManagement = (initialSpeaker: string | null) => {
    if (!selected || managementSpeakers.length === 0) return;
    identificationRequestRef.current += 1;
    setIdentification({
      recordingId: selected.id,
      initialSpeaker,
      session: null,
      analysisStatus: "idle",
      analysisError: null,
    });
  };

  const closeSpeakerManagement = () => {
    identificationRequestRef.current += 1;
    stopSpeakerPreview();
    if (identification?.session) {
      props.onDiscardSpeakerIdentification(identification.session.id);
    }
    setIdentification(null);
  };

  const requestTranscription = (item: RecordingItem, retranscription: boolean) => {
    if (props.activeProviderKind === "funAsr") {
      setTranscriptionOptions({
        recordingId: item.id,
        recordingTitle: item.title,
        retranscription,
      });
      return;
    }
    if (props.activeProviderKind === "openAiCompatible") {
      props.onStartTranscription(item.id, null);
      return;
    }
    props.onPlaybackError("请先在设置中选择可用的语音转写服务");
  };

  const openContextMenu = (
    item: RecordingItem,
    clientX: number,
    clientY: number,
  ) => {
    setActionMenu({
      recordingId: item.id,
      ...constrainActionMenuPosition(clientX, clientY, "context"),
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
      ...constrainActionMenuPosition(
        bounds.right - actionMenuWidth,
        bounds.bottom + 6,
        "detail",
      ),
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
  const importActive = props.audioImport?.status === "running";
  const failedImportItem = props.audioImport?.items.find((item) => item.status === "failed");
  const currentImportItem = !importActive && failedImportItem
    ? failedImportItem
    : props.audioImport && props.audioImport.currentIndex > 0
      ? props.audioImport.items[props.audioImport.currentIndex - 1]
      : props.audioImport?.items[0];
  const importProgress = currentImportItem?.progressTotalMs
    ? Math.min(100, Math.round(
        currentImportItem.progressCurrentMs * 100 / currentImportItem.progressTotalMs,
      ))
    : null;

  return (
    <section className="library-workspace">
      <aside className="history-pane">
        <div className="history-header">
          <div>
            <p className="eyebrow">RECORDINGS</p>
            <h1>录音记录</h1>
          </div>
          <div className="history-header-actions">
            <AppTooltip
              content={props.recordingActive
                ? "停止并保存当前录音后才能导入"
                : importActive
                  ? "已有音频正在导入"
                  : ""}
              wrapDisabled={props.recordingActive || importActive}
            >
              <button
                className="button secondary compact import-audio-button"
                disabled={props.recordingActive || importActive}
                onClick={props.onImportAudio}
              >
                <FileUp size={14} />导入录音
              </button>
            </AppTooltip>
            <span className="count-pill">{props.items.length}</span>
          </div>
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
        {props.audioImport && (
          <section
            className={`audio-import-card status-${props.audioImport.status}`}
            aria-live="polite"
          >
            <div className="audio-import-card-header">
              <div>
                <strong>
                  {importActive
                    ? props.audioImport.currentIndex > 0
                      ? `正在导入 ${props.audioImport.currentIndex} / ${props.audioImport.total}`
                      : `准备导入 ${props.audioImport.total} 个文件`
                    : props.audioImport.status === "cancelled"
                      ? "导入已停止"
                      : "导入任务已完成"}
                </strong>
                {currentImportItem && <span title={currentImportItem.fileName}>{currentImportItem.fileName}</span>}
              </div>
              {importActive ? (
                <AppTooltip content="停止当前和等待中的导入">
                  <button
                    className="icon-button compact"
                    aria-label="停止导入"
                    onClick={props.onCancelAudioImport}
                  >
                    <Square size={12} fill="currentColor" />
                  </button>
                </AppTooltip>
              ) : (
                <AppTooltip content="关闭导入状态">
                  <button
                    className="icon-button compact"
                    aria-label="关闭导入状态"
                    onClick={props.onDismissAudioImport}
                  >
                    <X size={14} />
                  </button>
                </AppTooltip>
              )}
            </div>
            {importActive && (
              <div
                className={`audio-import-progress ${importProgress === null ? "indeterminate" : ""}`}
                role="progressbar"
                aria-label="音频导入进度"
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={importProgress ?? undefined}
              >
                <i style={importProgress === null ? undefined : { width: `${importProgress}%` }} />
              </div>
            )}
            <small>
              {currentImportItem ? audioImportStatusLabel(currentImportItem.status) : "正在准备"}
              {importProgress === null || !importActive ? "" : ` · ${importProgress}%`}
              {!importActive
                ? ` · 成功 ${props.audioImport.completed}，跳过 ${props.audioImport.skipped}，失败 ${props.audioImport.failed}`
                : ""}
            </small>
            {currentImportItem?.status === "failed" && currentImportItem.errorMessage && (
              <p>{currentImportItem.errorMessage}</p>
            )}
          </section>
        )}
        {props.recoverable.length > 0 && (
          <div className="compact-recovery">
            <RotateCcw size={16} />
            <span>{props.recoverable.length} 个录音可恢复</span>
            <button onClick={() => props.onRecover(props.recoverable[0].id)}>恢复</button>
            <AppTooltip content="删除恢复文件">
              <button
                aria-label="删除恢复文件"
                onClick={() => props.onDiscardRecovery(props.recoverable[0].id)}
              >
                <Trash2 size={14} />
              </button>
            </AppTooltip>
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
              <span>
                {props.items.length
                  ? "换一个关键词试试。"
                  : "开始一次录音，或导入手机中的会议录音。"}
              </span>
              {!props.items.length && (
                <button
                  className="button primary compact"
                  disabled={props.recordingActive || importActive}
                  onClick={props.onImportAudio}
                >
                  <FileUp size={14} />导入录音
                </button>
              )}
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
                <AppTooltip content={playing && selected?.id === item.id ? "暂停" : "播放"} side="left">
                  <button
                    className="history-quick-play"
                    aria-label={playing && selected?.id === item.id ? `暂停 ${item.title}` : `播放 ${item.title}`}
                    onClick={() => toggleQuickPlayback(item)}
                  >
                    {playing && selected?.id === item.id
                      ? <Pause size={14} fill="currentColor" />
                      : <Play size={14} fill="currentColor" />}
                  </button>
                </AppTooltip>
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
                  {selected.origin === "imported" && (
                    <span className="import-origin-badge">
                      导入{selected.sourceFormat ? ` · ${selected.sourceFormat}` : ""}
                    </span>
                  )}
                </p>
                {selected.origin === "imported" && selected.sourceFileName && (
                  <small className="import-source-name" title={selected.sourceFileName}>
                    原文件：{selected.sourceFileName}
                  </small>
                )}
              </div>
              <div className="detail-actions">
                <AppTooltip content="打开所在文件夹">
                  <button className="icon-button" aria-label="打开录音所在文件夹" onClick={() => props.onReveal(selected.id)}>
                    <FolderOpen size={17} />
                  </button>
                </AppTooltip>
                <AppTooltip content="更多操作">
                  <button
                    ref={detailMenuButtonRef}
                    className="icon-button"
                    aria-label="更多录音操作"
                    aria-haspopup="menu"
                    aria-expanded={actionMenu?.source === "detail" && actionMenu.recordingId === selected.id}
                    onClick={toggleDetailMenu}
                  >
                    <MoreHorizontal size={17} />
                  </button>
                </AppTooltip>
              </div>
            </header>

            <div className="record-detail-sticky-controls">
              <div className="unified-player">
                {audioLoading && <LoaderCircle className="spin player-loader" size={18} />}
                <audio
                  ref={audioRef}
                  src={audioSource || undefined}
                  controls
                  preload="metadata"
                  onPlay={() => setPlaying(true)}
                  onPause={() => setPlaying(false)}
                  onEnded={() => {
                    setPlaying(false);
                    setActiveSpeakerPreview(null);
                  }}
                  onSeeked={() => {
                    const audio = audioRef.current;
                    if (!audio || !activeSpeakerPreview) return;
                    const currentMs = audio.currentTime * 1_000;
                    if (currentMs < activeSpeakerPreview.startMs
                      || currentMs >= activeSpeakerPreview.endMs) {
                      setActiveSpeakerPreview(null);
                    }
                  }}
                  onTimeUpdate={() => {
                    const audio = audioRef.current;
                    if (audio && activeSpeakerPreview
                      && audio.currentTime * 1_000 >= activeSpeakerPreview.endMs) {
                      setActiveSpeakerPreview(null);
                      audio.pause();
                    }
                  }}
                  onError={() => {
                    setActiveSpeakerPreview(null);
                    if (audioSource) {
                      props.onPlaybackError("无法播放录音，文件可能已移动或格式不可用。");
                    }
                  }}
                />
              </div>

              <div className="record-detail-tabs" role="tablist" aria-label="会议详情">
                <button
                  role="tab"
                  aria-selected={detailTab === "transcript"}
                  className={detailTab === "transcript" ? "active" : ""}
                  onClick={() => setDetailTab("transcript")}
                >
                  文字转写
                </button>
                <button
                  role="tab"
                  aria-selected={detailTab === "ai"}
                  className={detailTab === "ai" ? "active" : ""}
                  onClick={() => setDetailTab("ai")}
                >
                  AI 文档
                </button>
              </div>

              {detailTab === "transcript" && (
              <div className="transcript-toolbar">
                <div>
                  <strong>文字转写</strong>
                  {transcription && (
                    <span className={`transcription-badge status-${transcription.status}`}>
                      {transcriptionLabel(transcription)}
                    </span>
                  )}
                  {transcription?.providerName && (
                    <small>
                      {transcription.providerName} · {transcription.modelId}
                      {transcription.protocol === "nota_batch_v1"
                        ? ` · ${transcription.speakerCount === null
                          ? "自动判断人数"
                          : `目标 ${transcription.speakerCount} 人（安全优先）`}`
                        : ""}
                    </small>
                  )}
                </div>
                <div className="transcript-actions">
                  {isProcessing ? (
                    <button className="button secondary compact" onClick={() => props.onCancelTranscription(selected.id)}>
                      <Square size={13} fill="currentColor" />中断
                    </button>
                  ) : transcription && resumableStatuses.includes(transcription.status) ? (
                    <button className="button primary compact" onClick={() => props.onResumeTranscription(selected.id)}>
                      <RotateCcw size={15} />继续转写
                    </button>
                  ) : transcription?.status === "completed" ? (
                    <>
                      <button className="button secondary compact" onClick={() => props.onCopyTranscript(selected.id)}>
                        <Clipboard size={15} />复制全文
                      </button>
                      <button className="button secondary compact" onClick={() => props.onExportTranscript(selected.id, selected.title)}>
                        <Download size={15} />导出 TXT
                      </button>
                      <AppTooltip
                        content={props.hasVoiceprintProvider
                          ? "管理当前会议说话人并按需分析声纹"
                          : "可以手动管理姓名；配置 Nota ASR Server 后可分析声纹"}
                        wrapDisabled={managementSpeakers.length === 0}
                      >
                        <button
                          className="button secondary compact"
                          disabled={managementSpeakers.length === 0}
                          onClick={() => openSpeakerManagement(null)}
                        >
                          <Fingerprint size={15} />
                          {Object.keys(props.transcript?.speakerAssignments ?? {}).length > 0
                            ? "管理说话人"
                            : "说话人识别"}
                        </button>
                      </AppTooltip>
                      <button className="text-button" onClick={() => requestTranscription(selected, true)}>重新转写</button>
                    </>
                  ) : (
                    <AppTooltip
                      content={props.hasProvider ? "" : "请先在设置中配置语音转写服务"}
                      wrapDisabled={!props.hasProvider}
                    >
                      <button
                        className="button primary compact"
                        disabled={!props.hasProvider}
                        onClick={() => requestTranscription(selected, false)}
                      >
                        <Sparkles size={15} />开始转写
                      </button>
                    </AppTooltip>
                  )}
                </div>
              </div>
              )}
            </div>

            {detailTab === "transcript" ? (
            <>
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
                      {formatTranscriptTimestamp(segment.startMs)}
                    </button>
                    <div>
                      {segment.speaker && (
                        <AppTooltip content={`管理 ${segment.speaker} 的姓名`} side="right">
                          <button
                            className="speaker-label-button"
                            onClick={() => openSpeakerManagement(segment.speaker)}
                          >
                            {props.transcript?.speakerNames?.[segment.speaker] ?? segment.speaker}
                          </button>
                        </AppTooltip>
                      )}
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
            ) : (
              <AiDocumentsPanel
                recording={selected}
                transcript={props.transcript}
                providers={props.llmProviders}
                activeProviderId={props.activeLlmProviderId}
                onMessage={props.onAiMessage}
              />
            )}
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
          {actionMenu.source === "context" && (
            <>
              <button
                role="menuitem"
                onClick={() => runMenuAction((item) => props.onReveal(item.id))}
              >
                打开所在文件夹
              </button>
              <div className="recording-actions-separator" role="separator" />
            </>
          )}
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
      {identification && (
        <SpeakerIdentificationModal
          speakers={managementSpeakers}
          session={identification.session}
          analysisStatus={identification.analysisStatus}
          analysisError={identification.analysisError}
          participants={props.participants}
          initialSpeaker={identification.initialSpeaker}
          saving={identificationSaving}
          canAnalyzeVoiceprints={props.hasVoiceprintProvider}
          activePreviewId={activeSpeakerPreview?.id ?? null}
          previewPlaying={Boolean(activeSpeakerPreview && playing)}
          onAnalyze={() => void analyzeSpeakers(identification.recordingId)}
          onConfigureVoiceprints={() => {
            closeSpeakerManagement();
            props.onOpenVoiceprintSettings();
          }}
          onPreview={toggleSpeakerPreview}
          onStopPreview={stopSpeakerPreview}
          onCancel={closeSpeakerManagement}
          onSave={(request) => {
            const session = identification.session;
            if (request.saveVoiceprints && !session) {
              props.onPlaybackError("声纹分析会话已经失效，请重新分析后再保存声纹。");
              return;
            }
            if (identification.analysisStatus === "loading") {
              identificationRequestRef.current += 1;
              setIdentification((current) => current
                ? { ...current, analysisStatus: "idle", analysisError: null }
                : current);
            }
            setIdentificationSaving(true);
            const save = request.saveVoiceprints && session
              ? props.onSaveSpeakerIdentification(session.id, request.sessionAssignments)
              : props.onUpdateSpeakerAssignments(
                identification.recordingId,
                request.mappingAssignments,
              );
            void save
              .then(() => {
                identificationRequestRef.current += 1;
                stopSpeakerPreview();
                if (session && !request.saveVoiceprints) {
                  props.onDiscardSpeakerIdentification(session.id);
                }
                setIdentification(null);
              })
              .catch((error) => props.onPlaybackError(String(error)))
              .finally(() => setIdentificationSaving(false));
          }}
        />
      )}
      {transcriptionOptions && (
        <TranscriptionOptionsModal
          recordingTitle={transcriptionOptions.recordingTitle}
          retranscription={transcriptionOptions.retranscription}
          onCancel={() => setTranscriptionOptions(null)}
          onConfirm={(speakerCount) => {
            const recordingId = transcriptionOptions.recordingId;
            setTranscriptionOptions(null);
            props.onStartTranscription(recordingId, speakerCount);
          }}
        />
      )}
    </section>
  );
}
