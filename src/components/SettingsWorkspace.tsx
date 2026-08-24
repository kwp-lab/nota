import { useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  Bot,
  CheckCircle2,
  FileText,
  Folder,
  Info,
  Keyboard,
  LoaderCircle,
  Mic,
  Plus,
  RefreshCw,
  ShieldCheck,
  Trash2,
  Wifi,
  X,
} from "lucide-react";
import type {
  AppSettings,
  AsrConnectionTest,
  AsrModel,
  AsrProvider,
  AsrProviderKind,
  AsrProviderProbeRequest,
  LlmConnectionTest,
  LlmModel,
  LlmProvider,
  LlmProviderProbeRequest,
  SaveLlmProviderRequest,
  SaveAsrProviderRequest,
} from "../types";
import { AiSettingsSection } from "./AiSettingsSection";
import { AppTooltip } from "./AppTooltip";

interface SettingsWorkspaceProps {
  firstRun: boolean;
  dirty: boolean;
  recordingActive: boolean;
  settings: AppSettings;
  providers: AsrProvider[];
  llmProviders: LlmProvider[];
  microphoneCount: number;
  appVersion: string;
  onChange: (settings: AppSettings) => void;
  onChooseOutput: () => void;
  onChooseAiDocuments: () => void;
  onOpenMicrophoneSettings: () => void;
  onOpenLogDirectory: () => void;
  onSaveProvider: (request: SaveAsrProviderRequest) => Promise<AsrProvider>;
  onDeleteProvider: (id: string) => Promise<void>;
  onTestProvider: (request: AsrProviderProbeRequest) => Promise<AsrConnectionTest>;
  onListModels: (request: AsrProviderProbeRequest) => Promise<AsrModel[]>;
  onSaveLlmProvider: (request: SaveLlmProviderRequest) => Promise<LlmProvider>;
  onDeleteLlmProvider: (id: string) => Promise<void>;
  onTestLlmProvider: (request: LlmProviderProbeRequest) => Promise<LlmConnectionTest>;
  onListLlmModels: (request: LlmProviderProbeRequest) => Promise<LlmModel[]>;
  onDiscardChanges: () => void;
  onSave: () => void;
  onSkipFirstRun: () => void;
}

interface ProviderDraft {
  id: string | null;
  name: string;
  kind: AsrProviderKind;
  baseUrl: string;
  apiKey: string;
  modelId: string;
  hasApiKey: boolean;
}

type FeedbackTone = "loading" | "success" | "warning" | "error";

interface Feedback {
  tone: FeedbackTone;
  message: string;
}

const STORED_API_KEY_MASK = "••••••••";
const DASHSCOPE_BASE_URL = "https://dashscope.aliyuncs.com/api/v1";
const DASHSCOPE_MODEL = "qwen-audio-3.0-asr-flash-filetrans";

const providerKindLabel = (kind: AsrProviderKind) => {
  if (kind === "funAsr") return "FunASR";
  if (kind === "dashScope") return "阿里云千问";
  return "Compatible";
};

const emptyProvider = (): ProviderDraft => ({
  id: null,
  name: "本地 FunASR",
  kind: "funAsr",
  baseUrl: "http://127.0.0.1:8000/v1",
  apiKey: "",
  modelId: "sensevoice",
  hasApiKey: false,
});

const providerDraft = (provider: AsrProvider): ProviderDraft => ({
  id: provider.id,
  name: provider.name,
  kind: provider.kind,
  baseUrl: provider.baseUrl,
  apiKey: provider.hasApiKey ? STORED_API_KEY_MASK : "",
  modelId: provider.modelId,
  hasApiKey: provider.hasApiKey,
});

