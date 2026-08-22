import { AlertTriangle, Users } from "lucide-react";
import { useState } from "react";
import { AppTooltip } from "./AppTooltip";

interface TranscriptionOptionsModalProps {
  recordingTitle: string;
  retranscription: boolean;
  providerName: string;
  speakerCountMin: number;
  speakerCountMax: number;
  cloudUpload: boolean;
  maxDurationMinutes: number | null;
  onCancel: () => void;
  onConfirm: (speakerCount: number | null) => void;
}

export function TranscriptionOptionsModal(props: TranscriptionOptionsModalProps) {
  const [mode, setMode] = useState<"auto" | "specified">("auto");
  const [countText, setCountText] = useState(String(Math.max(2, props.speakerCountMin)));
  const count = Number(countText);
  const validCount = Number.isInteger(count)
    && count >= props.speakerCountMin
    && count <= props.speakerCountMax;

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
            <AppTooltip content={props.recordingTitle} side="bottom" align="start">
              <p>{props.recordingTitle}</p>
            </AppTooltip>
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
                <small>由服务端保守聚类，允许多拆但避免合并弱相似度声音</small>
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
                <strong>指定目标人数</strong>
                <small>
                  用于已知参会人数的会议，范围为 {props.speakerCountMin}–{props.speakerCountMax} 人；结果可能更多
                </small>
              </span>
              <input
                aria-label="说话人数"
                type="number"
                min={props.speakerCountMin}
                max={props.speakerCountMax}
                step={1}
                value={countText}
                disabled={mode !== "specified"}
                aria-invalid={mode === "specified" && !validCount}
                onChange={(event) => setCountText(event.target.value)}
              />
            </label>
          </fieldset>
          {mode === "specified" && !validCount && (
            <p className="field-error">
              请输入 {props.speakerCountMin}–{props.speakerCountMax} 之间的整数。
            </p>
          )}
          <div className="transcription-options-warning">
            <AlertTriangle size={17} />
            <span>
              {props.cloudUpload
                ? `将把完整录音上传至${props.providerName}，临时文件约 48 小时后清理；始终开启匿名说话人分离${props.maxDurationMinutes ? `，最长 ${props.maxDurationMinutes} 分钟` : ""}。`
                : "准确性优先：人数仅作为安全聚类目标，相似度不足时不会为凑人数强行合并。"}
            </span>
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
