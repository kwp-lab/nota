import { open } from "@tauri-apps/plugin-dialog";
import {
  AlertCircle,
  Clipboard,
  FileText,
  Link2,
  LoaderCircle,
  Plus,
  RefreshCw,
  Sparkles,
  Square,
  WandSparkles,
  X,
} from "lucide-react";
import { type KeyboardEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AiDocumentToolbar } from "./AiDocumentToolbar";
import { AiDocumentReader } from "./AiDocumentReader";
import { api, type UnlistenFn } from "../api";
import { estimateAiRequestInputTokens } from "../ai-token-estimate";
import { llmProviderReady } from "../llm";
import { AiGenerationDetailsDrawer } from "./AiGenerationDetailsDrawer";
import { AppTooltip } from "./AppTooltip";
import { JsonTreeView } from "./JsonTreeView";
import { useBackdropDismiss } from "./useBackdropDismiss";
import type {
  AiDocument,
  AiDocumentContent,
  AiDocumentVersion,
  AiGenerationDetails,
  AiGenerationDraftRequest,
  AiGenerationMode,
  AiGenerationRequestPreview,
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

const tokenFormatter = new Intl.NumberFormat("zh-CN");

const formatTokenUsage = (value: number | null) =>
  value === null ? "Provider 未返回" : `${tokenFormatter.format(value)} tokens`;

const isJsonContainer = (value: unknown): value is object | unknown[] =>
  value !== null && typeof value === "object";

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
  const closeGenerationDialog = () => { if (!submitting) setDialog(null); };
  const generationBackdrop = useBackdropDismiss(closeGenerationDialog);
  const [contentReloadKey, setContentReloadKey] = useState(0);
  const [previewTab, setPreviewTab] = useState<"document" | "details">("document");
  const [generationDetailTab, setGenerationDetailTab] = useState<"request" | "response">("request");
  const [generationDetails, setGenerationDetails] = useState<AiGenerationDetails | null>(null);
  const [generationDetailsLoading, setGenerationDetailsLoading] = useState(false);
  const [generationDetailsError, setGenerationDetailsError] = useState<string | null>(null);
  const [dialogTab, setDialogTab] = useState<"settings" | "request">("settings");
  const [requestPreview, setRequestPreview] = useState<AiGenerationRequestPreview | null>(null);
  const [estimatedTokens, setEstimatedTokens] = useState<number | null>(null);
  const [estimateError, setEstimateError] = useState<string | null>(null);
  const [estimating, setEstimating] = useState(false);
  const recordingIdRef = useRef(props.recording.id);
  const selectedDocumentIdRef = useRef(selectedDocumentId);
  const contentRequestRef = useRef(0);
  const generationDetailsRequestRef = useRef(0);
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
    setPreviewTab("document");
    setGenerationDetailTab("request");
    setGenerationDetails(null);
    setGenerationDetailsError(null);
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
    const requestId = ++generationDetailsRequestRef.current;
    const version = versions.find((item) => item.id === selectedVersionId);
    if (previewTab !== "details" || !version) {
      setGenerationDetailsLoading(false);
      if (!version) {
        setGenerationDetails(null);
        setGenerationDetailsError(null);
      }
      return;
    }
    setGenerationDetails(null);
    setGenerationDetailsError(null);
    setGenerationDetailsLoading(true);
    void api
      .readAiGenerationDetails(version.id)
      .then((next) => {
        if (
          generationDetailsRequestRef.current === requestId
          && next.versionId === version.id
        ) {
          setGenerationDetails(next);
        }
      })
      .catch((error) => {
        if (generationDetailsRequestRef.current === requestId) {
          setGenerationDetailsError(String(error));
        }
      })
      .finally(() => {
        if (generationDetailsRequestRef.current === requestId) {
          setGenerationDetailsLoading(false);
        }
      });
    return () => {
      if (generationDetailsRequestRef.current === requestId) {
        generationDetailsRequestRef.current += 1;
      }
    };
  }, [previewTab, selectedVersionId, versions]);

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
  const selectedGenerationJson = generationDetailTab === "request"
    ? generationDetails?.requestBody ?? null
    : generationDetails?.responseBody ?? null;
  const totalTokens = selectedVersion?.inputTokens !== null
    && selectedVersion?.inputTokens !== undefined
    && selectedVersion.outputTokens !== null
    ? selectedVersion.inputTokens + selectedVersion.outputTokens
    : null;
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
  const generationDraft = useMemo<AiGenerationDraftRequest | null>(() => dialog ? ({
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
  const requestBodyJson = useMemo(
    () => requestPreview ? JSON.stringify(requestPreview.requestBody, null, 2) : "",
    [requestPreview],
  );
  const tokenEstimateOver = Boolean(
    (provider && estimatedTokens !== null && estimatedTokens > provider.inputTokenBudget)
    || estimateError,
  );
  const tokenEstimateLabel = estimating
    ? "正在估算输入 tokens…"
    : estimateError
      ? estimateError
      : estimatedTokens === null
        ? "等待估算输入 tokens"
        : `预计输入约 ${estimatedTokens.toLocaleString()} tokens${provider ? ` / 上限 ${provider.inputTokenBudget.toLocaleString()}` : ""}`;

  useEffect(() => {
    if (
      !generationDraft
      || (!generationDraft.templateId && !generationDraft.documentId)
    ) {
      setRequestPreview(null);
      setEstimatedTokens(null);
      setEstimateError(null);
      setEstimating(false);
      return;
    }
    let active = true;
    setEstimating(true);
    setEstimateError(null);
    setRequestPreview(null);
    setEstimatedTokens(null);
    const timer = window.setTimeout(() => {
      void api.previewAiGenerationRequest(generationDraft)
        .then((preview) => {
          if (!active) return;
          setRequestPreview(preview);
          setEstimatedTokens(estimateAiRequestInputTokens(preview));
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
  }, [generationDraft]);

  const openDialog = (
    mode: AiGenerationMode,
    document: AiDocument | null,
    sourceVersion: AiDocumentVersion | null = null,
  ) => {
    setDialogTab("settings");
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

  const handleDialogTabKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    const nextTab = dialogTab === "settings" ? "request" : "settings";
    setDialogTab(nextTab);
    window.requestAnimationFrame(() => {
      window.document.getElementById(`ai-generation-${nextTab}-tab`)?.focus();
    });
  };

  const handleGenerationDetailTabKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    const nextTab = generationDetailTab === "request" ? "response" : "request";
    setGenerationDetailTab(nextTab);
    window.requestAnimationFrame(() => {
      window.document.getElementById(`ai-generation-detail-${nextTab}-tab`)?.focus();
    });
  };

  const copyGenerationJson = async () => {
    if (!selectedGenerationJson) return;
    try {
      await api.copyAiGenerationJson(JSON.stringify(selectedGenerationJson, null, 2));
      props.onMessage(
        "success",
        generationDetailTab === "request" ? "请求 JSON 已复制" : "响应 JSON 已复制",
      );
    } catch (error) {
      props.onMessage("error", String(error));
    }
  };

  const submitGeneration = async () => {
    if (
      !dialog
      || !generationDraft
      || estimatedTokens === null
      || !dialog.providerId
      || !dialog.templateId
    ) return;
    if (dialog.mode === "revise" && !dialog.runRequest.trim()) {
      props.onMessage("error", "请填写希望如何修改这个版本");
      return;
    }
    setSubmitting(true);
    try {
      const version = await api.generateAiDocument({
        ...generationDraft,
        estimatedInputTokens: estimatedTokens,
      });
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

  const copyRequestBody = async () => {
    if (!requestBodyJson) return;
    try {
      await api.copyAiRequestBody(requestBodyJson);
      props.onMessage("success", "AI Request Body 已复制");
    } catch (error) {
      props.onMessage("error", String(error));
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
      <AiDocumentToolbar
        model={{
          documents: workspace?.documents ?? [], versions: versions.map((version) => ({ id: version.id, label: formatVersionLabel(version) })),
          documentId: selectedDocumentId, versionId: selectedVersionId,
          canCreate: canCreateDocument, canRegenerate: availableProviders.length > 0, canRevise: canReviseVersion,
          canRead: selectedVersion?.status === "completed" && selectedVersion.fileState !== "missing",
          createHint: !availableProviders.length ? "请先配置可用的 LLM Provider" : unusedTemplates.length ? "生成新文档" : "所有模板都已生成",
        }}
        actions={{
          selectDocument: setSelectedDocumentId, selectVersion: setSelectedVersionId,
          create: () => openDialog("create", null),
          regenerate: () => { if (selectedDocument) openDialog("regenerate", selectedDocument); },
          revise: () => { if (selectedDocument) openDialog("revise", selectedDocument, selectedVersion); },
          details: () => setPreviewTab("details"),
          refresh: () => setContentReloadKey((current) => current + 1),
          copy: () => { if (selectedVersion) void api.copyAiDocumentVersion(selectedVersion.id).then(() => props.onMessage("success", "已复制 Markdown")).catch((error) => props.onMessage("error", String(error))); },
          copyPath: () => { if (selectedVersion) void api.copyAiDocumentPath(selectedVersion.id).then(() => props.onMessage("success", "已复制文件路径")).catch((error) => props.onMessage("error", String(error))); },
          open: () => { if (selectedVersion) void api.openAiDocumentVersion(selectedVersion.id).catch((error) => props.onMessage("error", String(error))); },
          reveal: () => { if (selectedVersion) void api.revealAiDocumentVersion(selectedVersion.id).catch((error) => props.onMessage("error", String(error))); },
        }}
      />
      {!availableProviders.length && <div className="ai-inline-warning"><AlertCircle size={15} />请先在设置中添加可用的 LLM Provider；OpenAI 官方服务需要 API Key。</div>}
      <section className="ai-document-preview">
        {!selectedDocument ? (
          <div className="ai-documents-empty">
            <WandSparkles size={28} /><p>选择模板生成第一份 Markdown 文档。</p>
            <button className="button primary" disabled={!canCreateDocument} onClick={() => openDialog("create", null)}><Plus size={15} />生成文档</button>
          </div>
        ) : (
          <>
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

            <AiDocumentReader versionId={selectedVersionId} loading={contentLoading} content={content} />
            {previewTab === "details" && <AiGenerationDetailsDrawer onClose={() => setPreviewTab("document")}>
              <div
                id="ai-document-details-panel"
                className="ai-generation-details"
                role="region"
                aria-label="生成详情内容"
              >
                {selectedVersion ? (
                  <>
                    <dl className="ai-generation-summary">
                      <div><dt>Provider</dt><dd>{selectedVersion.providerName}</dd></div>
                      <div><dt>API 类型</dt><dd>{selectedVersion.providerKind === "openAi" ? "Responses API" : "Chat Completions"}</dd></div>
                      <div><dt>模型</dt><dd>{selectedVersion.modelId}</dd></div>
                      <div><dt>状态</dt><dd>{statusLabels[selectedVersion.status]}</dd></div>
                      <div><dt>预计输入</dt><dd>{formatTokenUsage(selectedVersion.estimatedInputTokens)}</dd></div>
                      <div><dt>实际输入</dt><dd className={selectedVersion.inputTokens === null ? "muted" : ""}>{formatTokenUsage(selectedVersion.inputTokens)}</dd></div>
                      <div><dt>实际输出</dt><dd className={selectedVersion.outputTokens === null ? "muted" : ""}>{formatTokenUsage(selectedVersion.outputTokens)}</dd></div>
                      <div><dt>合计用量</dt><dd className={totalTokens === null ? "muted" : ""}>{formatTokenUsage(totalTokens)}</dd></div>
                    </dl>
                    <div
                      className="app-tab-bar ai-generation-detail-tabs"
                      role="tablist"
                      aria-label="生成详情 JSON"
                      onKeyDown={handleGenerationDetailTabKeyDown}
                    >
                      <button
                        id="ai-generation-detail-request-tab"
                        type="button"
                        role="tab"
                        aria-selected={generationDetailTab === "request"}
                        aria-controls="ai-generation-detail-json-panel"
                        tabIndex={generationDetailTab === "request" ? 0 : -1}
                        className={generationDetailTab === "request" ? "active" : ""}
                        onClick={() => setGenerationDetailTab("request")}
                      >
                        请求 JSON
                      </button>
                      <button
                        id="ai-generation-detail-response-tab"
                        type="button"
                        role="tab"
                        aria-selected={generationDetailTab === "response"}
                        aria-controls="ai-generation-detail-json-panel"
                        tabIndex={generationDetailTab === "response" ? 0 : -1}
                        className={generationDetailTab === "response" ? "active" : ""}
                        onClick={() => setGenerationDetailTab("response")}
                      >
                        响应 JSON
                      </button>
                    </div>
                    <section
                      id="ai-generation-detail-json-panel"
                      className="ai-generation-json-panel"
                      role="tabpanel"
                      aria-labelledby={`ai-generation-detail-${generationDetailTab}-tab`}
                    >
                      <header className="ai-generation-json-header">
                        <div>
                          <strong>{generationDetailTab === "request" ? "请求 JSON" : "响应 JSON"}</strong>
                          <small>
                            {generationDetailTab === "request"
                              ? "实际发送给模型的请求体，不包含 API Key 或 Authorization。"
                              : "Provider 返回并由 Nota 解析的原始 JSON。"}
                          </small>
                        </div>
                        <button
                          type="button"
                          className="button secondary compact"
                          disabled={!selectedGenerationJson}
                          onClick={() => void copyGenerationJson()}
                        >
                          <Clipboard size={14} />复制 JSON
                        </button>
                      </header>
                      {generationDetailsLoading ? (
                        <div className="ai-generation-detail-state"><LoaderCircle className="spin" size={18} />正在读取生成详情…</div>
                      ) : generationDetailsError ? (
                        <div className="ai-generation-detail-state"><AlertCircle size={18} /><p>{generationDetailsError}</p></div>
                      ) : isJsonContainer(selectedGenerationJson) ? (
                        <div className="ai-generation-json-scroll">
                          <JsonTreeView
                            data={selectedGenerationJson}
                            ariaLabel={generationDetailTab === "request" ? "AI 请求 JSON" : "AI 响应 JSON"}
                          />
                        </div>
                      ) : (
                        <div className="ai-generation-detail-state">
                          <FileText size={22} />
                          <p>
                            {generationDetailTab === "request"
                              ? "该版本生成时尚未记录原始请求 JSON。"
                              : selectedVersion.status === "completed"
                                ? "该版本生成时尚未记录原始响应 JSON。"
                                : "该版本未成功完成，因此没有可显示的响应 JSON。"}
                          </p>
                        </div>
                      )}
                    </section>
                  </>
                ) : (
                  <div className="ai-generation-detail-state"><FileText size={24} /><p>选择一个版本查看生成详情。</p></div>
                )}
              </div>
            </AiGenerationDetailsDrawer>}
          </>
        )}
      </section>

      {dialog && workspace && (
        <div className="modal-backdrop" role="presentation" {...generationBackdrop}>
          <section className="ai-generation-dialog" role="dialog" aria-modal="true" aria-label="生成 AI 文档">
            <header>
              <div>
                <p className="eyebrow">AI MARKDOWN</p>
                <h3>{dialog.mode === "create" ? "生成 AI 文档" : dialog.mode === "revise" ? "基于所选版本创建新版" : "生成全新版本"}</h3>
                <small className="ai-version-creation-note">
                  每次生成都会创建新的 Markdown 版本，不会覆盖已有版本或文件。
                </small>
              </div>
              <AppTooltip content="关闭"><button className="icon-button" aria-label="关闭生成窗口" disabled={submitting} onClick={closeGenerationDialog}><X size={16} /></button></AppTooltip>
            </header>
            <div
              className="app-tab-bar ai-generation-tabs"
              role="tablist"
              aria-label="生成 AI 文档"
              onKeyDown={handleDialogTabKeyDown}
            >
              <button
                id="ai-generation-settings-tab"
                type="button"
                role="tab"
                aria-selected={dialogTab === "settings"}
                aria-controls="ai-generation-settings-panel"
                tabIndex={dialogTab === "settings" ? 0 : -1}
                className={dialogTab === "settings" ? "active" : ""}
                onClick={() => setDialogTab("settings")}
              >
                生成设置
              </button>
              <button
                id="ai-generation-request-tab"
                type="button"
                role="tab"
                aria-selected={dialogTab === "request"}
                aria-controls="ai-generation-request-panel"
                tabIndex={dialogTab === "request" ? 0 : -1}
                className={dialogTab === "request" ? "active" : ""}
                onClick={() => setDialogTab("request")}
              >
                请求预览
              </button>
            </div>
            {dialogTab === "settings" ? (
              <div
                id="ai-generation-settings-panel"
                className="ai-generation-tab-panel"
                role="tabpanel"
                aria-labelledby="ai-generation-settings-tab"
              >
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
                <div className={`ai-token-estimate ${tokenEstimateOver ? "over" : ""}`}>
                  {tokenEstimateLabel}
                </div>
              </div>
            ) : (
              <div
                id="ai-generation-request-panel"
                className="ai-generation-tab-panel ai-request-preview"
                role="tabpanel"
                aria-labelledby="ai-generation-request-tab"
              >
                <div className="ai-request-preview-header">
                  <div>
                    <strong>Request Body</strong>
                    <small>
                      {(requestPreview?.providerKind ?? provider?.kind) === "openAi"
                        ? "Responses API"
                        : "Chat Completions"}
                    </small>
                  </div>
                  <button
                    type="button"
                    className="button secondary compact"
                    disabled={!requestBodyJson}
                    onClick={() => void copyRequestBody()}
                  >
                    <Clipboard size={14} />复制请求体
                  </button>
                </div>
                {estimating ? (
                  <div className="ai-request-preview-state"><LoaderCircle className="spin" size={18} />正在生成请求预览…</div>
                ) : estimateError ? (
                  <div className="ai-request-preview-state error"><AlertCircle size={18} />{estimateError}</div>
                ) : requestBodyJson ? (
                  <pre className="ai-request-json" aria-label="AI 请求 Request Body"><code>{requestBodyJson}</code></pre>
                ) : (
                  <div className="ai-request-preview-state">等待生成请求预览</div>
                )}
                <div className={`ai-token-estimate ai-request-token-estimate ${tokenEstimateOver ? "over" : ""}`}>
                  <span>{tokenEstimateLabel}</span>
                  {!estimateError && <small>由 tokenx 本地估算，实际用量以 Provider 返回结果为准。</small>}
                </div>
              </div>
            )}
            <footer>
              <button className="button secondary" disabled={submitting} onClick={closeGenerationDialog}>取消</button>
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
