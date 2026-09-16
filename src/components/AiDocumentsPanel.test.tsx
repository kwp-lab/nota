import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { save } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import { AiDocumentsPanel } from "./AiDocumentsPanel";
import type {
  AiDocumentContent,
  AiDocumentVersion,
  AiGenerationDetails,
  AiTemplate,
  AiWorkspace,
  LlmProvider,
  RecordingItem,
  TranscriptDocument,
} from "../types";

const testState = vi.hoisted(() => ({
  workspace: null as AiWorkspace | null,
  versions: [] as AiDocumentVersion[],
  contents: new Map<string, AiDocumentContent>(),
  details: new Map<string, AiGenerationDetails>(),
  readDocument: null as null | ((id: string) => Promise<AiDocumentContent | undefined>),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(async () => null),
  save: vi.fn(async () => null),
}));

vi.mock("../api", () => ({
  api: {
    getAiWorkspace: vi.fn(async () => testState.workspace),
    previewAiGenerationRequest: vi.fn(async () => ({
      providerKind: "openAi",
      requestBody: {
        model: "test-model",
        instructions: "Follow the policy.",
        input: "Summarize the meeting.",
        max_output_tokens: 4_096,
        store: false,
      },
    })),
    copyAiRequestBody: vi.fn(),
    listAiDocumentVersions: vi.fn(async () => testState.versions),
    readAiDocumentVersion: vi.fn(async (id: string) =>
      testState.readDocument ? testState.readDocument(id) : testState.contents.get(id)),
    readAiGenerationDetails: vi.fn(async (id: string) => testState.details.get(id) ?? ({
      versionId: id,
      requestBody: null,
      responseBody: null,
    })),
    copyAiGenerationJson: vi.fn(),
    onAiStatus: vi.fn(async () => () => undefined),
    generateAiDocument: vi.fn(),
    cancelAiGeneration: vi.fn(),
    relinkAiDocumentVersion: vi.fn(),
    findAiDocumentVersion: vi.fn(),
    copyAiDocumentVersion: vi.fn(),
    exportAiDocumentPdf: vi.fn(),
    revealAiDocumentPdf: vi.fn(async () => undefined),
    copyAiDocumentPath: vi.fn(),
    openAiDocumentVersion: vi.fn(),
    revealAiDocumentVersion: vi.fn(),
  },
}));

const recording: RecordingItem = {
  id: "meeting-1",
  title: "Weekly meeting",
  path: "C:\\Recordings\\meeting.ogg",
  createdAt: "2026-08-09T00:00:00Z",
  durationMs: 60_000,
  sizeBytes: 1024,
  recovered: false,
  origin: "captured",
  sourceFileName: null,
  sourceFormat: null,
  importedAt: null,
  transcription: null,
};

const transcript: TranscriptDocument = {
  recordingId: recording.id,
  generation: 1,
  status: "completed",
  providerName: "Test ASR",
  providerKind: "funAsr",
  modelId: "test-asr-model",
  speakerCount: null,
  protocol: "nota_batch_v1",
  voiceprintAnalysisSupported: true,
  text: "Discussed the release.",
  segments: [],
  language: "en",
  speakerNames: {},
  speakerAssignments: {},
  completedChunks: 1,
  totalChunks: 1,
  errorMessage: null,
  updatedAt: "2026-08-09T00:01:00Z",
  completedAt: "2026-08-09T00:01:00Z",
};

const provider: LlmProvider = {
  id: "provider-1",
  name: "OpenAI",
  kind: "openAi",
  baseUrl: "https://api.openai.com/v1",
  modelId: "test-model",
  inputTokenBudget: 32_768,
  maxOutputTokens: 4_096,
  hasApiKey: true,
  createdAt: "2026-08-09T00:00:00Z",
  updatedAt: "2026-08-09T00:00:00Z",
};

const template = (id: string, builtinKey: string | null): AiTemplate => ({
  id,
  name: builtinKey === "speaker_summary" ? "按发言人总结" : "会议总结",
  description: "",
  builtinKey,
  taskInstructions: "Summarize.",
  outputRequirements: "Markdown",
  requiresSpeakerLabels: builtinKey === "speaker_summary" || builtinKey === "speaker_standup",
  revision: 1,
  archived: false,
  createdAt: "2026-08-09T00:00:00Z",
  updatedAt: "2026-08-09T00:00:00Z",
});

