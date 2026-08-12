import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
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
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(async () => null) }));

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
  status: "completed",
  providerName: "Test ASR",
  modelId: "test-asr-model",
  text: "Discussed the release.",
  segments: [],
  language: "en",
  speakerNames: {},
  speakerAssignments: {},
  completedChunks: 1,
  totalChunks: 1,
  errorMessage: null,
  updatedAt: "2026-08-09T00:01:00Z",
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
    expect(screen.getByRole("button", { name: "重新生成" })).toHaveClass("compact");
    expect(screen.getByRole("button", { name: "重新生成" })).not.toHaveAttribute("title");
    expect(screen.getByRole("button", { name: "AI修改" })).toHaveClass("compact");
    expect(screen.getByRole("button", { name: "AI修改" })).not.toHaveAttribute("title");
    fireEvent.click(screen.getByRole("tab", { name: "生成详情" }));
    expect(await screen.findByText("该版本生成时尚未记录原始请求 JSON。")).toBeInTheDocument();
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

    const documentTab = await screen.findByRole("tab", { name: "文档" });
    fireEvent.keyDown(documentTab.parentElement!, { key: "ArrowRight" });
    expect(screen.getByRole("tab", { name: "生成详情" })).toHaveAttribute("aria-selected", "true");
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