const changeProviderKind = (draft: ProviderDraft, kind: AsrProviderKind): ProviderDraft => {
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

const apiKeyUpdate = (draft: ProviderDraft) =>
  draft.hasApiKey && draft.apiKey === STORED_API_KEY_MASK
    ? ({ kind: "keep" } as const)
    : draft.apiKey
      ? ({ kind: "replace", value: draft.apiKey } as const)
      : ({ kind: "clear" } as const);

const probeRequest = (draft: ProviderDraft): AsrProviderProbeRequest => ({
  id: draft.id,
  kind: draft.kind,
  baseUrl: draft.baseUrl,
  modelId: draft.modelId,
  apiKey: apiKeyUpdate(draft),
});

function InlineFeedback({ feedback }: { feedback: Feedback | null }) {
  if (!feedback) return null;
  return (
    <div
      className={`provider-feedback feedback-${feedback.tone}`}
      role={feedback.tone === "error" ? "alert" : "status"}
      aria-live="polite"
    >
      {feedback.tone === "loading" ? (
        <LoaderCircle size={15} className="spinning" />
      ) : feedback.tone === "success" ? (
        <CheckCircle2 size={15} />
      ) : (
        <AlertTriangle size={15} />
      )}
      <span>{feedback.message}</span>
    </div>
  );
}

export function SettingsWorkspace(props: SettingsWorkspaceProps) {
  const [editing, setEditing] = useState<ProviderDraft | null>(null);
  const [models, setModels] = useState<AsrModel[]>([]);
  const [testingConnection, setTestingConnection] = useState(false);
  const [loadingModels, setLoadingModels] = useState(false);
  const [savingProvider, setSavingProvider] = useState(false);
  const [connectionFeedback, setConnectionFeedback] = useState<Feedback | null>(null);
  const [modelFeedback, setModelFeedback] = useState<Feedback | null>(null);
  const [saveFeedback, setSaveFeedback] = useState<Feedback | null>(null);

  const activeProvider = useMemo(
    () => props.providers.find((provider) => provider.id === props.settings.activeAsrProviderId),
    [props.providers, props.settings.activeAsrProviderId],
  );

  useEffect(() => {
    if (
      editing?.id &&
      !props.providers.some((provider) => provider.id === editing.id)
    ) {
      setEditing(null);
      setModels([]);
      setConnectionFeedback(null);
      setModelFeedback(null);
      setSaveFeedback(null);
    }
  }, [editing?.id, props.providers]);

  const selectProvider = (draft: ProviderDraft) => {
    setEditing(draft);
    setModels([]);
    setConnectionFeedback(null);
    setModelFeedback(null);
    setSaveFeedback(null);
  };

  const changeConnectionField = (next: ProviderDraft) => {
    setEditing(next);
    setModels([]);
    setConnectionFeedback(null);
    setModelFeedback(null);
    setSaveFeedback(null);
  };

  const changeModelId = (modelId: string) => {
    if (!editing) return;
    setEditing({ ...editing, modelId });
    setConnectionFeedback(null);
    setSaveFeedback(null);
  };

  const applyReturnedModels = (next: AsrModel[]) => {
    setModels(next);
    if (editing && !editing.modelId.trim() && next.length === 1) {
      setEditing({ ...editing, modelId: next[0].id });
    }
  };

  const saveProvider = async () => {
    if (!editing) return;
    setSavingProvider(true);
    setSaveFeedback({ tone: "loading", message: "正在保存服务配置…" });
    try {
      const saved = await props.onSaveProvider({
        id: editing.id,
        name: editing.name,
        kind: editing.kind,
        baseUrl: editing.baseUrl,
        modelId: editing.modelId,
        apiKey: apiKeyUpdate(editing),
      });
      setEditing(providerDraft(saved));
      if (!props.settings.activeAsrProviderId) {
        props.onChange({ ...props.settings, activeAsrProviderId: saved.id });
      }
      setSaveFeedback({ tone: "success", message: "服务配置已保存。" });
    } catch (error) {
      setSaveFeedback({ tone: "error", message: String(error) });
    } finally {
      setSavingProvider(false);
    }
  };

  const testConnection = async () => {
    if (!editing) return;
    setTestingConnection(true);
    setConnectionFeedback({ tone: "loading", message: "正在连接服务并检查接口…" });
    try {
      const result = await props.onTestProvider(probeRequest(editing));
      applyReturnedModels(result.models);
      const details = [
        result.message,
        result.device ? `设备：${result.device}` : null,
        result.models.length ? `发现 ${result.models.length} 个模型` : null,
      ]
        .filter(Boolean)
        .join(" · ");
      setConnectionFeedback({
        tone: result.level,
        message: details,
      });
    } catch (error) {
      setConnectionFeedback({
        tone: "error",
        message: `连接失败：${String(error)}`,
      });
    } finally {
      setTestingConnection(false);
    }
  };

  const refreshModels = async () => {
    if (!editing) return;
    setLoadingModels(true);
    setModelFeedback({ tone: "loading", message: "正在从服务读取模型列表…" });
    try {
      const next = await props.onListModels(probeRequest(editing));
      applyReturnedModels(next);
      setModelFeedback(
        next.length
          ? { tone: "success", message: `已读取 ${next.length} 个模型，请从下方列表选择。` }
          : { tone: "warning", message: "服务没有返回模型，仍可手工填写模型 ID。" },
      );
    } catch (error) {
      setModels([]);
      setModelFeedback({
        tone: "error",
        message: `无法获取模型列表：${String(error)}。仍可手工填写模型 ID。`,
      });
    } finally {
      setLoadingModels(false);
    }
  };

  const deleteProvider = async () => {
    if (!editing?.id) return;
    if (!confirm(`删除语音转写服务“${editing.name}”？`)) return;
    setSavingProvider(true);
    try {
      await props.onDeleteProvider(editing.id);
      setEditing(null);
      setModels([]);
    } catch (error) {
      setSaveFeedback({ tone: "error", message: String(error) });
    } finally {
      setSavingProvider(false);
    }
  };

  return (
    <section className="settings-workspace">
      <header className="settings-page-header">
        <div>
          <p className="eyebrow">PREFERENCES</p>
          <h1>设置</h1>
          <p className="muted">录音设置和语音服务配置只保存在这台电脑上。</p>
        </div>
      </header>

      {props.firstRun && (
        <div className="first-run-banner">
          <ShieldCheck size={22} />
          <div>
            <strong>欢迎使用 Nota</strong>
            <span>建议确认麦克风、保存目录和托盘行为；也可以稍后再设置。</span>
          </div>
        </div>
      )}

      {props.recordingActive && (
        <div className="settings-recording-note">
          <AlertTriangle size={16} />
          当前录音不会被设置页操作中断；这里保存的录音设置将在下一次录音时生效。
        </div>
      )}

      <div className="settings-group">
        <div className="settings-heading">
          <Mic size={17} />
          <div>
            <strong>麦克风与回声消除</strong>
            <span>
              {props.microphoneCount > 0
                ? `已发现 ${props.microphoneCount} 个可用输入设备`
                : "未发现输入设备；请检查权限或连接"}
            </span>
          </div>
          <button type="button" className="text-button" onClick={props.onOpenMicrophoneSettings}>
            Windows 设置
          </button>
        </div>
        <label className="settings-row">
          <span>回声消除</span>
          <select
            value={props.settings.aecMode}
            onChange={(event) =>
              props.onChange({
                ...props.settings,
                aecMode: event.target.value as AppSettings["aecMode"],
              })
            }
          >
            <option value="auto">自动（扬声器开、耳机关）</option>
            <option value="on">强制开启</option>
            <option value="off">强制关闭</option>
          </select>
        </label>
      </div>

      <AiSettingsSection
        settings={props.settings}
        providers={props.llmProviders}
        onChange={props.onChange}
        onChooseDirectory={props.onChooseAiDocuments}
        onSaveProvider={props.onSaveLlmProvider}
        onDeleteProvider={props.onDeleteLlmProvider}
        onTestProvider={props.onTestLlmProvider}
        onListModels={props.onListLlmModels}
      />

      <div className="settings-group">
        <div className="settings-heading">
          <Folder size={17} />
          <div>
            <strong>默认保存目录</strong>
            <span className="path-preview">{props.settings.outputDirectory}</span>
          </div>
          <button type="button" className="text-button" onClick={props.onChooseOutput}>
            更改
          </button>
        </div>
      </div>

      <div className="settings-group transcription-settings">
        <div className="settings-heading">
          <Bot size={17} />
          <div>
            <strong>语音转写</strong>
            <span>支持 Nota ASR Server、OpenAI-compatible 与阿里云千问文件转写</span>
          </div>
          <button type="button" className="text-button" onClick={() => selectProvider(emptyProvider())}>
            <Plus size={14} /> 添加
          </button>
        </div>

        <label className="settings-row">
          <span>默认服务</span>
          <select
            aria-label="默认语音转写服务"
            value={props.settings.activeAsrProviderId ?? ""}
            onChange={(event) =>
              props.onChange({
                ...props.settings,
                activeAsrProviderId: event.target.value || null,
                autoTranscribe: event.target.value
                  ? props.settings.autoTranscribe
                  : false,
              })
            }
          >
            <option value="">未选择</option>
            {props.providers.map((provider) => (
              <option value={provider.id} key={provider.id}>{provider.name}</option>
            ))}
          </select>
        </label>
        {activeProvider?.kind === "dashScope" && (
          <p className="upload-disclosure" role="status">
            <Wifi size={14} />
            千问云转写会把完整录音上传至阿里云临时存储，约 48 小时后清理；支持匿名说话人分离，但不支持 Nota 声纹分析。
          </p>
        )}
        <label className="settings-row">
          <span>
            <strong>录音结束后自动转写</strong>
            <small>
              {activeProvider?.kind === "dashScope"
                ? "停止并保存后会自动上传完整录音至阿里云；录音进行中不会上传。"
                : "仅在停止并保存后开始；录音进行中不会上传。"}
            </small>
          </span>
          <input
            type="checkbox"
            aria-label="录音结束后自动转写"
            disabled={!props.settings.activeAsrProviderId}
            checked={props.settings.autoTranscribe}
            onChange={(event) =>
              props.onChange({ ...props.settings, autoTranscribe: event.target.checked })
            }
          />
        </label>

        {props.providers.length > 0 && (
          <div className="provider-list">
            {props.providers.map((provider) => (
              <button
                type="button"
                key={provider.id}
                className={provider.id === activeProvider?.id ? "active" : ""}
                onClick={() => selectProvider(providerDraft(provider))}
              >
                <span>
                  <strong>{provider.name}</strong>
                  <small>{provider.baseUrl} · {provider.modelId}</small>
                </span>
                <span>{providerKindLabel(provider.kind)}</span>
              </button>
            ))}
          </div>
        )}

        {editing && (
          <div className="provider-editor">
            <div className="provider-editor-title">
              <strong>{editing.id ? "编辑服务" : "添加服务"}</strong>
              <AppTooltip content="关闭">
                <button
                  type="button"
                  className="icon-button"
                  aria-label="关闭服务编辑"
                  onClick={() => setEditing(null)}
                >
                  <X size={15} />
                </button>
              </AppTooltip>
            </div>
            <div className="provider-form">
              <label>
                <span>名称</span>
                <input
                  value={editing.name}
                  onChange={(event) => {
                    setEditing({ ...editing, name: event.target.value });
                    setSaveFeedback(null);
                  }}
                />
              </label>
              <label>
                <span>服务类型</span>
                <select
                  value={editing.kind}
                  onChange={(event) => changeConnectionField(changeProviderKind(
                    editing,
                    event.target.value as AsrProviderKind,
                  ))}
                >
                  <option value="funAsr">FunASR</option>
                  <option value="openAiCompatible">OpenAI-compatible</option>
                  <option value="dashScope">阿里云千问（DashScope）</option>
                </select>
              </label>
              <label className="wide">
                <span>API Base URL（以 /v1 为根）</span>
                <input
                  value={editing.baseUrl}
                  readOnly={editing.kind === "dashScope"}
                  placeholder="http://192.168.1.10:8000/v1"
                  onChange={(event) =>
                    changeConnectionField({ ...editing, baseUrl: event.target.value })
                  }
                />
              </label>
              <label className="wide">
                <span>API Key</span>
                <input
                  type="password"
                  value={editing.apiKey}
                  placeholder="可留空"
                  autoComplete="new-password"
                  onFocus={(event) => {
                    if (editing.apiKey === STORED_API_KEY_MASK) {
                      event.currentTarget.select();
                    }
                  }}
                  onChange={(event) =>
                    changeConnectionField({
                      ...editing,
                      apiKey: event.target.value,
                    })
                  }
                />
              </label>
              <label className="wide">
                <span>模型 ID</span>
                <input
                  value={editing.modelId}
                  readOnly={editing.kind === "dashScope"}
                  placeholder="例如 sensevoice"
                  onChange={(event) => changeModelId(event.target.value)}
                />
              </label>
              {editing.kind !== "dashScope" && <div className="model-fetch-row wide">
                <button
                  type="button"
                  className="button secondary"
                  disabled={loadingModels || !editing.baseUrl.trim()}
                  onClick={() => void refreshModels()}
                >
                  <RefreshCw size={15} className={loadingModels ? "spinning" : ""} />
                  {loadingModels ? "正在获取…" : "获取模型列表"}
                </button>
                <span>访问当前服务的 `/models`，不会保存配置或上传录音。</span>
              </div>}
              {editing.kind !== "dashScope" && models.length > 0 && (
                <label className="wide returned-models">
                  <span>服务返回的模型（{models.length}）</span>
                  <select
                    aria-label="服务返回的模型"
                    value={models.some((model) => model.id === editing.modelId) ? editing.modelId : ""}
                    onChange={(event) => changeModelId(event.target.value)}
                  >
                    <option value="">选择一个模型</option>
                    {models.map((model) => (
                      <option
                        key={model.id}
                        value={model.id}
                        disabled={model.ready === false}
                      >
                        {model.id}
                        {model.ownedBy ? ` · ${model.ownedBy}` : ""}
                        {model.ready === false ? " · 未就绪" : ""}
                      </option>
                    ))}
                  </select>
                </label>
              )}
              {editing.kind === "dashScope" && (
                <p className="upload-disclosure wide">
                  <AlertTriangle size={14} />
                  完整 Ogg 录音会上传至阿里云临时存储并约在 48 小时后清理。单次最长 2 小时，始终开启匿名说话人分离；本次转写不支持 Nota 声纹分析。
                </p>
              )}
            </div>

            <InlineFeedback feedback={modelFeedback} />

            <div className="provider-test-row">
              <button
                type="button"
                className="button secondary"
                disabled={testingConnection || !editing.baseUrl.trim()}
                onClick={() => void testConnection()}
              >
                {testingConnection ? (
                  <LoaderCircle size={15} className="spinning" />
                ) : (
                  <Wifi size={15} />
                )}
                {testingConnection ? "测试中…" : "测试连接"}
              </button>
              <span>
                {editing.kind === "dashScope"
                  ? "只获取临时上传凭证，不上传音频，也不创建计费任务。"
                  : "检测健康状态和模型接口，不上传录音文件。"}
              </span>
            </div>

            <InlineFeedback feedback={connectionFeedback} />

            <div className="provider-actions">
              {editing.id && (
                <button
                  type="button"
                  className="button danger-button"
                  disabled={savingProvider}
                  onClick={() => void deleteProvider()}
                >
                  <Trash2 size={15} /> 删除
                </button>
              )}
              <button
                type="button"
                className="button primary provider-save"
                disabled={savingProvider}
                onClick={() => void saveProvider()}
              >
                {savingProvider && <LoaderCircle size={15} className="spinning" />}
                {savingProvider ? "保存中…" : "保存服务"}
              </button>
            </div>
            <InlineFeedback feedback={saveFeedback} />
          </div>
        )}

        <p className="upload-disclosure">
          <Wifi size={14} />
          只有手动开始转写，或明确开启自动转写后，录音音频才会发送到所选服务。
        </p>
      </div>

      <div className="settings-group">
        <div className="settings-heading">
          <Keyboard size={17} />
          <div>
            <strong>全局快捷键</strong>
            <span>发生冲突时不会覆盖其他应用。</span>
          </div>
          <label className="switch-label">
            <input
              type="checkbox"
              checked={props.settings.shortcutsEnabled}
              onChange={(event) =>
                props.onChange({ ...props.settings, shortcutsEnabled: event.target.checked })
              }
            />
            启用
          </label>
        </div>
        {props.settings.shortcutsEnabled && (
          <div className="shortcut-grid">
            <label>
              <span>开始 / 暂停 / 继续</span>
              <input
                value={props.settings.toggleShortcut}
                onChange={(event) =>
                  props.onChange({ ...props.settings, toggleShortcut: event.target.value })
                }
              />
            </label>
            <label>
              <span>停止并保存</span>
              <input
                value={props.settings.stopShortcut}
                onChange={(event) =>
                  props.onChange({ ...props.settings, stopShortcut: event.target.value })
                }
              />
            </label>
          </div>
        )}
      </div>

      <div className="settings-group">
        <div className="settings-heading">
          <FileText size={17} />
          <div>
            <strong>诊断与日志</strong>
            <span>技术日志仅保存在本机并自动轮转，不包含录音、转写正文或 AI 请求与响应内容。</span>
          </div>
          <button type="button" className="text-button" onClick={props.onOpenLogDirectory}>
            打开日志目录
          </button>
        </div>
      </div>

      <div className="settings-group about-group">
        <div className="settings-heading">
          <Info size={17} />
          <div>
            <strong>关于此应用</strong>
            <span>Nota · 本地优先的 Windows 会议录音工具</span>
          </div>
          <span className="version-badge">v{props.appVersion}</span>
        </div>
        <div className="about-details">
          <span>Windows 11 x64</span>
          <span>本地录音 · 无账号 · 无遥测</span>
          <span>© 2026 Nota Contributors</span>
        </div>
      </div>

      {props.firstRun && (
        <div className="tray-note">
          关闭主窗口后应用会留在系统托盘。录音中退出时必须先选择“停止并保存”或取消退出。
        </div>
      )}

      <div className="settings-savebar">
        <div>
          {props.dirty ? (
            <>
              <AlertTriangle size={15} />
              <span>有未保存的更改</span>
            </>
          ) : (
            <>
              <CheckCircle2 size={15} />
              <span>所有普通设置均已保存</span>
            </>
          )}
        </div>
        <div>
          {props.firstRun && (
            <button type="button" className="button secondary" onClick={props.onSkipFirstRun}>
              稍后设置
            </button>
          )}
          <button
            type="button"
            className="button secondary"
            disabled={!props.dirty}
            onClick={props.onDiscardChanges}
          >
            取消更改
          </button>
          <button
            type="button"
            className="button primary"
            disabled={!props.dirty && !props.firstRun}
            onClick={props.onSave}
          >
            {props.firstRun ? "保存并完成" : "保存设置"}
          </button>
        </div>
      </div>
    </section>
  );
}
