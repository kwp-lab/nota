import { useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  CheckCircle2,
  LoaderCircle,
  RefreshCw,
  Trash2,
  Wifi,
} from "lucide-react";
import type {
  AsrModel,
  AsrProvider,
  AsrProviderKind,
  AsrProviderProbeRequest,
} from "../../../types";
import type { SettingsController } from "../hooks/useSettingsController";
import { InlineStatus, SelectControl, type ProviderFeedback } from "../SettingsPrimitives";
import type { SettingsModel } from "../types";
import { ProviderManagerShell, ResourceEmptyState } from "./ProviderManagerShell";

interface AsrProviderDraft {
  id: string | null;
  name: string;
  kind: AsrProviderKind;
  baseUrl: string;
  apiKey: string;
  modelId: string;
  hasApiKey: boolean;
}

const KEY_MASK = "••••••••";
const DASHSCOPE_BASE_URL = "https://dashscope.aliyuncs.com/api/v1";
const DASHSCOPE_MODEL = "qwen-audio-3.0-asr-flash-filetrans";

const emptyDraft = (): AsrProviderDraft => ({
  id: null,
  name: "本地 FunASR",
  kind: "funAsr",
  baseUrl: "http://127.0.0.1:8000/v1",
  apiKey: "",
  modelId: "sensevoice",
  hasApiKey: false,
});

const toDraft = (provider: AsrProvider): AsrProviderDraft => ({
  id: provider.id,
  name: provider.name,
  kind: provider.kind,
  baseUrl: provider.baseUrl,
  apiKey: provider.hasApiKey ? KEY_MASK : "",
  modelId: provider.modelId,
  hasApiKey: provider.hasApiKey,
});

const keyUpdate = (draft: AsrProviderDraft) =>
  draft.hasApiKey && draft.apiKey === KEY_MASK
    ? ({ kind: "keep" } as const)
    : draft.apiKey
      ? ({ kind: "replace", value: draft.apiKey } as const)
      : ({ kind: "clear" } as const);

const probeRequest = (draft: AsrProviderDraft): AsrProviderProbeRequest => ({
  id: draft.id,
  kind: draft.kind,
  baseUrl: draft.baseUrl,
  modelId: draft.modelId,
  apiKey: keyUpdate(draft),
});

const kindLabel = (kind: AsrProviderKind) => {
  if (kind === "funAsr") return "FunASR";
  if (kind === "dashScope") return "阿里云千问";
  return "Compatible";
};

const applyKind = (draft: AsrProviderDraft, kind: AsrProviderKind): AsrProviderDraft => {
  if (kind === "dashScope") {
    return {
      ...draft,
      kind,
      name: draft.id ? draft.name : "千问云转写",
      baseUrl: DASHSCOPE_BASE_URL,
      modelId: DASHSCOPE_MODEL,
    };
  }
  if (kind === "funAsr") {
    return {
      ...draft,
      kind,
      name: draft.id ? draft.name : "本地 FunASR",
      baseUrl: "http://127.0.0.1:8000/v1",
      modelId: "sensevoice",
    };
  }
  return {
    ...draft,
    kind,
    name: draft.id ? draft.name : "兼容转写服务",
    baseUrl: "http://127.0.0.1:8000/v1",
    modelId: "whisper-1",
  };
};