const version = (
  id: string,
  versionNumber: number,
  status: AiDocumentVersion["status"],
): AiDocumentVersion => ({
  id,
  documentId: "document-1",
  versionNumber,
  mode: versionNumber === 1 ? "create" : "regenerate",
  parentVersionId: null,
  status,
  filePath: status === "completed" ? `C:\\AI\\${id}.md` : null,
  fileState: status === "completed" ? "ready" : "pending",
  providerName: "OpenAI",
  providerKind: "openAi",
  modelId: "test-model",
  templateName: "会议总结",
  templateRevision: 1,
  transcriptionGeneration: 1,
  estimatedInputTokens: 100,
  inputTokens: status === "completed" ? 90 : null,
  outputTokens: status === "completed" ? 20 : null,
  errorMessage: status === "failed" ? "synthetic failure" : null,
  createdAt: `2026-08-09T00:0${versionNumber}:00Z`,
  completedAt: status === "queued" || status === "generating" ? null : "2026-08-09T00:10:00Z",
});

describe("AI documents panel", () => {
  it.each(["create", "regenerate", "revise"] as const)("preserves the full %s form and generation payload", async (mode) => {
    const summary = template("summary", "meeting_summary");
    const current = version("v1", 1, "completed");
    testState.workspace!.profile.meetingContext = "existing meeting context";
    testState.workspace!.templates = [summary, template("extra", null)];
    testState.workspace!.documents = [{
      id: "document-1", recordingId: recording.id, templateId: summary.id,
      title: "Existing summary", requirements: "existing document requirements", templateName: summary.name,
      templateBuiltinKey: summary.builtinKey, latestVersion: current, createdAt: "", updatedAt: "",
    }];
    testState.versions = [current];
    testState.contents.set("v1", { version: current, markdown: "# Existing summary" });
    vi.mocked(api.generateAiDocument).mockResolvedValue(current);
    const onMessage = vi.fn();
    const secondProvider = { ...provider, id: "provider-2", name: "Second provider", modelId: "second-model" };
    render(<AiDocumentsPanel recording={recording} transcript={transcript} providers={[provider, secondProvider]} activeProviderId={provider.id} onMessage={onMessage} />);
    await screen.findByRole("heading", { name: "Existing summary" });
    const openModeDialog = async () => {
      fireEvent.click(screen.getByRole("button", {
        name: mode === "revise" ? "AI 修改" : "生成 AI 文档或新版本",
      }));
      const nextDialog = await screen.findByRole("dialog");
      if (mode === "create") {
        fireEvent.change(within(nextDialog).getByLabelText("场景模板"), { target: { value: "extra" } });
      }
      return nextDialog;
    };
    let dialog = await openModeDialog();
    const backdrop = dialog.parentElement!;
    const close = within(dialog).getByRole("button", { name: "关闭生成窗口" });
    expect(close.querySelector("svg.lucide-x")).not.toBeNull();
    fireEvent.click(within(dialog).getByLabelText("文档标题"));
    expect(dialog).toBeInTheDocument();
    fireEvent(within(dialog).getByLabelText("文档标题"), new MouseEvent("pointerdown", { bubbles: true, button: 0 }));
    fireEvent.click(backdrop);
    expect(dialog).toBeInTheDocument();
    fireEvent(backdrop, new MouseEvent("pointerdown", { bubbles: true, button: 0 }));
    fireEvent.pointerCancel(backdrop);
    fireEvent.click(backdrop);
    expect(dialog).toBeInTheDocument();
    fireEvent(backdrop, new MouseEvent("pointerdown", { bubbles: true, button: 0 }));
    fireEvent.click(backdrop);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(api.generateAiDocument).not.toHaveBeenCalled();
    dialog = await openModeDialog();
    expect(within(dialog).getByRole("tab", { name: "生成设置" })).toBeInTheDocument();
    expect(within(dialog).getByRole("tab", { name: "请求预览" })).toBeInTheDocument();
    expect(within(dialog).queryByLabelText("场景模板") !== null).toBe(mode !== "revise");
    if (mode === "regenerate") {
      expect(within(dialog).getByLabelText("场景模板")).toHaveValue("summary");
      expect(within(dialog).getByRole("option", { name: "会议总结（创建新版本）" })).toBeInTheDocument();
      expect(within(dialog).getByText("该模板已有文档，本次将创建新版本，不会覆盖现有版本。")).toBeInTheDocument();
    }
    if (mode === "create") {
      expect(within(dialog).getByLabelText("场景模板")).toHaveValue("extra");
      expect(within(dialog).getByRole("option", { name: "会议总结（新建文档）" })).toBeInTheDocument();
      expect(within(dialog).queryByText("该模板已有文档，本次将创建新版本，不会覆盖现有版本。")).toBeNull();
    }
    expect(within(dialog).getByLabelText(/会议级上下文/)).toHaveValue("existing meeting context");
    expect(within(dialog).getByLabelText(/文档要求/)).toHaveValue(mode === "create" ? "" : "existing document requirements");
    if (mode === "revise") {
      await waitFor(() => expect(within(dialog).getByRole("button", { name: "生成新版本" })).toBeEnabled());
      fireEvent.click(within(dialog).getByRole("button", { name: "生成新版本" }));
      expect(api.generateAiDocument).not.toHaveBeenCalled();
      expect(onMessage).toHaveBeenCalledWith("error", "请填写希望如何修改这个版本");
    }
    fireEvent.change(within(dialog).getByLabelText("文档标题"), { target: { value: "Updated title" } });
    fireEvent.change(within(dialog).getByLabelText(/会议级上下文/), { target: { value: "Updated context" } });
    fireEvent.change(within(dialog).getByLabelText(/文档要求/), { target: { value: "Updated requirements" } });
    fireEvent.change(within(dialog).getByLabelText(mode === "revise" ? "修改意见" : "本次附加要求"), { target: { value: "Updated request" } });
    fireEvent.change(within(dialog).getByLabelText("Provider"), { target: { value: secondProvider.id } });
    expect(within(dialog).getByLabelText("模型")).toHaveValue("second-model");
    fireEvent.change(within(dialog).getByLabelText("模型"), { target: { value: "manual-model" } });
    fireEvent.click(within(dialog).getByRole("tab", { name: "请求预览" }));
    await waitFor(() => expect(within(dialog).getByRole("button", { name: "生成新版本" })).toBeEnabled());
    fireEvent.click(within(dialog).getByRole("button", { name: "生成新版本" }));
    await waitFor(() => expect(api.generateAiDocument).toHaveBeenCalledWith(expect.objectContaining({
      mode, recordingId: recording.id, documentId: mode === "create" ? null : "document-1",
      templateId: mode === "create" ? "extra" : null,
      sourceVersionId: mode === "revise" ? "v1" : null,
      title: "Updated title", meetingContext: "Updated context", documentRequirements: "Updated requirements",
      runRequest: "Updated request", providerId: secondProvider.id, modelId: "manual-model", estimatedInputTokens: expect.any(Number),
    })));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("blocks every dismissal path while submitting generation", async () => {
    testState.workspace!.templates = [template("summary", "meeting_summary")];
    let finish!: (value: AiDocumentVersion) => void;
    vi.mocked(api.generateAiDocument).mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
    render(<AiDocumentsPanel recording={recording} transcript={transcript} providers={[provider]} activeProviderId={provider.id} onMessage={vi.fn()} />);
    fireEvent.click(await screen.findByRole("button", { name: "生成 AI 文档或新版本" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "生成新版本" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "生成新版本" }));
    const dialog = screen.getByRole("dialog");
    expect(screen.getByRole("button", { name: "关闭生成窗口" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "取消" })).toBeDisabled();
    fireEvent(dialog.parentElement!, new MouseEvent("pointerdown", { bubbles: true, button: 0 }));
    fireEvent.click(dialog.parentElement!);
    expect(dialog).toBeInTheDocument();
    await act(async () => finish(version("pending", 1, "queued")));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("keeps token budget and preview failures blocking generation without losing the draft", async () => {
    testState.workspace!.templates = [template("summary", "meeting_summary")];
    render(<AiDocumentsPanel recording={recording} transcript={transcript} providers={[{ ...provider, inputTokenBudget: 1 }]} activeProviderId={provider.id} onMessage={vi.fn()} />);
    fireEvent.click(await screen.findByRole("button", { name: "生成 AI 文档或新版本" }));
    await waitFor(() => expect(document.querySelector(".ai-token-estimate.over")).not.toBeNull());
    expect(screen.getByRole("button", { name: "生成新版本" })).toBeDisabled();
    vi.mocked(api.previewAiGenerationRequest).mockRejectedValueOnce(new Error("Preview unavailable"));
    fireEvent.change(screen.getByLabelText("文档标题"), { target: { value: "Keep this draft" } });
    await screen.findByText(/Preview unavailable/);
    expect(screen.getByLabelText("文档标题")).toHaveValue("Keep this draft");
    expect(screen.getByRole("button", { name: "生成新版本" })).toBeDisabled();
    expect(api.generateAiDocument).not.toHaveBeenCalled();
  });
  beforeEach(() => {
    testState.versions = [];
    testState.contents.clear();
    testState.details.clear();
    testState.readDocument = null;
    testState.workspace = {
      profile: {
        recordingId: recording.id,
        workspacePath: "C:\\AI\\meeting-1",
        meetingContext: "",
        updatedAt: "",
      },
      documents: [],
      templates: [],
    };
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("selects the newest completed version and never loads remote Markdown images", async () => {
    const summaryTemplate = template("summary", "meeting_summary");
    testState.workspace = {
      ...testState.workspace!,
      templates: [summaryTemplate],
      documents: [{
        id: "document-1",
        recordingId: recording.id,
        templateId: summaryTemplate.id,
        title: "Meeting summary",
        requirements: "",
        templateName: summaryTemplate.name,
        templateBuiltinKey: summaryTemplate.builtinKey,
        latestVersion: version("v3", 3, "failed"),
        createdAt: "2026-08-09T00:00:00Z",
        updatedAt: "2026-08-09T00:03:00Z",
      }],
    };
    testState.versions = [
      version("v3", 3, "failed"),
      version("v2", 2, "completed"),
      version("v1", 1, "completed"),
    ];
    testState.contents.set("v2", {
      version: testState.versions[1],
      markdown: "# Current summary\n\n![chart](https://example.test/tracker.png)",
    });

    render(
      <AiDocumentsPanel
        recording={recording}
        transcript={transcript}
        providers={[provider]}
        activeProviderId={provider.id}
        onMessage={vi.fn()}
      />,
    );

    const selector = await screen.findByLabelText("AI 文档版本");
    await waitFor(() => expect(selector).toHaveValue("v2"));
    expect(await screen.findByRole("heading", { name: "Current summary" })).toBeInTheDocument();
    expect(screen.getByText("[图片未自动加载：chart]")).toBeInTheDocument();
    expect(document.querySelector("img")).toBeNull();
    expect(screen.getByRole("button", { name: "AI 修改" })).toHaveClass("icon-button");
    expect(screen.getByRole("button", { name: "AI 修改" })).not.toHaveTextContent("AI修改");
    expect(screen.getByRole("button", { name: "生成 AI 文档或新版本" })).toHaveClass("icon-button");
    const toolbarActions = document.querySelector(".ai-document-actions");
    expect(toolbarActions).not.toBeNull();
    expect(within(toolbarActions as HTMLElement).getAllByRole("button")[0]).toHaveAccessibleName("生成 AI 文档或新版本");
    fireEvent.click(screen.getByLabelText("更多文档操作"));
    expect(screen.queryByRole("button", { name: "重新生成" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "查看生成详情" }));
    expect(await screen.findByText("该版本生成时尚未记录原始请求 JSON。")).toBeInTheDocument();
  });

  it("exports the rendered Markdown with syntax highlighting and removes the temporary print root", async () => {
    const summaryTemplate = template("summary", "meeting_summary");
    const completedVersion = version("v1", 1, "completed");
    testState.workspace = {
      ...testState.workspace!,
      templates: [summaryTemplate],
      documents: [{
        id: "document-1",
        recordingId: recording.id,
        templateId: summaryTemplate.id,
        title: "Weekly: meeting?",
        requirements: "",
        templateName: summaryTemplate.name,
        templateBuiltinKey: summaryTemplate.builtinKey,
        latestVersion: completedVersion,
        createdAt: "2026-08-09T00:00:00Z",
        updatedAt: "2026-08-09T00:03:00Z",
      }],
    };
    testState.versions = [completedVersion];
    testState.contents.set("v1", {
      version: completedVersion,
      markdown: "# Current summary\n\n```js\nconst ready = true;\n```",
    });
    vi.mocked(save).mockResolvedValueOnce("C:\\Exports\\Weekly meeting.pdf");
    vi.mocked(api.exportAiDocumentPdf).mockImplementationOnce(async () => {
      const printRoot = document.querySelector(".ai-pdf-document");
      expect(printRoot).not.toBeNull();
      expect(printRoot).toHaveTextContent("Current summary");
      expect(printRoot?.querySelector(".hljs-keyword")).toHaveTextContent("const");
      expect(document.title).toBe("Current summary");
    });
    const onMessage = vi.fn();
    const originalTitle = document.title;

    render(
      <AiDocumentsPanel
        recording={recording}
        transcript={transcript}
        providers={[provider]}
        activeProviderId={provider.id}
        onMessage={onMessage}
      />,
    );

    await screen.findByRole("heading", { name: "Current summary" });
    fireEvent.click(screen.getByRole("button", { name: "导出 PDF" }));

    await waitFor(() => expect(api.exportAiDocumentPdf).toHaveBeenCalledWith(
      "v1",
      "C:\\Exports\\Weekly meeting.pdf",
    ));
    expect(save).toHaveBeenCalledWith({
      defaultPath: "Current summary.pdf",
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    expect(document.querySelector(".ai-pdf-document")).toBeNull();
    expect(document.title).toBe(originalTitle);
    expect(onMessage).toHaveBeenCalledWith(
      "success",
      "AI 文档已导出为 PDF",
      expect.objectContaining({
        durationMs: 8_000,
        action: expect.objectContaining({ label: "打开文件夹" }),
      }),
    );
    const successOptions = onMessage.mock.calls.find(([tone]) => tone === "success")?.[2];
    successOptions?.action.onClick();
    await waitFor(() => expect(api.revealAiDocumentPdf).toHaveBeenCalledWith(
      "C:\\Exports\\Weekly meeting.pdf",
    ));
  });

  it("shows persisted request and response JSON with normalized token usage", async () => {
    const summaryTemplate = template("summary", "meeting_summary");
    const completedVersion = version("v1", 1, "completed");
    testState.workspace = {
      ...testState.workspace!,
      templates: [summaryTemplate],
      documents: [{
        id: "document-1",
        recordingId: recording.id,
        templateId: summaryTemplate.id,
        title: "Meeting summary",
        requirements: "",
        templateName: summaryTemplate.name,
        templateBuiltinKey: summaryTemplate.builtinKey,
        latestVersion: completedVersion,
        createdAt: "2026-08-09T00:00:00Z",
        updatedAt: "2026-08-09T00:03:00Z",
      }],
    };
    testState.versions = [completedVersion];
    testState.contents.set("v1", {
      version: completedVersion,
      markdown: "# Current summary",
    });
    testState.details.set("v1", {
      versionId: "v1",
      requestBody: {
        model: "test-model",
        instructions: "Follow the policy.",
        input: "Summarize the meeting.",
      },
      responseBody: {
        id: "response-1",
        usage: { input_tokens: 90, output_tokens: 20 },
      },
    });

    render(
      <AiDocumentsPanel
        recording={recording}
        transcript={transcript}
        providers={[provider]}
        activeProviderId={provider.id}
        onMessage={vi.fn()}
      />,
    );

    const moreButton = await screen.findByLabelText("更多文档操作");
    expect(api.readAiGenerationDetails).not.toHaveBeenCalled();
    fireEvent.click(moreButton);
    const detailsButton = screen.getByRole("button", { name: "查看生成详情" });
    detailsButton.focus();
    fireEvent.click(detailsButton);
    expect(screen.getByRole("dialog", { name: "生成详情" })).toBeInTheDocument();
    expect(await screen.findByText("90 tokens")).toBeInTheDocument();
    expect(screen.getByText("20 tokens")).toBeInTheDocument();
    expect(screen.getByText("110 tokens")).toBeInTheDocument();
    const requestJson = await screen.findByLabelText("AI 请求 JSON");
    expect(requestJson).toHaveTextContent(/"model":.*"test-model"/);

    const requestTab = screen.getByRole("tab", { name: "请求 JSON" });
    fireEvent.keyDown(requestTab.parentElement!, { key: "ArrowRight" });
    expect(screen.getByRole("tab", { name: "响应 JSON" })).toHaveAttribute("aria-selected", "true");
    const responseJson = await screen.findByLabelText("AI 响应 JSON");
    fireEvent.click(within(responseJson).getByLabelText("展开 JSON 节点"));
    expect(responseJson).toHaveTextContent(/"output_tokens":.*20/);
    fireEvent.click(screen.getByRole("button", { name: "复制 JSON" }));
    await waitFor(() => expect(api.copyAiGenerationJson).toHaveBeenCalledWith(
      expect.stringContaining('"response-1"'),
    ));
    fireEvent.keyDown(screen.getByRole("button", { name: "关闭生成详情" }), { key: "Escape" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(moreButton).toHaveFocus();
  });

  it("skips an unavailable speaker template when opening a new document dialog", async () => {
    const speakerTemplate = template("speaker", "speaker_summary");
    const summaryTemplate = template("summary", "meeting_summary");
    testState.workspace = {
      ...testState.workspace!,
      templates: [speakerTemplate, summaryTemplate],
    };

    render(
      <AiDocumentsPanel
        recording={recording}
        transcript={transcript}
        providers={[provider]}
        activeProviderId={provider.id}
        onMessage={vi.fn()}
      />,
    );

    const generateButtons = await screen.findAllByRole("button", { name: "生成文档" });
    fireEvent.click(generateButtons.at(-1)!);
    const selector = await screen.findByLabelText("场景模板");
    expect(selector).toHaveValue(summaryTemplate.id);
    expect(screen.getByRole("option", { name: /按发言人总结/ })).toBeDisabled();
  });

  it("previews the exact request body and shares the tokenx estimate across tabs", async () => {
    const summaryTemplate = template("summary", "meeting_summary");
    testState.workspace = {
      ...testState.workspace!,
      templates: [summaryTemplate],
    };

    render(
      <AiDocumentsPanel
        recording={recording}
        transcript={transcript}
        providers={[provider]}
        activeProviderId={provider.id}
        onMessage={vi.fn()}
      />,
    );

    fireEvent.click((await screen.findAllByRole("button", { name: "生成文档" })).at(-1)!);
    const settingsTab = await screen.findByRole("tab", { name: "生成设置" });
    expect(settingsTab).toHaveAttribute("aria-selected", "true");
    expect(await screen.findByText(/预计输入约 .* tokens/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("tab", { name: "请求预览" }));
    const requestBody = await screen.findByLabelText("AI 请求 Request Body");
    expect(requestBody).toHaveTextContent('"instructions": "Follow the policy."');
    expect(requestBody).toHaveClass("ai-request-json");
    expect(screen.getByText(/由 tokenx 本地估算/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "复制请求体" }));
    await waitFor(() => expect(api.copyAiRequestBody).toHaveBeenCalledWith(
      expect.stringContaining('"input": "Summarize the meeting."'),
    ));
  });

  it("ignores a stale preview response after the selected version changes", async () => {
    const summaryTemplate = template("summary", "meeting_summary");
    testState.workspace = {
      ...testState.workspace!,
      templates: [summaryTemplate],
      documents: [{
        id: "document-1",
        recordingId: recording.id,
        templateId: summaryTemplate.id,
        title: "Meeting summary",
        requirements: "",
        templateName: summaryTemplate.name,
        templateBuiltinKey: summaryTemplate.builtinKey,
        latestVersion: version("v2", 2, "completed"),
        createdAt: "2026-08-09T00:00:00Z",
        updatedAt: "2026-08-09T00:02:00Z",
      }],
    };
    testState.versions = [
      version("v2", 2, "completed"),
      version("v1", 1, "completed"),
    ];
    const resolvers = new Map<string, (content: AiDocumentContent) => void>();
    testState.readDocument = (id) => new Promise((resolve) => resolvers.set(id, resolve));

    render(
      <AiDocumentsPanel
        recording={recording}
        transcript={transcript}
        providers={[provider]}
        activeProviderId={provider.id}
        onMessage={vi.fn()}
      />,
    );

    const selector = await screen.findByLabelText("AI 文档版本");
    await waitFor(() => expect(resolvers.has("v2")).toBe(true));
    fireEvent.change(selector, { target: { value: "v1" } });
    await waitFor(() => expect(resolvers.has("v1")).toBe(true));
    await act(async () => {
      resolvers.get("v1")!({
        version: testState.versions[1],
        markdown: "# Version one",
      });
    });
    expect(await screen.findByRole("heading", { name: "Version one" })).toBeInTheDocument();

    await act(async () => {
      resolvers.get("v2")!({
        version: testState.versions[0],
        markdown: "# Stale version two",
      });
    });
    expect(screen.getByRole("heading", { name: "Version one" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Stale version two" })).toBeNull();
  });

  it("does not offer an OpenAI provider without an API key for generation", async () => {
    const summaryTemplate = template("summary", "meeting_summary");
    testState.workspace = {
      ...testState.workspace!,
      templates: [summaryTemplate],
    };
    render(
      <AiDocumentsPanel
        recording={recording}
        transcript={transcript}
        providers={[{ ...provider, hasApiKey: false }]}
        activeProviderId={provider.id}
        onMessage={vi.fn()}
      />,
    );

    expect(await screen.findByText(/OpenAI 官方服务需要 API Key/)).toBeInTheDocument();
    const generateButtons = screen.getAllByRole("button", { name: "生成文档" });
    expect(generateButtons.at(-1)).toBeDisabled();
  });
});
