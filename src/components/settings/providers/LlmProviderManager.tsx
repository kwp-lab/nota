import { useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  CheckCircle2,
  LoaderCircle,
  RefreshCw,
  Trash2,
  Wifi,
} from "lucide-react";
import { isOfficialOpenAiUrl, llmProviderReady, OPENAI_API_ROOT } from "../../../llm";
import type {
  LlmModel,
  LlmProvider,
  LlmProviderKind,
  LlmProviderProbeRequest,
} from "../../../types";
import type { SettingsController } from "../hooks/useSettingsController";
import { InlineStatus, SelectControl, type ProviderFeedback } from "../SettingsPrimitives";
import type { SettingsModel } from "../types";
import { ProviderManagerShell, ResourceEmptyState } from "./ProviderManagerShell";

interface LlmProviderDraft {
  id: string | null;
  name: string;
  kind: LlmProviderKind;
  baseUrl: string;
  apiKey: string;
  hasApiKey: boolean;
  modelId: string;
  inputTokenBudget: number;
  maxOutputTokens: number;
}

const KEY_MASK = "••••••••";

const emptyDraft = (): LlmProviderDraft => ({
  id: null,
  name: "OpenAI",
  kind: "openAi",
  baseUrl: OPENAI_API_ROOT,
  apiKey: "",
  hasApiKey: false,
  modelId: "gpt-5-mini",
  inputTokenBudget: 32_768,
  maxOutputTokens: 4_096,
});

const toDraft = (provider: LlmProvider): LlmProviderDraft => ({
  id: provider.id,
  name: provider.name,
  kind: provider.kind,
  baseUrl: provider.baseUrl,
  apiKey: provider.hasApiKey ? KEY_MASK : "",
  hasApiKey: provider.hasApiKey,
  modelId: provider.modelId,
  inputTokenBudget: provider.inputTokenBudget,
  maxOutputTokens: provider.maxOutputTokens,
});

const keyUpdate = (draft: LlmProviderDraft) =>
  draft.hasApiKey && draft.apiKey === KEY_MASK
    ? ({ kind: "keep" } as const)
    : draft.apiKey
      ? ({ kind: "replace", value: draft.apiKey } as const)
      : ({ kind: "clear" } as const);

const probeRequest = (draft: LlmProviderDraft): LlmProviderProbeRequest => ({
  id: draft.id,
  kind: draft.kind,
  baseUrl: draft.baseUrl,
  modelId: draft.modelId,
  inputTokenBudget: draft.inputTokenBudget,
  maxOutputTokens: draft.maxOutputTokens,
  apiKey: keyUpdate(draft),
});

const kindLabel = (kind: LlmProviderKind) => kind === "openAi" ? "Responses" : "Compatible";

