import { convertFileSrc } from "@tauri-apps/api/core";
import {
  AlertCircle,
  Clipboard,
  Download,
  FileAudio,
  FileUp,
  FolderOpen,
  LoaderCircle,
  MoreHorizontal,
  Pause,
  PanelLeftClose,
  PanelLeftOpen,
  Play,
  RotateCcw,
  Search,
  Sparkles,
  Square,
  Trash2,
  Users,
  X,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type {
  AsrProviderKind,
  HotwordListSummary,
  AudioImportBatchSnapshot,
  LlmProvider,
  ParticipantProfile,
  RecordingItem,
  SpeakerIdentificationAssignment,
  SpeakerIdentificationSession,
  TranscriptDocument,
  TranscriptionStatus,
  TranscriptionSummary,
  TranscriptionVersionSummary,
  TranscriptionOptions,
} from "../types";
import { DetailActionPopover } from "./DetailActionPopover";
import { DetailSelect } from "./DetailSelect";
import { AiDocumentsPanel } from "./AiDocumentsPanel";
import { AppTooltip } from "./AppTooltip";
import type { ToastOptions } from "./ToastRegion";
import {
  SpeakerIdentificationModal,
  type SpeakerAnalysisStatus,
  type SpeakerManagementSpeaker,
  type SpeakerPreviewRequest,
  type VoiceprintAvailability,
} from "./SpeakerIdentificationModal";
import { TranscriptionOptionsModal } from "./TranscriptionOptionsModal";

interface RecordingsWorkspaceProps {
  items: RecordingItem[];
  recoverable: RecordingItem[];
  selectedId: string | null;
  transcript: TranscriptDocument | null;
  transcriptionVersions: TranscriptionVersionSummary[];
  transcriptLoading: boolean;
  recordingActive: boolean;
  audioImport: AudioImportBatchSnapshot | null;
  hasProvider: boolean;
  activeProviderKind: AsrProviderKind | null;
  activeProviderName?: string | null;
  activeProviderId?: string | null;
  hotwordLists?: HotwordListSummary[];
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
  onStartTranscription: (id: string, speakerCount: number | null, hotwordListId: string | null) => void;
  onGetTranscriptionOptions?: (providerId: string) => Promise<TranscriptionOptions>;
  onOpenHotwordLibrary?: () => void;
  onResumeTranscription: (id: string) => void;
  onCancelTranscription: (id: string) => void;
  onSelectTranscriptionVersion: (id: string, generation: number) => Promise<void>;
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
  onAiMessage: (type: "success" | "error", message: string, options?: ToastOptions) => void;
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

const formatVersionDate = (value: string) => new Intl.DateTimeFormat("zh-CN", {
  month: "2-digit",
  day: "2-digit",
  hour: "2-digit",
  minute: "2-digit",
}).format(new Date(value));

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
  transcription.protocol !== "legacy_chunks"
  && processingStatuses.includes(transcription.status)
  && transcription.progressPhase
    ? batchPhaseLabels[transcription.progressPhase]
    : statusLabels[transcription.status];

const transcriptionProgress = (transcription: TranscriptionSummary) => {
  if (transcription.protocol === "legacy_chunks") {
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
  if (transcription.protocol !== "legacy_chunks") {
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
    options: TranscriptionOptions;
  } | null>(null);
  const [focused, setFocused] = useState(false);
  const [aiVisited, setAiVisited] = useState(false);
  const focusButtonRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    const escape = (event: globalThis.KeyboardEvent) => {
      // Tooltips may consume Escape; only task overlays take precedence over focus mode.
      if (event.key !== "Escape" || document.querySelector('dialog[open], [role="dialog"], details[open], [role="menu"]')) return;
      setFocused(false);
      focusButtonRef.current?.focus();
    };
    if (focused) document.addEventListener("keydown", escape);
    return () => document.removeEventListener("keydown", escape);
  }, [focused]);
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
    if (props.activeProviderKind === "dashScope" && item.durationMs > 7_200_000) {
      props.onPlaybackError(
        "该录音超过 2 小时，无法使用千问云转写；Nota 不会自动切片或关闭说话人分离。",
      );
      return;
    }
    if (!props.onGetTranscriptionOptions) {
      if (props.activeProviderKind === "openAiCompatible") {
        (props.onStartTranscription as (id: string, count: number | null) => void)(item.id, null);
        return;
      }
      if (props.activeProviderKind === "funAsr" || props.activeProviderKind === "dashScope") {
        setTranscriptionOptions({
          recordingId: item.id,
          recordingTitle: item.title,
          retranscription,
          options: {
            providerId: "active",
            providerName: props.activeProviderName ?? "当前转写服务",
            providerKind: props.activeProviderKind,
            modelId: "",
            speakerCountMin: props.activeProviderKind === "dashScope" ? 2 : 1,
            speakerCountMax: props.activeProviderKind === "dashScope" ? 100 : 64,
            cloudUpload: props.activeProviderKind === "dashScope",
            maxReliableAudioSeconds: props.activeProviderKind === "dashScope" ? 7200 : null,
            hotwords: {
              supported: false,
              mode: "unsupported",
              maxEntries: 0,
              maxEntryChars: 0,
              weightsSupported: false,
              defaultWeight: null,
              allowedWeights: [],
              superHotwordWeight: null,
              maxSuperHotwords: null,
            },
          },
        });
        return;
      }
    }
    if (props.activeProviderKind) {
      const fallback: TranscriptionOptions = {
        providerId: props.activeProviderId ?? "active",
        providerName: props.activeProviderName ?? "当前转写服务",
        providerKind: props.activeProviderKind,
        modelId: "",
        speakerCountMin: props.activeProviderKind === "openAiCompatible" ? null : props.activeProviderKind === "dashScope" ? 2 : 1,
        speakerCountMax: props.activeProviderKind === "openAiCompatible" ? null : props.activeProviderKind === "dashScope" ? 100 : 64,
        cloudUpload: props.activeProviderKind === "dashScope",
        maxReliableAudioSeconds: props.activeProviderKind === "dashScope" ? 7200 : null,
        hotwords: {
          supported: false,
          mode: "unsupported",
          maxEntries: 0,
          maxEntryChars: 0,
          weightsSupported: false,
          defaultWeight: null,
          allowedWeights: [],
          superHotwordWeight: null,
          maxSuperHotwords: null,
        },
      };
      const request = props.onGetTranscriptionOptions
        ? props.onGetTranscriptionOptions(props.activeProviderId ?? "")
        : Promise.resolve(fallback);
      void request
        .then((options) => setTranscriptionOptions({
          recordingId: item.id,
          recordingTitle: item.title,
          retranscription,
          options,
        }))
        .catch((error) => props.onPlaybackError(String(error)));
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
  const requiresPaidRetryConfirmation = Boolean(
    transcription?.errorMessage
    && (
      transcription.errorMessage.includes("可能重复计费")
      || transcription.errorMessage.includes("不会自动重试")
      || transcription.errorMessage.includes("重新转写可能产生费用")
    ),
  );
  const voiceprintAvailability: VoiceprintAvailability =
    props.transcript && !props.transcript.voiceprintAnalysisSupported
      ? {
          kind: "transcriptionProviderUnsupported",
          providerName: props.transcript.providerName,
        }
      : props.hasVoiceprintProvider
        ? { kind: "available" }
        : { kind: "providerNotConfigured" };
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
    <section className={`library-workspace ${focused ? "is-focused" : ""}`}>
      <aside id="recording-history-panel" className="history-pane" hidden={focused}>
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
            <span className="history-count" aria-label={`${props.items.length} 条录音`}>
              {props.items.length} 条
            </span>
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
                className={`history-item ${item.id === props.selectedId ? "selected" : ""} ${playing && selected?.id === item.id ? "is-playing" : ""} ${item.id === actionMenu?.recordingId ? "menu-target" : ""}`}
                onContextMenu={(event) => {
                  event.preventDefault();
                  openContextMenu(item, event.clientX, event.clientY);
                }}
              >
                <button
                  className="history-select"
                  aria-keyshortcuts="Space"
                  onClick={() => props.onSelect(item.id)}
                  onDoubleClick={() => toggleQuickPlayback(item)}
                  onKeyDown={(event) => {
                    if (event.key !== " ") return;
                    event.preventDefault();
                    toggleQuickPlayback(item);
                  }}
                >
                  <span className="history-item-main">
                    <span className="history-item-title" title={item.title}>{item.title}</span>
                    <small className="history-item-meta">
                      <span className="history-item-facts">
                        {new Date(item.createdAt).toLocaleDateString("zh-CN", {
                          month: "short",
                          day: "numeric",
                        })}
                        {" · "}{formatDuration(item.durationMs)}
                      </span>
                      <span className={`transcription-badge history-transcription-status status-${item.transcription?.status ?? "none"}`}>
                        {item.transcription ? transcriptionLabel(item.transcription) : "未转写"}
                        {item.transcription ? transcriptionProgressSuffix(item.transcription) : ""}
                      </span>
                    </small>
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
              <AppTooltip content={focused ? "展开录音列表" : "折叠录音列表"}>
                <button
                  ref={focusButtonRef}
                  className="icon-button record-list-toggle"
                  aria-label={focused ? "展开录音列表" : "折叠录音列表"}
                  aria-expanded={!focused}
                  aria-controls="recording-history-panel"
                  onClick={() => setFocused(!focused)}
                >
                  {focused ? <PanelLeftOpen size={17} /> : <PanelLeftClose size={17} />}
                </button>
              </AppTooltip>
              <div className="record-detail-heading">
                <h2 title={selected.title}>{selected.title}</h2>
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
                  <details className="record-source-details"><summary>原文件信息</summary><p>原文件：{selected.sourceFileName}</p></details>
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

            <div className="record-detail-controls">
              <div className="app-tab-bar" role="tablist" aria-label="会议详情" onKeyDown={(event) => {
                if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
                event.preventDefault();
                const next = event.key === "Home" ? "transcript" : event.key === "End" ? "ai" : detailTab === "ai" ? "transcript" : "ai";
                if (next === "ai") setAiVisited(true);
                setDetailTab(next);
                document.getElementById(`record-${next}-tab`)?.focus();
              }}>
                <button
                  role="tab"
                  id="record-transcript-tab"
                  aria-controls="record-transcript-panel"
                  tabIndex={detailTab === "transcript" ? 0 : -1}
                  aria-selected={detailTab === "transcript"}
                  className={detailTab === "transcript" ? "active" : ""}
                  onClick={() => setDetailTab("transcript")}
                >
                  文字转写
                </button>
                <button
                  role="tab"
                  id="record-ai-tab"
                  aria-controls="record-ai-panel"
                  tabIndex={detailTab === "ai" ? 0 : -1}
                  aria-selected={detailTab === "ai"}
                  className={detailTab === "ai" ? "active" : ""}
                  onClick={() => { setAiVisited(true); setDetailTab("ai"); }}
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
                  {props.transcript?.providerName && (
                    <small>
                      {props.transcript.providerName} · {props.transcript.modelId}
                      {props.transcript.protocol !== "legacy_chunks"
                        ? ` · ${props.transcript.speakerCount === null
                          ? "自动判断人数"
                          : `目标 ${props.transcript.speakerCount} 人（安全优先）`}`
                        : ""}
                    </small>
                  )}
                </div>
                <div className="transcript-actions">
                  {props.transcript && props.transcriptionVersions.length > 1 && (
                    <label className="transcription-version-select">
                      <span>转写版本</span>
                      <DetailSelect
                        aria-label="转写版本"
                        disabled={props.transcriptLoading}
                        value={props.transcript.generation}
                        onChange={(event) => void props.onSelectTranscriptionVersion(
                          selected.id,
                          Number(event.target.value),
                        )}
                      >
                        {props.transcriptionVersions.map((version) => (
                          <option key={version.generation} value={version.generation}>
                            {`第 ${version.generation} 次 · ${version.providerName} · ${version.hotwordListName
                              ? `${version.hotwordListName}（${version.hotwordCount ?? 0} 个词） · `
                              : "未使用热词 · "}${formatVersionDate(version.completedAt)}`}
                          </option>
                        ))}
                      </DetailSelect>
                    </label>
                  )}
                  {isProcessing ? (
                    <button className="button secondary compact" onClick={() => props.onCancelTranscription(selected.id)}>
                      <Square size={13} fill="currentColor" />中断
                    </button>
                  ) : transcription && resumableStatuses.includes(transcription.status)
                    && requiresPaidRetryConfirmation ? (
                    <button
                      className="button primary compact"
                      onClick={() => {
                        if (!confirm(
                          "原 DashScope 任务的提交或结果状态无法确认。重新创建任务可能重复计费，仍要继续吗？",
                        )) return;
                        requestTranscription(selected, true);
                      }}
                    >
                      <RotateCcw size={15} />重新创建任务
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
                      <DetailActionPopover label="更多转写操作">
                      <button className="button secondary compact" onClick={() => props.onExportTranscript(selected.id, selected.title)}>
                        <Download size={15} />导出 TXT
                      </button>
                      <AppTooltip
                        content={voiceprintAvailability.kind === "transcriptionProviderUnsupported"
                          ? `管理匿名说话人与姓名；${voiceprintAvailability.providerName}生成的本次转写不支持 Nota 声纹分析`
                          : voiceprintAvailability.kind === "available"
                            ? "管理当前会议说话人并按需分析声纹"
                            : "可以手动管理姓名；配置 Nota ASR Server 后可分析声纹"}
                        wrapDisabled={managementSpeakers.length === 0}
                      >
                        <button
                          className="button secondary compact"
                          disabled={managementSpeakers.length === 0}
                          onClick={() => openSpeakerManagement(null)}
                        >
                          <Users size={15} />
                          管理说话人
                        </button>
                      </AppTooltip>
                      <button className="text-button" onClick={() => requestTranscription(selected, true)}>重新转写</button>
                      </DetailActionPopover>
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

            <div id="record-transcript-panel" role="tabpanel" aria-labelledby="record-transcript-tab" className="record-transcript-content" hidden={detailTab !== "transcript"}>
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
                    transcription!.protocol !== "legacy_chunks"
                      ? transcription!.progressTotal
                      : transcription!.totalChunks,
                  )}
                  value={
                    transcription!.protocol !== "legacy_chunks"
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
            </div>
            {(aiVisited || detailTab === "ai") && <div id="record-ai-panel" role="tabpanel" aria-labelledby="record-ai-tab" className="record-ai-content" hidden={detailTab !== "ai"}>
              <AiDocumentsPanel
                recording={selected}
                transcript={props.transcript}
                providers={props.llmProviders}
                activeProviderId={props.activeLlmProviderId}
                onMessage={props.onAiMessage}
              />
            </div>}
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
          voiceprintAvailability={voiceprintAvailability}
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
          providerName={transcriptionOptions.options.providerName}
          speakerCountMin={transcriptionOptions.options.speakerCountMin}
          speakerCountMax={transcriptionOptions.options.speakerCountMax}
          cloudUpload={transcriptionOptions.options.cloudUpload}
          maxDurationMinutes={transcriptionOptions.options.maxReliableAudioSeconds
            ? transcriptionOptions.options.maxReliableAudioSeconds / 60
            : null}
          hotwordLists={props.hotwordLists}
          hotwordsSupported={transcriptionOptions.options.hotwords.supported}
          hotwordMode={transcriptionOptions.options.hotwords.mode}
          hotwordMaxEntries={transcriptionOptions.options.hotwords.maxEntries}
          hotwordWeightsSupported={transcriptionOptions.options.hotwords.weightsSupported}
          hotwordDefaultWeight={transcriptionOptions.options.hotwords.defaultWeight}
          onOpenHotwordLibrary={props.onOpenHotwordLibrary ?? (() => undefined)}
          onCancel={() => setTranscriptionOptions(null)}
          onConfirm={(speakerCount, hotwordListId) => {
            const recordingId = transcriptionOptions.recordingId;
            setTranscriptionOptions(null);
            if (props.hotwordLists === undefined) {
              (props.onStartTranscription as (id: string, count: number | null) => void)(
                recordingId,
                speakerCount,
              );
            } else {
              props.onStartTranscription(recordingId, speakerCount, hotwordListId);
            }
          }}
        />
      )}
    </section>
  );
}
