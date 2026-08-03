import { AlertCircle, LoaderCircle, Play, UserRound, Waves } from "lucide-react";
import { useMemo, useState } from "react";
import type {
  ParticipantProfile,
  SpeakerIdentificationAssignment,
  SpeakerIdentificationSession,
} from "../types";

interface SpeakerIdentificationModalProps {
  session: SpeakerIdentificationSession;
  participants: ParticipantProfile[];
  saving: boolean;
  onPreview: (startMs: number, endMs: number) => void;
  onCancel: () => void;
  onSave: (assignments: SpeakerIdentificationAssignment[]) => void;
}

interface Selection {
  participantId: string;
  newDisplayName: string;
}

const formatDuration = (milliseconds: number) => {
  const seconds = Math.round(milliseconds / 1000);
  const minutes = Math.floor(seconds / 60);
  return `${minutes}:${(seconds % 60).toString().padStart(2, "0")}`;
};

export function SpeakerIdentificationModal(props: SpeakerIdentificationModalProps) {
  const initial = useMemo(
    () => Object.fromEntries(props.session.candidates.map((candidate) => [
      candidate.rawSpeaker,
      {
        participantId: candidate.suggestedParticipantId ?? "",
        newDisplayName: "",
      },
    ])) as Record<string, Selection>,
    [props.session],
  );
  const [selections, setSelections] = useState(initial);

  const update = (speaker: string, patch: Partial<Selection>) => {
    setSelections((current) => ({
      ...current,
      [speaker]: { ...current[speaker], ...patch },
    }));
  };

  const save = () => {
    props.onSave(props.session.candidates.map((candidate) => {
      const selection = selections[candidate.rawSpeaker];
      return {
        rawSpeaker: candidate.rawSpeaker,
        participantId:
          selection.participantId && selection.participantId !== "__new__"
            ? selection.participantId
            : null,
        newDisplayName:
          selection.participantId === "__new__"
            ? selection.newDisplayName.trim() || null
            : null,
      };
    }));
  };
  const hasInvalidNewParticipant = Object.values(selections).some(
    (selection) => selection.participantId === "__new__"
      && !selection.newDisplayName.trim(),
  );

  return (
    <div className="modal-backdrop" role="presentation">
      <section
        className="speaker-identification-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="speaker-identification-title"
      >
        <header>
          <div>
            <p className="eyebrow">VOICEPRINT CONFIRMATION</p>
            <h2 id="speaker-identification-title">确认说话人</h2>
            <p>
              检测到 {props.session.speakerCount} 个 speaker，成功提取 {props.session.voiceprintCount} 个声纹。
              建议结果仅供参考，保存前请试听确认。
            </p>
          </div>
          <Waves size={26} />
        </header>

        <div className="speaker-candidate-list">
          {props.session.candidates.map((candidate) => {
            const selection = selections[candidate.rawSpeaker];
            const isNew = selection.participantId === "__new__";
            return (
              <article className="speaker-candidate" key={candidate.rawSpeaker}>
                <div className="speaker-candidate-heading">
                  <span className="participant-avatar"><UserRound size={18} /></span>
                  <div>
                    <strong>{candidate.rawSpeaker}</strong>
                    <small>累计发言 {formatDuration(candidate.totalSpeechMs)}</small>
                  </div>
                  <button
                    className="button secondary"
                    onClick={() => props.onPreview(
                      candidate.previewStartMs,
                      candidate.previewEndMs,
                    )}
                  >
                    <Play size={14} fill="currentColor" />试听
                  </button>
                </div>

                {candidate.suggestedParticipantName && (
                  <div className="match-suggestion">
                    建议：{candidate.suggestedParticipantName}
                    {candidate.matchScore !== null && (
                      <span>相似度 {(candidate.matchScore * 100).toFixed(1)}%</span>
                    )}
                  </div>
                )}
                {candidate.errorMessage && (
                  <div className="candidate-error">
                    <AlertCircle size={14} />{candidate.errorMessage}
                  </div>
                )}

                <label>
                  <span>真实姓名</span>
                  <select
                    value={selection.participantId}
                    onChange={(event) => update(candidate.rawSpeaker, {
                      participantId: event.target.value,
                    })}
                  >
                    <option value="">暂不标记</option>
                    {props.participants.map((participant) => (
                      <option value={participant.id} key={participant.id}>
                        {participant.displayName}
                      </option>
                    ))}
                    <option value="__new__">+ 新建参会人</option>
                  </select>
                </label>
                {isNew && (
                  <input
                    aria-label={`${candidate.rawSpeaker} 新参会人姓名`}
                    value={selection.newDisplayName}
                    maxLength={80}
                    placeholder="例如：小明"
                    autoFocus
                    onChange={(event) => update(candidate.rawSpeaker, {
                      newDisplayName: event.target.value,
                    })}
                  />
                )}
              </article>
            );
          })}
        </div>

        <footer>
          <button className="button secondary" disabled={props.saving} onClick={props.onCancel}>
            取消
          </button>
          <button
            className="button primary"
            disabled={props.saving || hasInvalidNewParticipant}
            onClick={save}
          >
            {props.saving && <LoaderCircle className="spin" size={15} />}
            保存声纹并更新说话人
          </button>
        </footer>
      </section>
    </div>
  );
}
