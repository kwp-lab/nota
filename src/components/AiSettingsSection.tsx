import {
  AlertTriangle,
  Bot,
  CheckCircle2,
  Copy,
  FileText,
  Folder,
  LoaderCircle,
  Plus,
  RefreshCw,
  Trash2,
  Wifi,
  X,
} from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../api";
import { isOfficialOpenAiUrl, llmProviderReady, OPENAI_API_ROOT } from "../llm";
import type {
  AiTemplate,
  AppSettings,
  LlmConnectionTest,
  LlmModel,
  LlmProvider,
  LlmProviderKind,
  LlmProviderProbeRequest,
  SaveAiTemplateRequest,
  SaveLlmProviderRequest,
} from "../types";

interface AiSettingsSectionProps {
  settings: AppSettings;
  providers: LlmProvider[];
  onChange: (settings: AppSettings) => void;
  onChooseDirectory: () => void;
  onSaveProvider: (request: SaveLlmProviderRequest) => Promise<LlmProvider>;
  onDeleteProvider: (id: string) => Promise<void>;
  onTestProvider: (request: LlmProviderProbeRequest) => Promise<LlmConnectionTest>;
  onListModels: (request: LlmProviderProbeRequest) => Promise<LlmModel[]>;
}

interface LlmDraft {
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

const emptyLlmDraft = (): LlmDraft => ({
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

const toDraft = (provider: LlmProvider): LlmDraft => ({
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

const keyUpdate = (draft: LlmDraft) =>
  draft.hasApiKey && draft.apiKey === KEY_MASK
    ? ({ kind: "keep" } as const)
    : draft.apiKey
      ? ({ kind: "replace", value: draft.apiKey } as const)
      : ({ kind: "clear" } as const);

const probe = (draft: LlmDraft): LlmProviderProbeRequest => ({
  id: draft.id,
  kind: draft.kind,
  baseUrl: draft.baseUrl,
  modelId: draft.modelId,
  inputTokenBudget: draft.inputTokenBudget,
  maxOutputTokens: draft.maxOutputTokens,
  apiKey: keyUpdate(draft),
});

const emptyTemplate = (): SaveAiTemplateRequest => ({
  id: null,
  name: "自定义会议模板",
  description: "",
  taskInstructions: "",
  outputRequirements: "使用 Markdown 输出。",
  requiresSpeakerLabels: false,
});

export function AiSettingsSection(props: AiSettingsSectionProps) {
  const [editing, setEditing] = useState<LlmDraft | null>(null);
  const [models, setModels] = useState<LlmModel[]>([]);
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState<string | null>(null);
  const [feedbackError, setFeedbackError] = useState(false);
  const [templates, setTemplates] = useState<AiTemplate[]>([]);
  const [templateDraft, setTemplateDraft] = useState<SaveAiTemplateRequest | null>(null);
  const [templateBusy, setTemplateBusy] = useState(false);

  const refreshTemplates = () =>
    api.listAiTemplates().then(setTemplates).catch((error) => {
      setFeedbackError(true);
      setFeedback(String(error));
    });

  useEffect(() => {
    void refreshTemplates();
  }, []);

  useEffect(() => {
    if (editing?.id && !props.providers.some((provider) => provider.id === editing.id)) {
      setEditing(null);
    }
  }, [editing?.id, props.providers]);

  const saveProvider = async () => {
    if (!editing) return;
    setBusy(true);
    setFeedback("正在保存 AI 模型服务…");
    setFeedbackError(false);
    try {
      const saved = await props.onSaveProvider({
        id: editing.id,
        name: editing.name,
        kind: editing.kind,
        baseUrl: editing.baseUrl,
        modelId: editing.modelId,
        inputTokenBudget: editing.inputTokenBudget,
        maxOutputTokens: editing.maxOutputTokens,
        apiKey: keyUpdate(editing),
      });
      setEditing(toDraft(saved));
      if (!props.settings.activeLlmProviderId && llmProviderReady(saved)) {
        props.onChange({ ...props.settings, activeLlmProviderId: saved.id });
      }
      setFeedback(
        llmProviderReady(saved)
          ? "AI 模型服务已保存"
          : "AI 模型服务已保存；OpenAI 官方服务需配置 API Key 后才能用于生成",
      );
    } catch (error) {
      setFeedbackError(true);
      setFeedback(String(error));
    } finally {
      setBusy(false);
    }
  };

  const testProvider = async () => {
    if (!editing) return;
    setBusy(true);
    setFeedback("正在发送不含会议内容的测试请求…");
    setFeedbackError(false);
    try {
      const result = await props.onTestProvider(probe(editing));
      setFeedback(result.message);
    } catch (error) {
      setFeedbackError(true);
      setFeedback(String(error));
    } finally {
      setBusy(false);
    }
  };

  const listModels = async () => {
    if (!editing) return;
    setBusy(true);
    setFeedback("正在读取模型列表…");
    setFeedbackError(false);
    try {
      const next = await props.onListModels(probe(editing));
      setModels(next);
      setFeedback(next.length ? `读取到 ${next.length} 个模型` : "服务未返回模型，可继续手动填写模型 ID");
    } catch (error) {
      setModels([]);
      setFeedbackError(true);
      setFeedback(`${String(error)}；仍可手动填写模型 ID`);
    } finally {
      setBusy(false);
    }
  };

  const saveTemplate = async () => {
    if (!templateDraft) return;
    setTemplateBusy(true);
    try {
      await api.saveAiTemplate(templateDraft);
      setTemplateDraft(null);
      await refreshTemplates();
    } catch (error) {
      setFeedbackError(true);
      setFeedback(String(error));
    } finally {
      setTemplateBusy(false);
    }
  };

  const cloneTemplate = async (template: AiTemplate) => {
    const name = prompt("输入复制后的模板名称", `${template.name}（自定义）`);
    if (!name?.trim()) return;
    setTemplateBusy(true);
    try {
      const cloned = await api.cloneAiTemplate(template.id, name.trim());
      await refreshTemplates();
      setTemplateDraft({
        id: cloned.id,
        name: cloned.name,
        description: cloned.description,
        taskInstructions: cloned.taskInstructions,
        outputRequirements: cloned.outputRequirements,
        requiresSpeakerLabels: cloned.requiresSpeakerLabels,
      });
    } catch (error) {
      setFeedbackError(true);
      setFeedback(String(error));
    } finally {
      setTemplateBusy(false);
    }
  };

  return (
    <div className="settings-group ai-settings-group">
      <div className="settings-heading">
        <Bot size={17} />
        <div>
          <strong>AI 文档</strong>
          <span>从已完成的转写生成可分享的 Markdown；仅在手动点击生成时联网</span>
        </div>
        <button type="button" className="text-button" onClick={() => {
          setEditing(emptyLlmDraft());
          setModels([]);
          setFeedback(null);
        }}><Plus size={14} /> 添加 Provider</button>
      </div>

      <label className="settings-row">
        <span>默认 LLM Provider</span>
        <select
          value={props.settings.activeLlmProviderId ?? ""}
          onChange={(event) => props.onChange({
            ...props.settings,
            activeLlmProviderId: event.target.value || null,
          })}
        >
          <option value="">未选择</option>
          {props.providers.map((provider) => (
            <option key={provider.id} value={provider.id} disabled={!llmProviderReady(provider)}>
              {provider.name}{llmProviderReady(provider) ? "" : "（缺少 API Key）"}
            </option>
          ))}
        </select>
      </label>

      <div className="settings-row ai-directory-row">
        <span><strong>Markdown 保存目录</strong><small>{props.settings.aiDocumentsDirectory}</small></span>
        <button type="button" className="button secondary" onClick={props.onChooseDirectory}><Folder size={14} />更改</button>
      </div>

      {props.providers.length > 0 && (
        <div className="provider-list">
          {props.providers.map((provider) => (
            <button type="button" key={provider.id} onClick={() => {
              setEditing(toDraft(provider));
              setModels([]);
              setFeedback(null);
            }}>
              <span><strong>{provider.name}</strong><small>{provider.modelId} · 输入上限 {provider.inputTokenBudget.toLocaleString()}</small></span>
              <span>{llmProviderReady(provider) ? (provider.kind === "openAi" ? "Responses" : "Compatible") : "缺少 API Key"}</span>
            </button>
          ))}
        </div>
      )}

      {editing && (
        <div className="provider-editor">
          <div className="provider-editor-title"><strong>{editing.id ? "编辑 LLM Provider" : "添加 LLM Provider"}</strong><button className="icon-button" onClick={() => setEditing(null)}><X size={15} /></button></div>
          <div className="provider-form">
            <label><span>名称</span><input value={editing.name} onChange={(event) => setEditing({ ...editing, name: event.target.value })} /></label>
            <label><span>类型</span><select value={editing.kind} onChange={(event) => {
              const kind = event.target.value as LlmProviderKind;
              setEditing({
                ...editing,
                kind,
              });
            }}><option value="openAi">Responses API（OpenAI 及兼容服务）</option><option value="openAiCompatible">OpenAI-compatible Chat</option></select></label>
            <label className="wide"><span>API Base URL</span><input value={editing.baseUrl} onChange={(event) => setEditing({ ...editing, baseUrl: event.target.value })} /></label>
            <label className="wide"><span>API Key</span><input type="password" autoComplete="new-password" value={editing.apiKey} placeholder={isOfficialOpenAiUrl(editing.kind, editing.baseUrl) ? "OpenAI 官方服务必填" : "按服务要求，可留空"} onFocus={(event) => editing.apiKey === KEY_MASK && event.currentTarget.select()} onChange={(event) => setEditing({ ...editing, apiKey: event.target.value })} /></label>
            <label className="wide"><span>模型 ID</span><input value={editing.modelId} onChange={(event) => setEditing({ ...editing, modelId: event.target.value })} /></label>
            {models.length > 0 && <label className="wide"><span>服务返回的模型</span><select value={models.some((model) => model.id === editing.modelId) ? editing.modelId : ""} onChange={(event) => setEditing({ ...editing, modelId: event.target.value })}><option value="">选择模型</option>{models.map((model) => <option key={model.id} value={model.id}>{model.id}</option>)}</select></label>}
            <label><span>最大输入 tokens</span><input type="number" min={1024} value={editing.inputTokenBudget} onChange={(event) => setEditing({ ...editing, inputTokenBudget: Number(event.target.value) })} /></label>
            <label><span>最大输出 tokens</span><input type="number" min={256} value={editing.maxOutputTokens} onChange={(event) => setEditing({ ...editing, maxOutputTokens: Number(event.target.value) })} /></label>
          </div>
          <div className="provider-test-row">
            <button className="button secondary" disabled={busy} onClick={() => void testProvider()}><Wifi size={14} />测试连接</button>
            <button className="button secondary" disabled={busy} onClick={() => void listModels()}><RefreshCw size={14} />获取模型</button>
            <span>测试请求不包含会议或转写内容。</span>
          </div>
          {feedback && <div className={`provider-feedback ${feedbackError ? "feedback-error" : "feedback-success"}`}>{feedbackError ? <AlertTriangle size={15} /> : <CheckCircle2 size={15} />}<span>{feedback}</span></div>}
          <div className="provider-actions">
            {editing.id && <button className="button danger-button" disabled={busy} onClick={() => {
              if (!editing.id || !confirm(`删除 AI 模型服务“${editing.name}”？`)) return;
              void props.onDeleteProvider(editing.id).then(() => setEditing(null)).catch((error) => {
                setFeedbackError(true);
                setFeedback(String(error));
              });
            }}><Trash2 size={14} />删除</button>}
            <button className="button primary provider-save" disabled={busy} onClick={() => void saveProvider()}>{busy && <LoaderCircle className="spin" size={14} />}保存 Provider</button>
          </div>
          {editing.kind === "openAi" && <p className="upload-disclosure"><AlertTriangle size={14} />Responses API 请求会发送 <code>store: false</code>；这不代表零数据保留，请同时查看所选服务的数据控制说明。</p>}
        </div>
      )}

      <div className="ai-template-heading">
        <div><FileText size={16} /><span><strong>AI 模板</strong><small>固定安全策略不可编辑；内置模板可复制后自定义。</small></span></div>
        <button className="text-button" onClick={() => setTemplateDraft(emptyTemplate())}><Plus size={14} />新建模板</button>
      </div>
      <div className="ai-template-list">
        {templates.filter((template) => !template.archived).map((template) => (
          <article key={template.id}>
            <div><strong>{template.name}</strong><small>{template.description || "暂无描述"}</small></div>
            <span>{template.builtinKey ? "内置" : `自定义 r${template.revision}`}{template.requiresSpeakerLabels ? " · 需说话人" : ""}</span>
            <button title="复制模板" onClick={() => void cloneTemplate(template)}><Copy size={14} /></button>
            {!template.builtinKey && <button title="编辑模板" onClick={() => setTemplateDraft({ id: template.id, name: template.name, description: template.description, taskInstructions: template.taskInstructions, outputRequirements: template.outputRequirements, requiresSpeakerLabels: template.requiresSpeakerLabels })}><FileText size={14} /></button>}
            {!template.builtinKey && <button title="归档模板" onClick={() => {
              if (!confirm(`归档模板“${template.name}”？已有 AI 文档仍可继续使用它。`)) return;
              setTemplateBusy(true);
              void api.archiveAiTemplate(template.id).then(refreshTemplates).finally(() => setTemplateBusy(false));
            }}><Trash2 size={14} /></button>}
          </article>
        ))}
      </div>

      {templateDraft && (
        <div className="provider-editor ai-template-editor">
          <div className="provider-editor-title"><strong>{templateDraft.id ? "编辑自定义模板" : "新建自定义模板"}</strong><button className="icon-button" onClick={() => setTemplateDraft(null)}><X size={15} /></button></div>
          <label><span>模板名称</span><input value={templateDraft.name} onChange={(event) => setTemplateDraft({ ...templateDraft, name: event.target.value })} /></label>
          <label><span>用途说明</span><input value={templateDraft.description} onChange={(event) => setTemplateDraft({ ...templateDraft, description: event.target.value })} /></label>
          <label><span>任务指令</span><textarea rows={5} value={templateDraft.taskInstructions} onChange={(event) => setTemplateDraft({ ...templateDraft, taskInstructions: event.target.value })} /></label>
          <label><span>输出结构与要求</span><textarea rows={5} value={templateDraft.outputRequirements} onChange={(event) => setTemplateDraft({ ...templateDraft, outputRequirements: event.target.value })} /></label>
          <label className="settings-row"><span><strong>需要说话人标签</strong><small>开启后，无说话人分段的转写不能使用此模板。</small></span><input type="checkbox" aria-label="需要带说话人标签的转写" checked={templateDraft.requiresSpeakerLabels} onChange={(event) => setTemplateDraft({ ...templateDraft, requiresSpeakerLabels: event.target.checked })} /></label>
          <div className="provider-actions"><button className="button primary provider-save" disabled={templateBusy} onClick={() => void saveTemplate()}>{templateBusy && <LoaderCircle className="spin" size={14} />}保存模板</button></div>
        </div>
      )}
    </div>
  );
}
