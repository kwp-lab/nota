import { open } from "@tauri-apps/plugin-dialog";
import {
  AlertCircle,
  Clipboard,
  FileText,
  FolderOpen,
  Link2,
  LoaderCircle,
  Plus,
  RefreshCw,
  RotateCcw,
  Sparkles,
  Square,
  WandSparkles,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { api, type UnlistenFn } from "../api";
import { llmProviderReady } from "../llm";
import { AppTooltip } from "./AppTooltip";
import type {
  AiDocument,
  AiDocumentContent,
  AiDocumentVersion,
  AiGenerationMode,
  AiGenerationRequest,
  AiTemplate,
  AiWorkspace,
  LlmProvider,
  RecordingItem,
  TranscriptDocument,
} from "../types";

interface AiDocumentsPanelProps {
  recording: RecordingItem;
  transcript: TranscriptDocument | null;
  providers: LlmProvider[];
  activeProviderId: string | null;
  onMessage: (type: "success" | "error", message: string) => void;
}

interface GenerationDialogState {
  mode: AiGenerationMode;
  document: AiDocument | null;
  sourceVersion: AiDocumentVersion | null;
  templateId: string;
  title: string;
  meetingContext: string;
  documentRequirements: string;
  runRequest: string;
  providerId: string;
  modelId: string;
}

const statusLabels: Record<AiDocumentVersion["status"], string> = {
  queued: "排队中",
  generating: "生成中",
  completed: "已完成",
  failed: "失败",
  cancelled: "已取消",
  interrupted: "已中断",
};

const formatVersionLabel = (version: AiDocumentVersion) => {
  const time = new Date(version.createdAt).toLocaleString("zh-CN", {
    month: "numeric",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
  return `v${String(version.versionNumber).padStart(3, "0")} · ${time} · ${statusLabels[version.status]}`;
};

const hasSpeakerLabels = (transcript: TranscriptDocument | null) =>
  transcript?.segments.some((segment) => Boolean(segment.speaker?.trim())) ?? false;

const isSpeakerTemplate = (template: AiTemplate) => template.requiresSpeakerLabels;

export function AiDocumentsPanel(props: AiDocumentsPanelProps) {
  const [workspace, setWorkspace] = useState<AiWorkspace | null>(null);
  const [loading, setLoading] = useState(true);
  const [selectedDocumentId, setSelectedDocumentId] = useState<string | null>(null);
  const [versions, setVersions] = useState<AiDocumentVersion[]>([]);
  const [selectedVersionId, setSelectedVersionId] = useState<string | null>(null);
  const [content, setContent] = useState<AiDocumentContent | null>(null);
  const [contentLoading, setContentLoading] = useState(false);
  const [dialog, setDialog] = useState<GenerationDialogState | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [contentReloadKey, setContentReloadKey] = useState(0);
  const [estimatedTokens, setEstimatedTokens] = useState<number | null>(null);
  const [estimateError, setEstimateError] = useState<string | null>(null);
  const [estimating, setEstimating] = useState(false);
  const recordingIdRef = useRef(props.recording.id);
  const selectedDocumentIdRef = useRef(selectedDocumentId);
  const contentRequestRef = useRef(0);
  recordingIdRef.current = props.recording.id;
  selectedDocumentIdRef.current = selectedDocumentId;

  const refreshWorkspace = useCallback(async () => {
    const recordingId = props.recording.id;
    const next = await api.getAiWorkspace(recordingId);
    if (recordingIdRef.current === recordingId) {
      setWorkspace(next);
      setSelectedDocumentId((current) => {
        if (current && next.documents.some((document) => document.id === current)) return current;
        return next.documents[0]?.id ?? null;
      });
    }
    return next;
  }, [props.recording.id]);

  const refreshVersions = useCallback(async (documentId: string) => {
    const recordingId = recordingIdRef.current;
    const next = await api.listAiDocumentVersions(documentId);
    if (
      recordingIdRef.current === recordingId
      && selectedDocumentIdRef.current === documentId
    ) {
      setVersions(next);
      setSelectedVersionId((current) => {
        if (current && next.some((version) => version.id === current)) return current;
        return next.find((version) => version.status === "completed")?.id ?? next[0]?.id ?? null;
      });
    }
    return next;
  }, []);

  useEffect(() => {
    let active = true;
    contentRequestRef.current += 1;
    setLoading(true);
    setWorkspace(null);
    setSelectedDocumentId(null);
    setVersions([]);
    setSelectedVersionId(null);
    setContent(null);
    void refreshWorkspace()
      .catch((error) => props.onMessage("error", String(error)))
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [props.recording.id, props.onMessage, refreshWorkspace]);

  useEffect(() => {
    if (!selectedDocumentId) {
      setVersions([]);
      setSelectedVersionId(null);
      return;
    }
    void refreshVersions(selectedDocumentId).catch((error) =>
      props.onMessage("error", String(error)),
    );
  }, [props.onMessage, refreshVersions, selectedDocumentId]);

  useEffect(() => {
    const requestId = ++contentRequestRef.current;
    const version = versions.find((item) => item.id === selectedVersionId);
    if (
      !version
      || version.status !== "completed"
      || version.fileState === "missing"
    ) {
      setContent(null);
      setContentLoading(false);
      return;
    }
    setContentLoading(true);
    void api
      .readAiDocumentVersion(version.id)
      .then((next) => {
        if (contentRequestRef.current === requestId && next.version.id === version.id) {
          setContent(next);
        }
      })
      .catch((error) => {
        if (contentRequestRef.current === requestId) {
          setContent(null);
          props.onMessage("error", String(error));
        }
      })
      .finally(() => {
        if (contentRequestRef.current === requestId) setContentLoading(false);
      });
    return () => {
      if (contentRequestRef.current === requestId) contentRequestRef.current += 1;
    };
  }, [contentReloadKey, props.onMessage, selectedVersionId, versions]);

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let disposed = false;
    void api.onAiStatus((event) => {
      if (event.recordingId !== props.recording.id) return;
      void refreshWorkspace().catch((error) => props.onMessage("error", String(error)));
      if (event.documentId === selectedDocumentId) {
        void refreshVersions(event.documentId).catch((error) =>
          props.onMessage("error", String(error)),
        );
      }
      if (event.version.status === "completed") {
        setSelectedDocumentId(event.documentId);
        setSelectedVersionId(event.version.id);
        props.onMessage("success", "AI Markdown 文档已生成");
      }
      if (event.version.status === "failed") {
        props.onMessage("error", event.version.errorMessage ?? "AI 文档生成失败");
      }
    }).then((dispose) => {
      if (disposed) dispose();
      else unlisten = dispose;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [props.onMessage, props.recording.id, refreshVersions, refreshWorkspace, selectedDocumentId]);

  const selectedDocument = workspace?.documents.find(
    (document) => document.id === selectedDocumentId,
  ) ?? null;
  const selectedVersion = versions.find((version) => version.id === selectedVersionId) ?? null;
  const unusedTemplates = useMemo(() => {
    if (!workspace) return [];
    const used = new Set(workspace.documents.map((document) => document.templateId));
    return workspace.templates.filter((template) => !template.archived && !used.has(template.id));
  }, [workspace]);
  const availableProviders = useMemo(
    () => props.providers.filter(llmProviderReady),
    [props.providers],
  );
  const canCreateDocument = unusedTemplates.length > 0 && availableProviders.length > 0;
  const canReviseVersion = Boolean(
    availableProviders.length
    && selectedVersion
    && selectedVersion.status === "completed"
    && selectedVersion.fileState !== "missing",
  );
  const speakerLabelsAvailable = hasSpeakerLabels(props.transcript);
  const provider = dialog
    ? availableProviders.find((item) => item.id === dialog.providerId) ?? null
    : null;
  const generationRequest = useMemo<AiGenerationRequest | null>(() => dialog ? ({
    recordingId: props.recording.id,
    mode: dialog.mode,
    documentId: dialog.document?.id ?? null,
    templateId: dialog.mode === "create" ? dialog.templateId : null,
    title: dialog.title,
    meetingContext: dialog.meetingContext,
    documentRequirements: dialog.documentRequirements,
    runRequest: dialog.runRequest,
    providerId: dialog.providerId,
    modelId: dialog.modelId,
    sourceVersionId: dialog.mode === "revise" ? dialog.sourceVersion?.id ?? null : null,
  }) : null, [dialog, props.recording.id]);

  useEffect(() => {
    if (
      !generationRequest
      || (!generationRequest.templateId && !generationRequest.documentId)
    ) {
      setEstimatedTokens(null);
      setEstimateError(null);
      setEstimating(false);
      return;
    }
    let active = true;
    setEstimating(true);
    setEstimateError(null);
    const timer = window.setTimeout(() => {
      void api.estimateAiGenerationTokens(generationRequest)
        .then((estimate) => {
          if (active) setEstimatedTokens(estimate);
        })
        .catch((error) => {
          if (active) {
            setEstimatedTokens(null);
            setEstimateError(String(error));
          }
        })
        .finally(() => {
          if (active) setEstimating(false);
        });
    }, 180);
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [generationRequest]);

  const openDialog = (
    mode: AiGenerationMode,
    document: AiDocument | null,
    sourceVersion: AiDocumentVersion | null = null,
  ) => {
    const templateId = document?.templateId
      ?? unusedTemplates.find((template) => !isSpeakerTemplate(template) || speakerLabelsAvailable)?.id
      ?? unusedTemplates[0]?.id
      ?? "";
    const defaultProvider = availableProviders.find((item) => item.id === props.activeProviderId)
      ?? availableProviders[0]
      ?? null;
    setDialog({
      mode,
      document,
      sourceVersion,
      templateId,
      title: document?.title ?? workspace?.templates.find((item) => item.id === templateId)?.name ?? "",
      meetingContext: workspace?.profile.meetingContext ?? "",
      documentRequirements: document?.requirements ?? "",
      runRequest: "",
      providerId: defaultProvider?.id ?? "",
      modelId: defaultProvider?.modelId ?? "",
    });
  };

  const submitGeneration = async () => {
    if (!dialog || !generationRequest || !dialog.providerId || !dialog.templateId) return;
    if (dialog.mode === "revise" && !dialog.runRequest.trim()) {
      props.onMessage("error", "请填写希望如何修改这个版本");
      return;
    }
    setSubmitting(true);
    try {
      const version = await api.generateAiDocument(generationRequest);
      setDialog(null);
      const next = await refreshWorkspace();
      const document = next.documents.find((item) => item.id === version.documentId);
      if (document) setSelectedDocumentId(document.id);
      await refreshVersions(version.documentId);
    } catch (error) {
      props.onMessage("error", String(error));
    } finally {
      setSubmitting(false);
    }
  };

  const relink = async () => {
    if (!selectedVersion) return;
    const selected = await open({
      title: "重新关联 AI Markdown",
      multiple: false,
      directory: false,
      filters: [{ name: "Markdown", extensions: ["md"] }],
    });
    if (typeof selected !== "string") return;
    try {
      await api.relinkAiDocumentVersion(selectedVersion.id, selected);
      await refreshVersions(selectedVersion.documentId);
      props.onMessage("success", "Markdown 文件已重新关联");
    } catch (error) {
      props.onMessage("error", String(error));
    }
  };

  const findMoved = async () => {
    if (!selectedVersion || !workspace) return;
    try {
      await api.findAiDocumentVersion(selectedVersion.id, workspace.profile.workspacePath);
      await refreshVersions(selectedVersion.documentId);
      props.onMessage("success", "已找到并重新关联 Markdown 文件");
    } catch (error) {
      props.onMessage("error", String(error));
    }
  };

  if (loading) {
    return <div className="ai-documents-loading"><LoaderCircle className="spin" />读取 AI 文档…</div>;
  }

  if (props.transcript?.status !== "completed" || !props.transcript.text.trim()) {
    return (
      <div className="ai-documents-empty">
        <FileText size={30} />
        <h3>需要先完成文字转写</h3>
        <p>AI 文档只使用当前已经完成的转写内容，不会自动发起转写。</p>
      </div>
    );
  }

  return (
    <div className="ai-documents-layout">
      <aside className="ai-document-list">
        <div className="ai-document-list-header">
          <div><strong>AI 文档</strong><small>{workspace?.documents.length ?? 0} 个场景</small></div>
          <AppTooltip
            content={unusedTemplates.length ? "生成新文档" : "所有模板都已生成"}
            wrapDisabled={!canCreateDocument}
          >
            <button
              className="icon-button"
              aria-label="生成新文档"
              disabled={!canCreateDocument}
              onClick={() => openDialog("create", null)}
            >
              <Plus size={16} />
            </button>
          </AppTooltip>
        </div>
        {!availableProviders.length && (
          <div className="ai-inline-warning"><AlertCircle size={15} />请先在设置中添加可用的 LLM Provider；OpenAI 官方服务需要 API Key。</div>
        )}
        {workspace?.documents.length ? workspace.documents.map((document) => (
          <button
            key={document.id}
            className={`ai-document-item ${document.id === selectedDocumentId ? "active" : ""}`}
            onClick={() => setSelectedDocumentId(document.id)}
          >
            <FileText size={17} />
            <span><strong>{document.title}</strong><small>{document.templateName}</small></span>
          </button>
        )) : (
          <div className="ai-document-list-empty">
            <Sparkles size={20} />
            <p>选择模板生成第一份 Markdown 文档。</p>
            <button
              className="button primary"
              disabled={!unusedTemplates.length || !availableProviders.length}
              onClick={() => openDialog("create", null)}
            >
              <Plus size={15} />生成文档
            </button>
          </div>
        )}
      </aside>

      <section className="ai-document-preview">
        {!selectedDocument ? (
          <div className="ai-documents-empty"><WandSparkles size={28} /><p>选择或生成一份 AI 文档。</p></div>
        ) : (
          <>
            <header className="ai-document-preview-header">
              <div>
                <strong>{selectedDocument.title}</strong>
                <small>{selectedDocument.templateName}</small>
              </div>
              <div className="ai-version-controls">
                <select
                  aria-label="AI 文档版本"
                  value={selectedVersionId ?? ""}
                  onChange={(event) => setSelectedVersionId(event.target.value || null)}
                >
                  {versions.map((version) => (
                    <option key={version.id} value={version.id}>{formatVersionLabel(version)}</option>
                  ))}
                </select>
                <AppTooltip
                  content="不参考当前版本，使用当前转写创建全新版本；已有版本不会被覆盖"
                  wrapDisabled={!availableProviders.length}
                >
                  <button
                    className="button secondary compact"
                    disabled={!availableProviders.length}
                    onClick={() => openDialog("regenerate", selectedDocument)}
                  >
                    <RotateCcw size={14} />重新生成
                  </button>
                </AppTooltip>
                <AppTooltip
                  content="以当前所选版本为基础创建修改后的新版本；原版本不会被覆盖"
                  wrapDisabled={!canReviseVersion}
                >
                  <button
                    className="button secondary compact"
                    disabled={!canReviseVersion}
                    onClick={() => openDialog("revise", selectedDocument, selectedVersion)}
                  >
                    <WandSparkles size={14} />AI修改
                  </button>
                </AppTooltip>
              </div>
            </header>

            {selectedVersion?.status === "generating" || selectedVersion?.status === "queued" ? (
              <div className="ai-generation-progress">
                <LoaderCircle className="spin" />
                <span>{statusLabels[selectedVersion.status]}，完成后会生成新的 Markdown 文件。</span>
                <button className="button secondary" onClick={() => {
                  void api.cancelAiGeneration(selectedVersion.id)
                    .then(() => props.onMessage("success", "已请求取消；当前网络请求返回后将停止提交文件"))
                    .catch((error) => props.onMessage("error", String(error)));
                }}>
                  <Square size={12} fill="currentColor" />取消
                </button>
              </div>
            ) : selectedVersion?.errorMessage ? (
              <div className="ai-inline-warning"><AlertCircle size={15} />{selectedVersion.errorMessage}</div>
            ) : null}

            {selectedVersion?.status === "completed" && selectedVersion.fileState === "missing" && (
              <div className="ai-missing-file">
                <AlertCircle size={18} />
                <div><strong>Markdown 文件已被移动或删除</strong><small>{selectedVersion.filePath}</small></div>
                <button className="button secondary" onClick={() => void findMoved()}><RefreshCw size={14} />自动查找</button>
                <button className="button secondary" onClick={() => void relink()}><Link2 size={14} />选择文件</button>
              </div>
            )}
            {selectedVersion?.fileState === "modified" && (
              <div className="ai-external-edit">文件已在外部修改；预览和“基于此版本修改”都会使用磁盘上的当前内容。</div>
            )}

            <div className="ai-preview-toolbar">
              <span>
                {selectedVersion
                  ? `${selectedVersion.providerName} · ${selectedVersion.modelId}`
                  : "尚未生成版本"}
              </span>
              {selectedVersion?.status === "completed" && selectedVersion.fileState !== "missing" && (
                <div>
                  <AppTooltip content="刷新预览"><button aria-label="刷新预览" onClick={() => setContentReloadKey((current) => current + 1)}><RefreshCw size={14} /></button></AppTooltip>
                  <AppTooltip content="复制 Markdown"><button aria-label="复制 Markdown" onClick={() => void api.copyAiDocumentVersion(selectedVersion.id).then(() => props.onMessage("success", "已复制 Markdown")).catch((error) => props.onMessage("error", String(error)))}><Clipboard size={14} /></button></AppTooltip>
                  <AppTooltip content="复制文件路径"><button aria-label="复制文件路径" onClick={() => void api.copyAiDocumentPath(selectedVersion.id).then(() => props.onMessage("success", "已复制文件路径")).catch((error) => props.onMessage("error", String(error)))}><Link2 size={14} /></button></AppTooltip>
                  <AppTooltip content="使用默认应用打开"><button aria-label="使用默认应用打开" onClick={() => void api.openAiDocumentVersion(selectedVersion.id).catch((error) => props.onMessage("error", String(error)))}><FileText size={14} /></button></AppTooltip>
                  <AppTooltip content="在资源管理器中显示"><button aria-label="在资源管理器中显示" onClick={() => void api.revealAiDocumentVersion(selectedVersion.id).catch((error) => props.onMessage("error", String(error)))}><FolderOpen size={14} /></button></AppTooltip>
                </div>
              )}
            </div>
            <div className="ai-markdown-body">
              {contentLoading ? (
                <div className="ai-documents-loading"><LoaderCircle className="spin" />读取 Markdown…</div>
              ) : content ? (
                <ReactMarkdown
                  remarkPlugins={[remarkGfm]}
                  components={{
                    img: ({ alt }) => <span className="ai-remote-image">[图片未自动加载：{alt || "无标题"}]</span>,
                  }}
                >
                  {content.markdown}
                </ReactMarkdown>
              ) : (
                <div className="ai-documents-empty"><FileText size={24} /><p>选择一个已完成版本查看内容。</p></div>
              )}
            </div>
          </>
        )}
      </section>

      {dialog && workspace && (
        <div className="modal-backdrop" role="presentation">
          <section className="ai-generation-dialog" role="dialog" aria-modal="true" aria-label="生成 AI 文档">
            <header>
              <div>
                <p className="eyebrow">AI MARKDOWN</p>
                <h3>{dialog.mode === "create" ? "生成 AI 文档" : dialog.mode === "revise" ? "基于所选版本创建新版" : "生成全新版本"}</h3>
                <small className="ai-version-creation-note">
                  每次生成都会创建新的 Markdown 版本，不会覆盖已有版本或文件。
                </small>
              </div>
              <AppTooltip content="关闭"><button className="icon-button" aria-label="关闭生成窗口" onClick={() => setDialog(null)}>×</button></AppTooltip>
            </header>
            {dialog.mode === "create" && (
              <label>
                <span>场景模板</span>
                <select value={dialog.templateId} onChange={(event) => {
                  const template = workspace.templates.find((item) => item.id === event.target.value);
                  setDialog({ ...dialog, templateId: event.target.value, title: template?.name ?? dialog.title });
                }}>
                  {unusedTemplates.map((template) => (
                    <option
                      key={template.id}
                      value={template.id}
                      disabled={isSpeakerTemplate(template) && !speakerLabelsAvailable}
                    >
                      {template.name}{isSpeakerTemplate(template) && !speakerLabelsAvailable ? "（需要说话人标签）" : ""}
                    </option>
                  ))}
                </select>
              </label>
            )}
            <label><span>文档标题</span><input value={dialog.title} onChange={(event) => setDialog({ ...dialog, title: event.target.value })} /></label>
            <label>
              <span>会议级上下文 <small>会保存到当前会议</small></span>
              <textarea rows={3} value={dialog.meetingContext} placeholder="项目背景、缩写、参会人角色、会议目标…" onChange={(event) => setDialog({ ...dialog, meetingContext: event.target.value })} />
            </label>
            <label>
              <span>文档要求 <small>会保存到这条文档版本链</small></span>
              <textarea rows={3} value={dialog.documentRequirements} placeholder="受众、用途、语言、语气、篇幅、重点…" onChange={(event) => setDialog({ ...dialog, documentRequirements: event.target.value })} />
            </label>
            <label>
              <span>{dialog.mode === "revise" ? "修改意见" : "本次附加要求"}</span>
              <textarea rows={3} value={dialog.runRequest} placeholder={dialog.mode === "revise" ? "说明需要补充、删除或调整的内容…" : "仅对本次生成生效，可留空"} onChange={(event) => setDialog({ ...dialog, runRequest: event.target.value })} />
            </label>
            <div className="ai-provider-row">
              <label><span>Provider</span><select value={dialog.providerId} onChange={(event) => {
                const next = availableProviders.find((item) => item.id === event.target.value);
                setDialog({ ...dialog, providerId: event.target.value, modelId: next?.modelId ?? "" });
              }}>{availableProviders.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}</select></label>
              <label><span>模型</span><input value={dialog.modelId} onChange={(event) => setDialog({ ...dialog, modelId: event.target.value })} /></label>
            </div>
            <div className={`ai-token-estimate ${(
              provider
              && estimatedTokens !== null
              && estimatedTokens > provider.inputTokenBudget
            ) || estimateError ? "over" : ""}`}>
              {estimating
                ? "正在本地精确估算输入 tokens…"
                : estimateError
                  ? estimateError
                  : estimatedTokens === null
                    ? "等待估算输入 tokens"
                    : `预计输入约 ${estimatedTokens.toLocaleString()} tokens${provider ? ` / 上限 ${provider.inputTokenBudget.toLocaleString()}` : ""}`}
            </div>
            <footer>
              <button className="button secondary" disabled={submitting} onClick={() => setDialog(null)}>取消</button>
              <button
                className="button primary"
                disabled={
                  submitting
                  || estimating
                  || Boolean(estimateError)
                  || !dialog.title.trim()
                  || !dialog.providerId
                  || Boolean(
                    provider
                    && estimatedTokens !== null
                    && estimatedTokens > provider.inputTokenBudget,
                  )
                  || Boolean(
                    workspace.templates.some(
                      (template) => template.id === dialog.templateId
                        && isSpeakerTemplate(template)
                        && !speakerLabelsAvailable,
                    ),
                  )
                }
                onClick={() => void submitGeneration()}
              >
                {submitting ? <LoaderCircle className="spin" size={15} /> : <Sparkles size={15} />}
                {submitting ? "正在加入队列…" : "生成新版本"}
              </button>
            </footer>
          </section>
        </div>
      )}
    </div>
  );
}
