import { AlertTriangle, Settings2 } from "lucide-react";
import { useState } from "react";
import { AppTooltip } from "./AppTooltip";

interface TranscriptionOptionsModalProps {
  recordingTitle: string;
  retranscription: boolean;
  providerName: string;
  speakerCountMin: number | null;
  speakerCountMax: number | null;
  cloudUpload: boolean;
  maxDurationMinutes: number | null;
  onCancel: () => void;
  hotwordLists?: {
    id: string;
    name: string;
    entryCount: number;
    weightedEntryCount: number;
    superHotwordCount: number;
  }[];
  hotwordsSupported?: boolean;
  hotwordMode?: string;
  hotwordMaxEntries?: number;
  hotwordWeightsSupported?: boolean;
  hotwordDefaultWeight?: number | null;
  onOpenHotwordLibrary?: () => void;
  onConfirm: (speakerCount: number | null, hotwordListId: string | null) => void;
}

export function TranscriptionOptionsModal(props: TranscriptionOptionsModalProps) {
  const [mode, setMode] = useState<"auto" | "specified">("auto");
  const [countText, setCountText] = useState(String(Math.max(2, props.speakerCountMin ?? 2)));
  const [hotwordListId, setHotwordListId] = useState("");
  const count = Number(countText);
  const validCount = Number.isInteger(count)
    && count >= (props.speakerCountMin ?? 1)
    && count <= (props.speakerCountMax ?? 100);
  const hasSpeakerOptions = props.speakerCountMin !== null && props.speakerCountMax !== null;
  const hotwordLists = props.hotwordLists ?? [];
  const hotwordsSupported = props.hotwordsSupported ?? false;
  const selectedHotwordList = hotwordLists.find((list) => list.id === hotwordListId);
  const selectedExceedsLimit = !!selectedHotwordList
    && !!props.hotwordMaxEntries
    && selectedHotwordList.entryCount > props.hotwordMaxEntries;
  const hotwordInvalid = !!hotwordListId
    && (!hotwordsSupported || !selectedHotwordList?.entryCount || selectedExceedsLimit);

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
            <p className="eyebrow">TRANSCRIPTION OPTIONS</p>
            <h2 id="transcription-options-title">
              {props.retranscription ? "重新转写" : "开始转写"}
            </h2>
            <AppTooltip content={props.recordingTitle} side="bottom" align="start">
              <p>{props.recordingTitle}</p>
            </AppTooltip>
          </div>
          <Settings2 size={26} />
        </header>

        <div className="transcription-options-body">
          <div className="transcription-provider-row">
            <span>Provider</span>
            <strong>{props.providerName}</strong>
          </div>
          {hasSpeakerOptions && <fieldset>
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
                min={props.speakerCountMin ?? undefined}
                max={props.speakerCountMax ?? undefined}
                step={1}
                value={countText}
                disabled={mode !== "specified"}
                aria-invalid={mode === "specified" && !validCount}
                onChange={(event) => setCountText(event.target.value)}
              />
            </label>
          </fieldset>}
          {hasSpeakerOptions && mode === "specified" && !validCount && (
            <p className="field-error">
              请输入 {props.speakerCountMin}–{props.speakerCountMax} 之间的整数。
            </p>
          )}
          <label className="transcription-hotword-select">
            <span>热词列表</span>
            <select value={hotwordListId} onChange={(event) => setHotwordListId(event.target.value)}>
              <option value="">不使用热词</option>
              {hotwordLists.map((list) => (
                <option
                  key={list.id}
                  value={list.id}
                  disabled={list.entryCount === 0 || !hotwordsSupported
                    || (!!props.hotwordMaxEntries && list.entryCount > props.hotwordMaxEntries)}
                >
                  {list.name}（{list.entryCount} 个词）
                </option>
              ))}
            </select>
            {!hotwordsSupported && (
              <small>
                {props.hotwordMode === "serverUpgradeRequired"
                  ? "当前 Nota ASR Server 版本过旧，请升级后使用热词。"
                  : "当前 Provider 或模型不支持热词。"}
                <button type="button" className="text-button" onClick={props.onOpenHotwordLibrary}>前往热词库</button>
              </small>
            )}
            {selectedExceedsLimit && (
              <small>当前模型最多支持 {props.hotwordMaxEntries} 条热词，请精简列表后重试。</small>
            )}
            {selectedHotwordList && hotwordsSupported && props.hotwordWeightsSupported && (
              <small>
                共 {selectedHotwordList.entryCount} 个热词
                {selectedHotwordList.superHotwordCount > 0
                  ? `，其中 ${selectedHotwordList.superHotwordCount} 个超级热词`
                  : ""}
                {selectedHotwordList.entryCount > selectedHotwordList.weightedEntryCount
                  ? `；${selectedHotwordList.entryCount - selectedHotwordList.weightedEntryCount} 个未指定权重的热词使用默认权重 ${props.hotwordDefaultWeight ?? 4}`
                  : ""}。
              </small>
            )}
            {selectedHotwordList && hotwordsSupported && !props.hotwordWeightsSupported
              && selectedHotwordList.weightedEntryCount > 0 && (
              <small>
                当前 Provider 不支持自定义权重，将忽略权重并发送 {selectedHotwordList.entryCount} 个普通热词。
              </small>
            )}
          </label>
          <div className="transcription-options-warning">
            <AlertTriangle size={17} />
            <span>
              {props.cloudUpload
                ? selectedHotwordList
                  ? `${selectedHotwordList.entryCount} 个热词及完整录音将发送至${props.providerName}，临时文件约 48 小时后清理；始终开启匿名说话人分离${props.maxDurationMinutes ? `，最长 ${props.maxDurationMinutes} 分钟` : ""}。`
                  : `将把完整录音上传至${props.providerName}，临时文件约 48 小时后清理；始终开启匿名说话人分离${props.maxDurationMinutes ? `，最长 ${props.maxDurationMinutes} 分钟` : ""}。`
                : "准确性优先：人数仅作为安全聚类目标，相似度不足时不会为凑人数强行合并。"}
            </span>
          </div>
        </div>

        <footer>
          <button className="button secondary" onClick={props.onCancel}>取消</button>
          <button
            className="button primary"
            disabled={(hasSpeakerOptions && mode === "specified" && !validCount) || hotwordInvalid}
            onClick={() => {
              const speakerCount = !hasSpeakerOptions || mode === "auto" ? null : count;
              if (props.hotwordLists === undefined) {
                (props.onConfirm as (value: number | null) => void)(speakerCount);
              } else {
                props.onConfirm(speakerCount, hotwordListId || null);
              }
            }}
          >
            {props.retranscription ? "重新转写" : "开始转写"}
          </button>
        </footer>
      </section>
    </div>
  );
}
