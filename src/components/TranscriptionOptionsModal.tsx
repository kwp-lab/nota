import { AlertTriangle, Users } from "lucide-react";
import { useState } from "react";

interface TranscriptionOptionsModalProps {
  recordingTitle: string;
  retranscription: boolean;
  onCancel: () => void;
  onConfirm: (speakerCount: number | null) => void;
}

export function TranscriptionOptionsModal(props: TranscriptionOptionsModalProps) {
  const [mode, setMode] = useState<"auto" | "specified">("auto");
  const [countText, setCountText] = useState("2");
  const count = Number(countText);
  const validCount = Number.isInteger(count) && count >= 1 && count <= 64;

  return (
    <div className="modal-backdrop" role="presentation">
      <section
        className="transcription-options-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="transcription-options-title"
      >
        <header>
          <div>
            <p className="eyebrow">SPEAKER OPTIONS</p>
            <h2 id="transcription-options-title">
              {props.retranscription ? "重新转写" : "开始转写"}
            </h2>
            <p title={props.recordingTitle}>{props.recordingTitle}</p>
          </div>
          <Users size={26} />
        </header>

        <div className="transcription-options-body">
          <fieldset>
            <legend>会议中有多少位说话人？</legend>
            <label className={mode === "auto" ? "selected" : ""}>
              <input
                type="radio"
                name="speaker-count-mode"
                checked={mode === "auto"}
                onChange={() => setMode("auto")}
              />
              <span>
                <strong>自动判断</strong>
                <small>由服务端根据整场会议的声纹自动聚类</small>
              </span>
            </label>
            <label className={mode === "specified" ? "selected" : ""}>
              <input
                type="radio"
                name="speaker-count-mode"
                checked={mode === "specified"}
                onChange={() => setMode("specified")}
              />
              <span>
                <strong>指定人数</strong>
                <small>用于已知参会人数的会议，范围为 1–64 人</small>
              </span>
              <input
                aria-label="说话人数"
                type="number"
                min={1}
                max={64}
                step={1}
                value={countText}
                disabled={mode !== "specified"}
                aria-invalid={mode === "specified" && !validCount}
                onChange={(event) => setCountText(event.target.value)}
              />
            </label>
          </fieldset>
          {mode === "specified" && !validCount && (
            <p className="field-error">请输入 1–64 之间的整数。</p>
          )}
          <div className="transcription-options-warning">
            <AlertTriangle size={17} />
            <span>指定错误的人数可能导致不同说话人被合并，或同一说话人被拆分。</span>
          </div>
        </div>

        <footer>
          <button className="button secondary" onClick={props.onCancel}>取消</button>
          <button
            className="button primary"
            disabled={mode === "specified" && !validCount}
            onClick={() => props.onConfirm(mode === "auto" ? null : count)}
          >
            {props.retranscription ? "重新转写" : "开始转写"}
          </button>
        </footer>
      </section>
    </div>
  );
}