export function LlmProviderManager(props: {
  model: SettingsModel;
  controller: SettingsController;
  onBack: () => void;
  onDirtyChange: (dirty: boolean) => void;
}) {
  const initialProvider = props.model.llmProviders.find(
    (provider) => provider.id === props.model.settings.activeLlmProviderId,
  ) ?? props.model.llmProviders[0];
  const [draft, setDraft] = useState<LlmProviderDraft | null>(
    initialProvider ? toDraft(initialProvider) : null,
  );
  const [baseline, setBaseline] = useState<LlmProviderDraft | null>(
    initialProvider ? toDraft(initialProvider) : null,
  );
  const [models, setModels] = useState<LlmModel[]>([]);
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState<ProviderFeedback | null>(null);

  const dirty = useMemo(
    () => JSON.stringify(draft) !== JSON.stringify(baseline),
    [baseline, draft],
  );
  useEffect(() => props.onDirtyChange(dirty), [dirty, props.onDirtyChange]);
  useEffect(() => () => props.onDirtyChange(false), [props.onDirtyChange]);

  useEffect(() => {
    if (draft?.id && !props.model.llmProviders.some((provider) => provider.id === draft.id)) {
      setDraft(null);
      setBaseline(null);
      setModels([]);
    }
  }, [draft?.id, props.model.llmProviders]);

  const confirmDiscard = () => !dirty || confirm("模型服务配置尚未保存。放弃这些更改吗？");
  const selectDraft = (next: LlmProviderDraft) => {
    if (!confirmDiscard()) return;
    setDraft(next);
    setBaseline(next.id ? next : null);
    setModels([]);
    setFeedback(null);
  };
  const changeDraft = (next: LlmProviderDraft, clearModels = true) => {
    setDraft(next);
    if (clearModels) setModels([]);
    setFeedback(null);
  };

  const saveProvider = async () => {
    if (!draft) return;
    setBusy(true);
    setFeedback({ tone: "loading", message: "正在保存 AI 模型服务…" });
    try {
      const saved = await props.controller.saveLlmProvider({
        id: draft.id,
        name: draft.name,
        kind: draft.kind,
        baseUrl: draft.baseUrl,
        modelId: draft.modelId,
        inputTokenBudget: draft.inputTokenBudget,
        maxOutputTokens: draft.maxOutputTokens,
        apiKey: keyUpdate(draft),
      });
      const next = toDraft(saved);
      setDraft(next);
      setBaseline(next);
      setFeedback({
        tone: llmProviderReady(saved) ? "success" : "warning",
        message: llmProviderReady(saved)
          ? "AI 模型服务已保存。"
          : "服务已保存；OpenAI 官方服务需配置 API Key 后才能用于生成。",
      });
    } catch (error) {
      setFeedback({ tone: "error", message: String(error) });
    } finally {
      setBusy(false);
    }
  };

  const testConnection = async () => {
    if (!draft) return;
    setBusy(true);
    setFeedback({ tone: "loading", message: "正在发送不含会议内容的测试请求…" });
    try {
      const result = await props.controller.testLlmProvider(probeRequest(draft));
      setFeedback({ tone: "success", message: result.message });
    } catch (error) {
      setFeedback({ tone: "error", message: String(error) });
    } finally {
      setBusy(false);
    }
  };

  const loadModels = async () => {
    if (!draft) return;
    setBusy(true);
    setFeedback({ tone: "loading", message: "正在读取模型列表…" });
    try {
      const next = await props.controller.listLlmModels(probeRequest(draft));
      setModels(next);
      setFeedback(next.length
        ? { tone: "success", message: `读取到 ${next.length} 个模型。` }
        : { tone: "warning", message: "服务未返回模型，可继续手动填写模型 ID。" });
    } catch (error) {
      setModels([]);
      setFeedback({ tone: "error", message: `${String(error)}；仍可手动填写模型 ID。` });
    } finally {
      setBusy(false);
    }
  };

  const deleteProvider = async () => {
    if (!draft?.id || !confirm(`删除 AI 模型服务“${draft.name}”？`)) return;
    setBusy(true);
    try {
      await props.controller.deleteLlmProvider(draft.id);
      const nextProvider = props.model.llmProviders.find((provider) => provider.id !== draft.id);
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

  const persistedList = props.model.llmProviders.map((provider) => {
    const active = provider.id === props.model.settings.activeLlmProviderId;
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
          <small>{provider.modelId} · {kindLabel(provider.kind)}</small>
        </span>
        {active ? <em>默认</em> : !llmProviderReady(provider) ? <em className="warning">缺少 Key</em> : null}
      </button>
    );
  });
  const list = (
    <>
      {draft?.id === null && (
        <button
          type="button"
          className="resource-draft selected"
          aria-label={`正在添加 ${draft.name.trim() || "未命名模型服务"} ${kindLabel(draft.kind)}`}
        >
          <span>
            <strong>{draft.name.trim() || "未命名模型服务"}</strong>
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
      title="AI 文档 / 模型服务"
      description="添加和测试用于生成 Markdown 的模型服务。"
      listTitle="模型服务"
      count={props.model.llmProviders.length + (draft?.id === null ? 1 : 0)}
      onBack={() => confirmDiscard() && props.onBack()}
      onAdd={() => selectDraft(emptyDraft())}
      list={list}
      detail={draft ? (
        <div className="provider-detail-form">
          <header>
            <div>
              <h2>{draft.id ? draft.name : "添加模型服务"}</h2>
              {draft.id === props.model.settings.activeLlmProviderId && <span>默认服务</span>}
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
                <span>类型</span>
                <SelectControl
                  value={draft.kind}
                  onChange={(event) => changeDraft({ ...draft, kind: event.target.value as LlmProviderKind })}
                >
                  <option value="openAi">Responses API</option>
                  <option value="openAiCompatible">OpenAI-compatible Chat</option>
                </SelectControl>
              </label>
              <label className="wide">
                <span>API Base URL</span>
                <input value={draft.baseUrl} onChange={(event) => changeDraft({ ...draft, baseUrl: event.target.value })} />
              </label>
              <label className="wide">
                <span>API Key</span>
                <input
                  type="password"
                  autoComplete="new-password"
                  value={draft.apiKey}
                  placeholder={isOfficialOpenAiUrl(draft.kind, draft.baseUrl) ? "OpenAI 官方服务必填" : "按服务要求，可留空"}
                  onFocus={(event) => draft.apiKey === KEY_MASK && event.currentTarget.select()}
                  onChange={(event) => changeDraft({ ...draft, apiKey: event.target.value })}
                />
              </label>
              <label className="wide">
                <span>模型 ID</span>
                <input value={draft.modelId} onChange={(event) => changeDraft({ ...draft, modelId: event.target.value }, false)} />
              </label>
              {models.length > 0 && (
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
              <label>
                <span>最大输入 tokens</span>
                <input
                  type="number"
                  min={1024}
                  value={draft.inputTokenBudget}
                  onChange={(event) => changeDraft({ ...draft, inputTokenBudget: Number(event.target.value) }, false)}
                />
              </label>
              <label>
                <span>最大输出 tokens</span>
                <input
                  type="number"
                  min={256}
                  value={draft.maxOutputTokens}
                  onChange={(event) => changeDraft({ ...draft, maxOutputTokens: Number(event.target.value) }, false)}
                />
              </label>
            </div>
          </fieldset>
          <div className="provider-probe-actions">
            <button type="button" className="button secondary" disabled={busy} onClick={() => void testConnection()}>
              <Wifi size={14} /> 测试连接
            </button>
            <button type="button" className="button secondary" disabled={busy || !draft.baseUrl.trim()} onClick={() => void loadModels()}>
              <RefreshCw size={14} /> 获取模型
            </button>
            <span>测试请求不包含会议或转写内容。</span>
          </div>
          {feedback && (
            <InlineStatus tone={feedback.tone === "loading" ? "neutral" : feedback.tone}>
              {feedback.tone === "loading" ? <LoaderCircle size={15} className="spinning" />
                : feedback.tone === "success" ? <CheckCircle2 size={15} />
                  : <AlertTriangle size={15} />}
              <span>{feedback.message}</span>
            </InlineStatus>
          )}
          {draft.kind === "openAi" && (
            <InlineStatus tone="warning">
              <AlertTriangle size={15} />
              <span>Responses API 会发送 store: false；这不代表零数据保留，请查看所选服务的数据控制说明。</span>
            </InlineStatus>
          )}
          <footer className="provider-detail-actions">
            <span>{dirty ? "有未保存的更改" : "配置已保存"}</span>
            <div>
              <button type="button" className="button secondary" disabled={!dirty || busy} onClick={() => {
                setDraft(baseline);
                setFeedback(null);
              }}>取消更改</button>
              <button type="button" className="button primary" disabled={!dirty || busy} onClick={() => void saveProvider()}>
                {busy && <LoaderCircle size={14} className="spinning" />} 保存服务
              </button>
            </div>
          </footer>
        </div>
      ) : (
        <ResourceEmptyState
          title="尚未配置模型服务"
          description="添加 Provider 后，可以从转写生成 Markdown 文档。"
          actionLabel="添加服务"
          onAction={() => selectDraft(emptyDraft())}
        />
      )}
    />
  );
}
