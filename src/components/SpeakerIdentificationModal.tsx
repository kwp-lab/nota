import {
  AlertCircle,
  CheckCircle2,
  Info,
  LoaderCircle,
  Pause,
  Play,
  RotateCcw,
  UserRound,
  Waves,
  X,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import type {
  ParticipantProfile,
  SpeakerIdentificationAssignment,
  SpeakerIdentificationSession,
} from "../types";
import { AppTooltip } from "./AppTooltip";

export interface SpeakerUtterance {
  startMs: number;
  endMs: number;
  text: string;
}

export interface SpeakerManagementSpeaker {
  rawSpeaker: string;
  currentParticipantId: string | null;
  currentDisplayName: string | null;
  totalSpeechMs: number;
  utterances: SpeakerUtterance[];
}

export type SpeakerAnalysisStatus = "idle" | "loading" | "ready" | "failed";
export type VoiceprintAvailability =
  | { kind: "available" }
  | { kind: "providerNotConfigured" }
  | { kind: "transcriptionProviderUnsupported"; providerName: string };

export interface SpeakerPreviewRequest {
  id: string;
  startMs: number;
  endMs: number;
}

export interface SpeakerManagementSaveRequest {
  mappingAssignments: SpeakerIdentificationAssignment[];
  sessionAssignments: SpeakerIdentificationAssignment[];
  saveVoiceprints: boolean;
}

interface SpeakerIdentificationModalProps {
  speakers: SpeakerManagementSpeaker[];
  session: SpeakerIdentificationSession | null;
  analysisStatus: SpeakerAnalysisStatus;
  analysisError: string | null;
  participants: ParticipantProfile[];
  initialSpeaker: string | null;
  saving: boolean;
  voiceprintAvailability: VoiceprintAvailability;
  activePreviewId: string | null;
  previewPlaying: boolean;
  onAnalyze: () => void;
  onConfigureVoiceprints: () => void;
  onPreview: (preview: SpeakerPreviewRequest) => void;
  onStopPreview: () => void;
  onCancel: () => void;
  onSave: (request: SpeakerManagementSaveRequest) => void;
}

interface Selection {
  participantId: string;
  newDisplayName: string;
  dirty: boolean;
  saveVoiceprint: boolean;
}

const formatDuration = (milliseconds: number) => {
  const seconds = Math.round(milliseconds / 1000);
  const minutes = Math.floor(seconds / 60);
  return `${minutes}:${(seconds % 60).toString().padStart(2, "0")}`;
};

const formatTimestamp = (milliseconds: number) => {
  const seconds = Math.floor(Math.max(milliseconds, 0) / 1000);
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const rest = seconds % 60;
  return [hours, minutes, rest]
    .map((value) => value.toString().padStart(2, "0"))
    .join(":");
};

const initialSelections = (speakers: SpeakerManagementSpeaker[]) =>
  Object.fromEntries(speakers.map((speaker) => [
    speaker.rawSpeaker,
    {
      participantId: speaker.currentParticipantId ?? "",
      newDisplayName: speaker.currentParticipantId ? "" : speaker.currentDisplayName ?? "",
      dirty: false,
      saveVoiceprint: false,
    },
  ])) as Record<string, Selection>;

const previewId = (
  kind: "clean" | "utterance",
  rawSpeaker: string,
  startMs: number,
  endMs: number,
) => `${kind}:${rawSpeaker}:${startMs}-${endMs}`;

const assignmentFor = (speaker: SpeakerManagementSpeaker, selection: Selection) => ({
  rawSpeaker: speaker.rawSpeaker,
  participantId:
    selection.participantId
      ? selection.participantId
      : null,
  newDisplayName:
    !selection.participantId
      ? selection.newDisplayName.trim() || null
      : null,
});

export function SpeakerIdentificationModal(props: SpeakerIdentificationModalProps) {
  const orderedSpeakers = useMemo(
    () => [...props.speakers].sort((left, right) => {
      const leftConfirmed = left.currentDisplayName ? 1 : 0;
      const rightConfirmed = right.currentDisplayName ? 1 : 0;
      return leftConfirmed - rightConfirmed
        || left.rawSpeaker.localeCompare(right.rawSpeaker, undefined, { numeric: true });
    }),
    [props.speakers],
  );
  const defaultSpeaker = props.initialSpeaker
    && props.speakers.some((speaker) => speaker.rawSpeaker === props.initialSpeaker)
    ? props.initialSpeaker
    : orderedSpeakers[0]?.rawSpeaker ?? "";
  const [activeSpeaker, setActiveSpeaker] = useState(defaultSpeaker);
  const [selections, setSelections] = useState(() => initialSelections(props.speakers));

  const candidates = useMemo(
    () => new Map(props.session?.candidates.map((candidate) => [candidate.rawSpeaker, candidate])),
    [props.session],
  );

  useEffect(() => {
    if (props.initialSpeaker
      && props.speakers.some((speaker) => speaker.rawSpeaker === props.initialSpeaker)) {
      setActiveSpeaker(props.initialSpeaker);
    }
  }, [props.initialSpeaker, props.speakers]);

  useEffect(() => {
    setSelections((current) => {
      const next = { ...current };
      for (const speaker of props.speakers) {
        const existing = current[speaker.rawSpeaker] ?? {
          participantId: speaker.currentParticipantId ?? "",
          newDisplayName: speaker.currentParticipantId ? "" : speaker.currentDisplayName ?? "",
          dirty: false,
          saveVoiceprint: false,
        };
        if (existing.dirty || speaker.currentDisplayName) {
          next[speaker.rawSpeaker] = existing;
          continue;
        }
        const suggestion = candidates.get(speaker.rawSpeaker)?.suggestedParticipantId;
        next[speaker.rawSpeaker] = suggestion
          ? { ...existing, participantId: suggestion }
          : existing;
      }
      return next;
    });
  }, [candidates, props.speakers]);

  useEffect(() => {
    setSelections((current) => Object.fromEntries(Object.entries(current).map(
      ([speaker, selection]) => [speaker, { ...selection, saveVoiceprint: false }],
    )));
  }, [props.session?.id]);

  const update = (speaker: string, patch: Partial<Selection>) => {
    setSelections((current) => ({
      ...current,
      [speaker]: {
        ...current[speaker],
        ...patch,
        saveVoiceprint: patch.saveVoiceprint ?? false,
        dirty: patch.saveVoiceprint !== undefined ? current[speaker].dirty : true,
      },
    }));
  };

  const mappingAssignments = useMemo(
    () => props.speakers.flatMap((speaker) => {
      const selection = selections[speaker.rawSpeaker];
      if (!selection) return [];
      const candidate = candidates.get(speaker.rawSpeaker);
      const suggestedSelection = !speaker.currentDisplayName
        && Boolean(candidate?.suggestedParticipantId)
        && candidate?.suggestedParticipantId === selection.participantId;
      return selection.dirty || suggestedSelection
        ? [assignmentFor(speaker, selection)]
        : [];
    }),
    [candidates, props.speakers, selections],
  );

  const voiceprintAssignments = useMemo(
    () => props.speakers.flatMap((speaker) => {
      const selection = selections[speaker.rawSpeaker];
      const candidate = candidates.get(speaker.rawSpeaker);
      if (!selection?.saveVoiceprint || !props.session || !candidate?.embeddingExtracted
        || candidate.sampleStatus !== "enrollable" || props.analysisStatus !== "ready"
        || props.voiceprintAvailability.kind !== "available") return [];
      const hasIdentity = Boolean(selection.participantId || selection.newDisplayName.trim());
      return hasIdentity ? [{ ...assignmentFor(speaker, selection), saveVoiceprint: true }] : [];
    }),
    [candidates, props.session, props.speakers, selections, props.analysisStatus, props.voiceprintAvailability],
  );

  const sessionAssignments = useMemo(
    () => {
      const combined = new Map<string, SpeakerIdentificationAssignment>();
      for (const assignment of mappingAssignments) {
        combined.set(assignment.rawSpeaker, assignment);
      }
      for (const assignment of voiceprintAssignments) {
        combined.set(assignment.rawSpeaker, assignment);
      }
      return [...combined.values()];
    },
    [mappingAssignments, voiceprintAssignments],
  );

  const confirmedCount = props.speakers.filter((speaker) => speaker.currentDisplayName).length;
  const selectedSpeaker = props.speakers.find((speaker) => speaker.rawSpeaker === activeSpeaker)
    ?? orderedSpeakers[0];
  const selection = selectedSpeaker ? selections[selectedSpeaker.rawSpeaker] : null;
  const candidate = selectedSpeaker ? candidates.get(selectedSpeaker.rawSpeaker) : undefined;
  const canAnalyzeVoiceprints = props.voiceprintAvailability.kind === "available";
  const canSaveVoiceprints = Boolean(props.session && candidate?.embeddingExtracted
    && candidate.sampleStatus === "enrollable" && props.analysisStatus === "ready"
    && (selection?.participantId || selection?.newDisplayName.trim()) && canAnalyzeVoiceprints);
  const saveVoiceprints = voiceprintAssignments.length > 0;
  const selectedName = selection?.participantId
    ? props.participants.find((person) => person.id === selection.participantId)?.displayName
      ?? selectedSpeaker?.currentDisplayName ?? ""
    : selection?.newDisplayName ?? "";
  const hasSaveableWork = mappingAssignments.length > 0
    || (saveVoiceprints && voiceprintAssignments.length > 0);

  const speakerStatus = (speaker: SpeakerManagementSpeaker) => {
    const item = selections[speaker.rawSpeaker];
    const suggestion = candidates.get(speaker.rawSpeaker)?.suggestedParticipantId;
    if (item?.dirty) {
      return item.participantId || item.newDisplayName.trim()
        ? { label: "已修改", tone: "changed" }
        : { label: "将清除", tone: "cleared" };
    }
    if (speaker.currentDisplayName) return { label: "已确认", tone: "confirmed" };
    if (suggestion && item?.participantId === suggestion) {
      return { label: "建议匹配", tone: "suggested" };
    }
    return { label: "待确认", tone: "pending" };
  };

  return (
    <div
      className="modal-backdrop"
      role="presentation"
      onClick={(event) => {
        if (event.target === event.currentTarget && !props.saving) {
          props.onCancel();
        }
      }}
    >
      <section
        className="speaker-identification-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="speaker-identification-title"
      >
        <header>
          <div>
            <h2 id="speaker-identification-title">管理说话人</h2>
            <p>
              共 {props.speakers.length} 位说话人，已确认 {confirmedCount} 位，
              待确认 {Math.max(0, props.speakers.length - confirmedCount)} 位。
            </p>
          </div>
          <AppTooltip content={props.saving ? "正在保存，暂时无法关闭" : "关闭"} wrapDisabled={props.saving}>
            <button
              className="icon-button speaker-modal-close"
              type="button"
              aria-label="关闭管理说话人"
              disabled={props.saving}
              onClick={props.onCancel}
            >
              <X size={19} />
            </button>
          </AppTooltip>
        </header>

        <div className={`speaker-analysis-banner status-${props.analysisStatus}`}>
          {props.analysisStatus === "loading" ? (
            <><LoaderCircle className="spin" size={16} />正在分析声纹；你可以先试听发言并填写姓名。</>
          ) : props.analysisStatus === "ready" ? (
            <>
              <CheckCircle2 size={16} />
              <span>声纹分析完成，{props.session?.voiceprintCount ?? 0} 位具备可用声纹。</span>
              <button
                className="button secondary compact speaker-analysis-action"
                disabled={!canAnalyzeVoiceprints || props.saving}
                onClick={props.onAnalyze}
              >
                <RotateCcw size={13} />重新分析
              </button>
            </>
          ) : props.analysisStatus === "failed" ? (
            <>
              <AlertCircle size={16} />
              <span>声纹分析失败：{props.analysisError}</span>
              <button
                className="button secondary speaker-analysis-action"
                disabled={!canAnalyzeVoiceprints || props.saving}
                onClick={props.onAnalyze}
              >
                <RotateCcw size={13} />重试
              </button>
            </>
          ) : props.voiceprintAvailability.kind === "transcriptionProviderUnsupported" ? (
            <>
              <Info size={16} />
              <span>
                本次转写由{props.voiceprintAvailability.providerName}生成，支持区分匿名说话人，但不支持 Nota 声纹分析。你仍可试听发言并手动设置姓名。
              </span>
              <AppTooltip
                content={`本次转写由${props.voiceprintAvailability.providerName}生成，不支持 Nota 声纹分析`}
                wrapDisabled
              >
                <button
                  className="button secondary speaker-analysis-action"
                  disabled
                >
                  <Waves size={13} />声纹分析不可用
                </button>
              </AppTooltip>
            </>
          ) : props.voiceprintAvailability.kind === "providerNotConfigured" ? (
            <>
              <Info size={16} />
              <span>可直接手动设置姓名；如需声纹分析，请先选择声纹提取服务。</span>
              <button
                className="button secondary speaker-analysis-action"
                disabled={props.saving}
                onClick={props.onConfigureVoiceprints}
              >
                前往声纹管理
              </button>
            </>
          ) : (
            <>
              <Info size={16} />
              <span>声纹分析是可选功能；仅在需要跨会议识别时手动启动。</span>
              <button
                className="button secondary speaker-analysis-action"
                disabled={props.saving}
                onClick={props.onAnalyze}
              >
                <Waves size={13} />开始声纹分析
              </button>
            </>
          )}
        </div>

        <div className="speaker-manager-body">
          <nav className="speaker-manager-list" aria-label="会议说话人">
            {orderedSpeakers.map((speaker) => {
              const status = speakerStatus(speaker);
              const item = selections[speaker.rawSpeaker];
              const selectedParticipant = props.participants.find(
                (participant) => participant.id === item?.participantId,
              );
              const displayName = !item?.participantId
                ? item.newDisplayName.trim()
                : selectedParticipant?.displayName ?? speaker.currentDisplayName;
              return (
                <button
                  className={speaker.rawSpeaker === selectedSpeaker?.rawSpeaker ? "active" : ""}
                  key={speaker.rawSpeaker}
                  onClick={() => {
                    if (speaker.rawSpeaker !== selectedSpeaker?.rawSpeaker) {
                      props.onStopPreview();
                    }
                    setActiveSpeaker(speaker.rawSpeaker);
                  }}
                >
                  <span className="participant-avatar"><UserRound size={17} /></span>
                  <span>
                    <strong>{displayName || speaker.rawSpeaker}</strong>
                    <small>{displayName ? speaker.rawSpeaker : formatDuration(speaker.totalSpeechMs)}</small>
                  </span>
                  <i className={`speaker-state state-${status.tone}`}>{status.label}</i>
                </button>
              );
            })}
          </nav>

          {selectedSpeaker && selection && (
            <div className="speaker-manager-detail">
              <div className="speaker-detail-heading">
                <span className="participant-avatar"><UserRound size={19} /></span>
                <div>
                  <strong>{selectedSpeaker.rawSpeaker}</strong>
                  <small>累计发言约 {formatDuration(selectedSpeaker.totalSpeechMs)}</small>
                </div>
                {candidate && candidate.previewEndMs > candidate.previewStartMs && (() => {
                  const id = previewId(
                    "clean",
                    selectedSpeaker.rawSpeaker,
                    candidate.previewStartMs,
                    candidate.previewEndMs,
                  );
                  const isPlaying = props.activePreviewId === id && props.previewPlaying;
                  return (
                    <button
                      className={`button secondary speaker-preview-button${isPlaying ? " is-playing" : ""}`}
                      aria-label={isPlaying ? "暂停纯净试听" : "纯净试听"}
                      aria-pressed={isPlaying}
                      onClick={() => props.onPreview({
                        id,
                        startMs: candidate.previewStartMs,
                        endMs: candidate.previewEndMs,
                      })}
                    >
                      {isPlaying
                        ? <Pause size={14} fill="currentColor" />
                        : <Play size={14} fill="currentColor" />}
                      {isPlaying ? "试听中…" : "纯净试听"}
                    </button>
                  );
                })()}
              </div>

              {candidate?.suggestedParticipantName && !selectedSpeaker.currentDisplayName && (
                <div className="match-suggestion">
                  建议：{candidate.suggestedParticipantName}
                  {candidate.matchScore !== null && (
                    <span>相似度 {(candidate.matchScore * 100).toFixed(1)}%</span>
                  )}
                </div>
              )}
              {candidate?.statusMessage && (
                <div className={`candidate-status ${candidate.sampleStatus}`}>
                  <Info size={14} />{candidate.statusMessage}
                </div>
              )}
              {candidate?.errorMessage && (
                <div className="candidate-error">
                  <AlertCircle size={14} />{candidate.errorMessage}
                </div>
              )}

              <label className="speaker-name-field">
                <span>本场显示名称</span>
                <input
                  aria-label={`${selectedSpeaker.rawSpeaker} 本场显示名称`}
                  aria-describedby="speaker-name-scope"
                  value={selectedName}
                  maxLength={80}
                  placeholder="例如：张经理、客户采购负责人"
                  disabled={props.saving}
                  onChange={(event) => update(selectedSpeaker.rawSpeaker, {
                    participantId: "",
                    newDisplayName: event.target.value,
                  })}
                />
              </label>
              <p id="speaker-name-scope" className="speaker-name-scope">
                {selection.participantId
                  ? "已关联已有人员。修改上方名称将改为仅本场使用，不会重命名全局人员。"
                  : "仅用于当前录音的此转写版本，重启后保留，不加入声纹库。清空可恢复匿名标签。"}
              </p>
              <details className="speaker-library-picker" open={Boolean(selection.participantId) || undefined}>
                <summary>从声纹库选择已有人员</summary>
                <label className="speaker-name-field">
                <span>关联已有人员</span>
                <select
                  aria-label={`${selectedSpeaker.rawSpeaker} 关联已有人员`}
                  value={selection.participantId}
                  disabled={props.saving}
                  onChange={(event) => update(selectedSpeaker.rawSpeaker, {
                    participantId: event.target.value,
                    newDisplayName: event.target.value ? "" : selectedName,
                  })}
                >
                  <option value="">仅本场录音（不关联人员）</option>
                  {props.participants.map((participant) => (
                    <option value={participant.id} key={participant.id}>
                      {participant.displayName}
                    </option>
                  ))}
                </select>
                </label>
              </details>

              {canAnalyzeVoiceprints && (
                <div className="speaker-save-options">
                  <label className={!canSaveVoiceprints ? "disabled" : ""}>
                    <input type="checkbox"
                      aria-label={`${selectedSpeaker.rawSpeaker} 保存声纹供以后会议识别`}
                      checked={Boolean(selection.saveVoiceprint && canSaveVoiceprints)}
                      disabled={!canSaveVoiceprints || props.saving}
                      onChange={(event) => update(selectedSpeaker.rawSpeaker, {
                        saveVoiceprint: event.target.checked,
                      })}
                    />
                    <span>保存此人的声纹，供以后会议识别
                      <small>{canSaveVoiceprints
                        ? "默认不保存；勾选后将保存此人的档案和合格声纹。"
                        : "填写名称并完成声纹分析，获得合格声纹后可选。"}</small>
                    </span>
                  </label>
                </div>
              )}

              <section className="speaker-utterances" aria-label={`${selectedSpeaker.rawSpeaker} 代表发言`}>
                <div>
                  <strong>代表发言</strong>
                  <small>声纹不够纯净时，可以多试听几条再判断。</small>
                </div>
                {selectedSpeaker.utterances.map((utterance) => {
                  const id = previewId(
                    "utterance",
                    selectedSpeaker.rawSpeaker,
                    utterance.startMs,
                    utterance.endMs,
                  );
                  const isPlaying = props.activePreviewId === id && props.previewPlaying;
                  const timestamp = formatTimestamp(utterance.startMs);
                  return (
                    <article
                      className={isPlaying ? "is-playing" : ""}
                      key={`${utterance.startMs}-${utterance.endMs}`}
                    >
                      <button
                        className={isPlaying ? "is-playing" : ""}
                        aria-label={isPlaying
                          ? `暂停 ${timestamp} 的发言`
                          : `试听 ${timestamp} 的发言`}
                        aria-pressed={isPlaying}
                        onClick={() => props.onPreview({
                          id,
                          startMs: utterance.startMs,
                          endMs: utterance.endMs,
                        })}
                      >
                        {isPlaying
                          ? <Pause size={12} fill="currentColor" />
                          : <Play size={12} fill="currentColor" />}
                        {timestamp}
                      </button>
                      <p>{utterance.text}</p>
                    </article>
                  );
                })}
              </section>
            </div>
          )}
        </div>

        <footer>
          <div className="speaker-save-options">
            {saveVoiceprints
              ? `已选择保存 ${voiceprintAssignments.length} 位说话人的声纹。`
              : "保存本场姓名不会新增全局人员或声纹。"}
          </div>
          <div>
            <button className="button secondary" disabled={props.saving} onClick={props.onCancel}>
              稍后继续
            </button>
            <button
              className="button primary"
              disabled={props.saving || !hasSaveableWork}
              onClick={() => props.onSave({
                mappingAssignments,
                sessionAssignments,
                saveVoiceprints,
              })}
            >
              {props.saving && <LoaderCircle className="spin" size={15} />}
              {saveVoiceprints
                ? mappingAssignments.length > 0
                  ? `保存姓名与 ${voiceprintAssignments.length} 份声纹`
                  : `保存 ${voiceprintAssignments.length} 份声纹`
                : "保存姓名更改"}
            </button>
          </div>
        </footer>
      </section>
    </div>
  );
}