export function AsrProviderManager(props: {
  model: SettingsModel;
  controller: SettingsController;
  onBack: () => void;
  onDirtyChange: (dirty: boolean) => void;
}) {
  const initialProvider = props.model.asrProviders.find(
    (provider) => provider.id === props.model.settings.activeAsrProviderId,
  ) ?? props.model.asrProviders[0];
  const [draft, setDraft] = useState<AsrProviderDraft | null>(
    initialProvider ? toDraft(initialProvider) : null,
  );
  const [baseline, setBaseline] = useState<AsrProviderDraft | null>(
    initialProvider ? toDraft(initialProvider) : null,
  );
  const [models, setModels] = useState<AsrModel[]>([]);
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState<ProviderFeedback | null>(null);

  const dirty = useMemo(
    () => JSON.stringify(draft) !== JSON.stringify(baseline),
    [baseline, draft],
  );

  useEffect(() => props.onDirtyChange(dirty), [dirty, props.onDirtyChange]);
  useEffect(() => () => props.onDirtyChange(false), [props.onDirtyChange]);

  useEffect(() => {
    if (draft?.id && !props.model.asrProviders.some((provider) => provider.id === draft.id)) {
      setDraft(null);
      setBaseline(null);
      setModels([]);
    }
  }, [draft?.id, props.model.asrProviders]);

  const confirmDiscard = () => !dirty || confirm("服务配置尚未保存。放弃这些更改吗？");

  const selectDraft = (next: AsrProviderDraft) => {
    if (!confirmDiscard()) return;
    setDraft(next);
    setBaseline(next.id ? next : null);
    setModels([]);
    setFeedback(null);
  };

  const changeDraft = (next: AsrProviderDraft, clearModels = true) => {
    setDraft(next);
    if (clearModels) setModels([]);
    setFeedback(null);
  };

  const saveProvider = async () => {
    if (!draft) return;
    setBusy(true);
    setFeedback({ tone: "loading", message: "正在保存服务配置…" });
    try {
      const saved = await props.controller.saveAsrProvider({
        id: draft.id,
        name: draft.name,
        kind: draft.kind,
        baseUrl: draft.baseUrl,
        modelId: draft.modelId,
        apiKey: keyUpdate(draft),
      });
      const next = toDraft(saved);
      setDraft(next);
      setBaseline(next);
      setFeedback({ tone: "success", message: "服务配置已保存。" });
    } catch (error) {
      setFeedback({ tone: "error", message: String(error) });
    } finally {
      setBusy(false);
    }
  };

  const testConnection = async () => {
    if (!draft) return;
    setBusy(true);
    setFeedback({ tone: "loading", message: "正在连接服务并检查接口…" });
    try {
      const result = await props.controller.testAsrProvider(probeRequest(draft));
      setModels(result.models);
      if (!draft.modelId.trim() && result.models.length === 1) {
        setDraft({ ...draft, modelId: result.models[0].id });
      }
      setFeedback({
        tone: result.level,
        message: [
          result.message,
          result.device ? `设备：${result.device}` : null,
          result.models.length ? `发现 ${result.models.length} 个模型` : null,
        ].filter(Boolean).join(" · "),
      });
    } catch (error) {
      setFeedback({ tone: "error", message: `连接失败：${String(error)}` });
    } finally {
      setBusy(false);
    }
  };

  const loadModels = async () => {
    if (!draft) return;
    setBusy(true);
    setFeedback({ tone: "loading", message: "正在从服务读取模型列表…" });
    try {
      const next = await props.controller.listAsrModels(probeRequest(draft));
      setModels(next);
      if (!draft.modelId.trim() && next.length === 1) setDraft({ ...draft, modelId: next[0].id });
      setFeedback(next.length
        ? { tone: "success", message: `已读取 ${next.length} 个模型，请从列表选择。` }
        : { tone: "warning", message: "服务没有返回模型，仍可手工填写模型 ID。" });
    } catch (error) {
      setModels([]);
      setFeedback({
        tone: "error",
        message: `无法获取模型列表：${String(error)}。仍可手工填写模型 ID。`,
      });
    } finally {
      setBusy(false);
    }
  };

  const deleteProvider = async () => {
    if (!draft?.id || !confirm(`删除语音转写服务“${draft.name}”？`)) return;
    setBusy(true);
    try {
      await props.controller.deleteAsrProvider(draft.id);
      const nextProvider = props.model.asrProviders.find((provider) => provider.id !== draft.id);
      const next = nextProvider ? toDraft(nextProvider) : null;
      setDraft(next);
      setBaseline(next);
      setFeedback(null);
    } catch (error) {
      setFeedback({ tone: "error", message: String(error) });
    } finally {
      setBusy(false);
    }
  };

  const requestBack = () => {
    if (confirmDiscard()) props.onBack();
  };

  const persistedList = props.model.asrProviders.map((provider) => {
    const active = provider.id === props.model.settings.activeAsrProviderId;
    const selected = provider.id === draft?.id;
    return (
      <button
        type="button"
        key={provider.id}
        className={selected ? "selected" : ""}
        aria-label={`${provider.name} ${kindLabel(provider.kind)}`}
        onClick={() => selectDraft(toDraft(provider))}
      >
        <span>
          <strong>{provider.name}</strong>
          <small>{kindLabel(provider.kind)} · {provider.modelId}</small>
        </span>
        {active && <em>默认</em>}
      </button>
    );
  });
  const list = (
    <>
      {draft?.id === null && (
        <button
          type="button"
          className="resource-draft selected"
          aria-label={`正在添加 ${draft.name.trim() || "未命名转写服务"} ${kindLabel(draft.kind)}`}
        >
          <span>
            <strong>{draft.name.trim() || "未命名转写服务"}</strong>
            <small>新建 · {kindLabel(draft.kind)} · {draft.modelId || "待填写模型"}</small>
          </span>
          <em className="draft">填写中</em>
        </button>
      )}
      {persistedList}
    </>
  );

  return (
    <ProviderManagerShell
      title="语音转写 / 服务管理"
      description="添加、测试并选择语音转写服务。"
      listTitle="转写服务"
      count={props.model.asrProviders.length + (draft?.id === null ? 1 : 0)}
      onBack={requestBack}
      onAdd={() => selectDraft(emptyDraft())}
      list={list}
      detail={draft ? (
        <div className="provider-detail-form">
          <header>
            <div>
              <h2>{draft.id ? draft.name : "添加转写服务"}</h2>
              {draft.id === props.model.settings.activeAsrProviderId && <span>默认服务</span>}
            </div>
            {draft.id && (
              <button type="button" className="button danger-button" disabled={busy} onClick={() => void deleteProvider()}>
                <Trash2 size={14} /> 删除
              </button>
            )}
          </header>
          <fieldset disabled={busy}>
            <legend>连接配置</legend>
            <div className="provider-field-grid">
              <label>
                <span>名称</span>
                <input value={draft.name} onChange={(event) => changeDraft({ ...draft, name: event.target.value }, false)} />
              </label>
              <label>
                <span>服务类型</span>
                <SelectControl
                  value={draft.kind}
                  onChange={(event) => changeDraft(applyKind(draft, event.target.value as AsrProviderKind))}
                >
                  <option value="funAsr">FunASR</option>
                  <option value="openAiCompatible">OpenAI-compatible</option>
                  <option value="dashScope">阿里云千问（DashScope）</option>
                </SelectControl>
              </label>
              <label className="wide">
                <span>API Base URL</span>
                <input
                  value={draft.baseUrl}
                  readOnly={draft.kind === "dashScope"}
                  onChange={(event) => changeDraft({ ...draft, baseUrl: event.target.value })}
                />
              </label>
              <label className="wide">
                <span>API Key</span>
                <input
                  type="password"
                  autoComplete="new-password"
                  value={draft.apiKey}
                  placeholder="可留空"
                  onFocus={(event) => draft.apiKey === KEY_MASK && event.currentTarget.select()}
                  onChange={(event) => changeDraft({ ...draft, apiKey: event.target.value })}
                />
              </label>
              <label className="wide">
                <span>模型 ID</span>
                <input
                  value={draft.modelId}
                  readOnly={draft.kind === "dashScope"}
                  onChange={(event) => changeDraft({ ...draft, modelId: event.target.value }, false)}
                />
              </label>
              {models.length > 0 && draft.kind !== "dashScope" && (
                <label className="wide">
                  <span>服务返回的模型</span>
                  <SelectControl
                    aria-label="服务返回的模型"
                    value={models.some((model) => model.id === draft.modelId) ? draft.modelId : ""}
                    onChange={(event) => changeDraft({ ...draft, modelId: event.target.value }, false)}
                  >
                    <option value="">选择模型</option>
                    {models.map((model) => <option key={model.id} value={model.id}>{model.id}</option>)}
                  </SelectControl>
                </label>
              )}
            </div>
          </fieldset>
          <div className="provider-probe-actions">
            <button type="button" className="button secondary" disabled={busy} onClick={() => void testConnection()}>
              <Wifi size={14} /> {busy && feedback?.tone === "loading" ? "测试中…" : "测试连接"}
            </button>
            {draft.kind !== "dashScope" && (
              <button type="button" className="button secondary" disabled={busy || !draft.baseUrl.trim()} onClick={() => void loadModels()}>
                <RefreshCw size={14} /> 获取模型列表
              </button>
            )}
            <span>测试不会保存配置或上传录音。</span>
          </div>
          {feedback && (
            <InlineStatus tone={feedback.tone === "loading" ? "neutral" : feedback.tone}>
              {feedback.tone === "loading" ? <LoaderCircle size={15} className="spinning" />
                : feedback.tone === "success" ? <CheckCircle2 size={15} />
                  : <AlertTriangle size={15} />}
              <span>{feedback.message}</span>
            </InlineStatus>
          )}
          {draft.kind === "dashScope" && (
            <InlineStatus tone="warning">
              <AlertTriangle size={15} />
              <span>完整录音会上传至阿里云临时存储，约在 48 小时后清理；不支持 Nota 声纹分析。</span>
            </InlineStatus>
          )}
          <footer className="provider-detail-actions">
            <span>{dirty ? "有未保存的更改" : "配置已保存"}</span>
            <div>
              <button
                type="button"
                className="button secondary"
                disabled={!dirty || busy}
                onClick={() => {
                  setDraft(baseline);
                  setFeedback(null);
                }}
              >
                取消更改
              </button>
              <button type="button" className="button primary" disabled={!dirty || busy} onClick={() => void saveProvider()}>
                {busy && <LoaderCircle size={14} className="spinning" />} 保存服务
              </button>
            </div>
          </footer>
        </div>
      ) : (
        <ResourceEmptyState
          title="尚未配置转写服务"
          description="添加服务后，可以手动转写录音或开启自动转写。"
          actionLabel="添加服务"
          onAction={() => selectDraft(emptyDraft())}
        />
      )}
    />
  );
}
