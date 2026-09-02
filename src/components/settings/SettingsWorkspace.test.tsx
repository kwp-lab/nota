import { useMemo, useState } from "react";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  AiTemplate,
  AppSettings,
  AsrProvider,
  LlmProvider,
} from "../../types";
import { SettingsWorkspace } from "./SettingsWorkspace";
import type { SettingsRoute } from "./types";

const mocks = vi.hoisted(() => ({
  open: vi.fn(),
  saveSettings: vi.fn(),
  setActiveAsrProvider: vi.fn(),
  setActiveLlmProvider: vi.fn(),
  openMicrophoneSettings: vi.fn(),
  openLogDirectory: vi.fn(),
  listAsrProviders: vi.fn(),
  saveAsrProvider: vi.fn(),
  deleteAsrProvider: vi.fn(),
  testAsrProvider: vi.fn(),
  listAsrModels: vi.fn(),
  listLlmProviders: vi.fn(),
  saveLlmProvider: vi.fn(),
  deleteLlmProvider: vi.fn(),
  testLlmProvider: vi.fn(),
  listLlmModels: vi.fn(),
  getSettings: vi.fn(),
  listAiTemplates: vi.fn(),
  saveAiTemplate: vi.fn(),
  cloneAiTemplate: vi.fn(),
  archiveAiTemplate: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: mocks.open }));
vi.mock("../../api", () => ({
  api: {
    saveSettings: mocks.saveSettings,
    setActiveAsrProvider: mocks.setActiveAsrProvider,
    setActiveLlmProvider: mocks.setActiveLlmProvider,
    openMicrophoneSettings: mocks.openMicrophoneSettings,
    openLogDirectory: mocks.openLogDirectory,
    listAsrProviders: mocks.listAsrProviders,
    saveAsrProvider: mocks.saveAsrProvider,
    deleteAsrProvider: mocks.deleteAsrProvider,
    testAsrProvider: mocks.testAsrProvider,
    listAsrModels: mocks.listAsrModels,
    listLlmProviders: mocks.listLlmProviders,
    saveLlmProvider: mocks.saveLlmProvider,
    deleteLlmProvider: mocks.deleteLlmProvider,
    testLlmProvider: mocks.testLlmProvider,
    listLlmModels: mocks.listLlmModels,
    getSettings: mocks.getSettings,
    listAiTemplates: mocks.listAiTemplates,
    saveAiTemplate: mocks.saveAiTemplate,
    cloneAiTemplate: mocks.cloneAiTemplate,
    archiveAiTemplate: mocks.archiveAiTemplate,
  },
}));

const settings: AppSettings = {
  outputDirectory: "C:\\Recordings",
  aiDocumentsDirectory: "C:\\AI Documents",
  aecMode: "auto",
  microphoneEnabled: true,
  firstRunComplete: true,
  shortcutsEnabled: true,
  toggleShortcut: "Ctrl+Alt+F9",
  stopShortcut: "Ctrl+Alt+F10",
  activeAsrProviderId: "lan",
  voiceprintProviderId: null,
  autoTranscribe: false,
  activeLlmProviderId: "llm",
};

const asrProvider: AsrProvider = {
  id: "lan",
  name: "LAN FunASR",
  kind: "funAsr",
  baseUrl: "http://192.168.1.20:8000/v1",
  modelId: "sensevoice",
  hasApiKey: true,
  capabilities: {
    wholeMeeting: true,
    diarization: true,
    speakerCountMin: 1,
    speakerCountMax: 64,
    voiceprintAnalysis: true,
    modelDiscovery: true,
    cloudUpload: false,
    maxReliableAudioSeconds: null,
  },
  createdAt: "2026-08-01T00:00:00Z",
  updatedAt: "2026-08-01T00:00:00Z",
};

const llmProvider: LlmProvider = {
  id: "llm",
  name: "OpenAI",
  kind: "openAi",
  baseUrl: "https://api.openai.com/v1",
  modelId: "gpt-5-mini",
  inputTokenBudget: 32768,
  maxOutputTokens: 4096,
  hasApiKey: true,
  createdAt: "2026-08-01T00:00:00Z",
  updatedAt: "2026-08-01T00:00:00Z",
};

const builtinTemplate: AiTemplate = {
  id: "builtin-summary",
  name: "会议纪要",
  description: "生成结构化会议纪要",
  builtinKey: "summary",
  taskInstructions: "总结会议内容。",
  outputRequirements: "使用 Markdown 输出。",
  requiresSpeakerLabels: false,
  revision: 1,
  archived: false,
  createdAt: "2026-08-01T00:00:00Z",
  updatedAt: "2026-08-01T00:00:00Z",
};

const customTemplate: AiTemplate = {
  ...builtinTemplate,
  id: "custom-summary",
  name: "产品复盘",
  description: "提炼产品决策与行动项",
  builtinKey: null,
  taskInstructions: "整理产品讨论。",
  revision: 2,
};

interface HarnessProps {
  initialRoute?: SettingsRoute;
  initialSettings?: AppSettings;
  asrProviders?: AsrProvider[];
  llmProviders?: LlmProvider[];
  onFirstRunComplete?: () => void;
  onError?: (error: unknown) => void;
}

function Harness(props: HarnessProps) {
  const [route, setRoute] = useState<SettingsRoute>(props.initialRoute ?? "recording");
  const [currentSettings, setCurrentSettings] = useState(props.initialSettings ?? settings);
  const [asrProviders, setAsrProviders] = useState(props.asrProviders ?? [asrProvider]);
  const [llmProviders, setLlmProviders] = useState(props.llmProviders ?? [llmProvider]);
  const actions = useMemo(() => ({
    onSettingsChange: setCurrentSettings,
    onAsrProvidersChange: setAsrProviders,
    onLlmProvidersChange: setLlmProviders,
    onFirstRunComplete: props.onFirstRunComplete ?? vi.fn(),
    onEditorDirtyChange: vi.fn(),
    onToast: vi.fn(),
    onError: props.onError ?? vi.fn(),
  }), [props.onError, props.onFirstRunComplete]);

  return (
    <SettingsWorkspace
      route={route}
      onRouteChange={setRoute}
      model={{
        settings: currentSettings,
        asrProviders,
        llmProviders,
        microphoneCount: 1,
        appVersion: "0.8.0",
        recordingActive: false,
      }}
      actions={actions}
    />
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.saveSettings.mockResolvedValue(undefined);
  mocks.openMicrophoneSettings.mockResolvedValue(undefined);
  mocks.openLogDirectory.mockResolvedValue(undefined);
  mocks.setActiveAsrProvider.mockImplementation(async (id: string | null) => ({
    ...settings,
    activeAsrProviderId: id,
    autoTranscribe: id ? settings.autoTranscribe : false,
  }));
  mocks.setActiveLlmProvider.mockImplementation(async (id: string | null) => ({
    ...settings,
    activeLlmProviderId: id,
  }));
  mocks.getSettings.mockResolvedValue(settings);
  mocks.listAsrProviders.mockResolvedValue([asrProvider]);
  mocks.listLlmProviders.mockResolvedValue([llmProvider]);
  mocks.saveAsrProvider.mockResolvedValue(asrProvider);
  mocks.saveLlmProvider.mockResolvedValue(llmProvider);
  mocks.testAsrProvider.mockResolvedValue({
    reachable: true,
    level: "success",
    message: "服务可用",
    models: [],
    device: "cpu",
  });
  mocks.listAsrModels.mockResolvedValue([]);
  mocks.testLlmProvider.mockResolvedValue({ reachable: true, message: "连接成功" });
  mocks.listLlmModels.mockResolvedValue([]);
  mocks.listAiTemplates.mockResolvedValue([builtinTemplate]);
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("SettingsWorkspace navigation and preferences", () => {
  it("shows one category at a time and keeps the parent category selected on manager pages", async () => {
    render(<Harness />);
    expect(screen.getByRole("heading", { name: "录音与保存" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "语音转写" }));
    expect(screen.getByRole("heading", { name: "语音转写" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /管理转写服务/ }));
    expect(screen.getByRole("heading", { name: "语音转写 / 服务管理" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "语音转写" })).toHaveAttribute("aria-current", "page");
  });

  it("auto-saves ordinary preferences without completing first run", async () => {
    render(<Harness initialSettings={{ ...settings, firstRunComplete: false }} />);
    fireEvent.change(screen.getByRole("combobox", { name: "回声消除" }), {
      target: { value: "off" },
    });
    await waitFor(() => expect(mocks.saveSettings).toHaveBeenCalledWith(
      expect.objectContaining({ aecMode: "off", firstRunComplete: false }),
    ));
    expect(screen.queryByRole("button", { name: "保存设置" })).not.toBeInTheDocument();
  });

  it("rolls back an optimistic preference when persistence fails", async () => {
    const onError = vi.fn();
    mocks.saveSettings.mockRejectedValueOnce(new Error("disk full"));
    render(<Harness onError={onError} />);
    fireEvent.change(screen.getByRole("combobox", { name: "回声消除" }), {
      target: { value: "off" },
    });
    await waitFor(() => expect(screen.getByRole("combobox", { name: "回声消除" })).toHaveValue("auto"));
    expect(onError).toHaveBeenCalled();
    expect(screen.getByRole("alert")).toHaveTextContent("disk full");
  });

  it("serializes writes and coalesces pending ordinary-setting snapshots", async () => {
    let finishFirst!: () => void;
    mocks.saveSettings.mockImplementationOnce(() => new Promise<void>((resolve) => {
      finishFirst = resolve;
    }));
    render(<Harness />);
    const aec = screen.getByRole("combobox", { name: "回声消除" });
    fireEvent.change(aec, { target: { value: "on" } });
    fireEvent.change(aec, { target: { value: "off" } });
    fireEvent.change(aec, { target: { value: "auto" } });
    expect(mocks.saveSettings).toHaveBeenCalledTimes(1);
    finishFirst();
    await waitFor(() => expect(mocks.saveSettings).toHaveBeenCalledTimes(2));
    expect(mocks.saveSettings).toHaveBeenLastCalledWith(
      expect.objectContaining({ aecMode: "auto" }),
    );
  });

  it("does not save when directory selection is cancelled", async () => {
    mocks.open.mockResolvedValueOnce(null);
    render(<Harness />);
    fireEvent.click(screen.getByRole("button", { name: "更改" }));
    await waitFor(() => expect(mocks.open).toHaveBeenCalled());
    expect(mocks.saveSettings).not.toHaveBeenCalled();
  });

  it("uses the dedicated command when changing the default ASR provider", async () => {
    render(<Harness initialRoute="transcription" />);
    const selector = screen.getByRole("combobox", { name: "默认语音转写服务" });
    expect(selector.parentElement).toHaveClass("select-wrap", "settings-select");
    fireEvent.change(selector, {
      target: { value: "" },
    });
    await waitFor(() => expect(mocks.setActiveAsrProvider).toHaveBeenCalledWith(null));
  });
});

describe("Settings resource managers", () => {
  it("keeps stored ASR keys masked and tests the unsaved draft", async () => {
    render(<Harness initialRoute="asrProviders" />);
    const key = screen.getByLabelText("API Key");
    expect(key).toHaveAttribute("type", "password");
    expect(key).toHaveValue("••••••••");
    fireEvent.change(screen.getByDisplayValue(asrProvider.baseUrl), {
      target: { value: "http://draft-host:9000/v1" },
    });
    fireEvent.click(screen.getByRole("button", { name: "测试连接" }));
    await waitFor(() => expect(mocks.testAsrProvider).toHaveBeenCalledWith(
      expect.objectContaining({ baseUrl: "http://draft-host:9000/v1", apiKey: { kind: "keep" } }),
    ));
    expect(await screen.findByText(/服务可用 · 设备：cpu/)).toBeInTheDocument();
  });

  it("applies fixed DashScope fields and preserves its cloud disclosure", () => {
    render(<Harness initialRoute="asrProviders" />);
    fireEvent.click(screen.getByRole("button", { name: /添加/ }));
    const draftCard = screen.getByRole("button", { name: "正在添加 本地 FunASR FunASR" });
    expect(draftCard).toHaveClass("resource-draft", "selected");
    expect(screen.getByText("填写中")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("服务类型"), { target: { value: "dashScope" } });
    expect(screen.getByRole("button", { name: "正在添加 千问云转写 阿里云千问" })).toHaveClass("selected");
    expect(screen.getByDisplayValue("https://dashscope.aliyuncs.com/api/v1")).toHaveAttribute("readonly");
    expect(screen.getByDisplayValue("qwen-audio-3.0-asr-flash-filetrans")).toHaveAttribute("readonly");
    expect(screen.getByText(/约在 48 小时后清理/)).toBeInTheDocument();
  });

  it("renders LLM provider readiness and explicit save controls", () => {
    render(<Harness initialRoute="llmProviders" />);
    expect(screen.getByRole("heading", { name: "OpenAI" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /保存服务/ })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("模型 ID"), { target: { value: "gpt-5.1" } });
    expect(screen.getByRole("button", { name: /保存服务/ })).toBeEnabled();
  });

  it("loads built-in templates as read-only resources", async () => {
    render(<Harness initialRoute="aiTemplates" />);
    expect(await screen.findByRole("heading", { name: "会议纪要" })).toBeInTheDocument();
    expect(screen.getByText("内置模板不能直接修改")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /复制为自定义模板/ })).toBeEnabled();
  });

  it("shows template loading failures and offers a retry", async () => {
    mocks.listAiTemplates.mockRejectedValueOnce(new Error("数据库暂时不可用"));
    render(<Harness initialRoute="aiTemplates" />);
    expect(await screen.findByRole("alert")).toHaveTextContent("数据库暂时不可用");
    expect(screen.getByRole("button", { name: "重试" })).toBeEnabled();
  });

  it("copies a built-in template into an editable custom resource", async () => {
    vi.spyOn(window, "prompt").mockReturnValue("产品复盘");
    mocks.cloneAiTemplate.mockResolvedValue(customTemplate);
    mocks.listAiTemplates
      .mockResolvedValueOnce([builtinTemplate])
      .mockResolvedValueOnce([builtinTemplate, customTemplate]);
    render(<Harness initialRoute="aiTemplates" />);
    fireEvent.click(await screen.findByRole("button", { name: /复制为自定义模板/ }));
    await waitFor(() => expect(mocks.cloneAiTemplate).toHaveBeenCalledWith(
      builtinTemplate.id,
      "产品复盘",
    ));
    expect(await screen.findByRole("heading", { name: "产品复盘" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /保存模板/ })).toBeDisabled();
  });

  it("saves a custom template and refreshes its revision", async () => {
    const revised = { ...customTemplate, taskInstructions: "整理产品决策。", revision: 3 };
    mocks.listAiTemplates
      .mockResolvedValueOnce([customTemplate])
      .mockResolvedValueOnce([revised]);
    mocks.saveAiTemplate.mockResolvedValue(revised);
    render(<Harness initialRoute="aiTemplates" />);
    const instructions = await screen.findByLabelText("任务指令");
    fireEvent.change(instructions, { target: { value: "整理产品决策。" } });
    fireEvent.click(screen.getByRole("button", { name: /保存模板/ }));
    await waitFor(() => expect(mocks.saveAiTemplate).toHaveBeenCalledWith(
      expect.objectContaining({ id: customTemplate.id, taskInstructions: "整理产品决策。" }),
    ));
    expect(await screen.findByText("r3")).toBeInTheDocument();
  });

  it("archives custom templates and guards dirty category navigation", async () => {
    mocks.listAiTemplates.mockResolvedValue([customTemplate]);
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    render(<Harness initialRoute="aiTemplates" />);
    const name = await screen.findByLabelText("模板名称");
    fireEvent.change(name, { target: { value: "尚未保存的模板" } });
    fireEvent.click(screen.getByRole("button", { name: "关于" }));
    expect(confirm).toHaveBeenCalled();
    expect(screen.getByRole("heading", { name: "AI 文档 / 模板管理" })).toBeInTheDocument();
    expect(name).toHaveValue("尚未保存的模板");

    confirm.mockReturnValue(true);
    mocks.archiveAiTemplate.mockResolvedValue(undefined);
    mocks.listAiTemplates.mockResolvedValueOnce([]);
    fireEvent.click(screen.getByRole("button", { name: /归档/ }));
    await waitFor(() => expect(mocks.archiveAiTemplate).toHaveBeenCalledWith(customTemplate.id));
  });
});

describe("Settings setup and utilities", () => {
  it("keeps first run non-blocking and only completes it from the setup actions", async () => {
    const complete = vi.fn();
    render(<Harness
      initialRoute="setup"
      initialSettings={{ ...settings, firstRunComplete: false }}
      onFirstRunComplete={complete}
    />);
    expect(screen.getByRole("heading", { name: "开始使用 Nota" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "稍后设置" }));
    await waitFor(() => expect(mocks.saveSettings).toHaveBeenCalledWith(
      expect.objectContaining({ firstRunComplete: true }),
    ));
    expect(complete).toHaveBeenCalled();
  });

  it("returns from Provider setup to the first-run checklist", async () => {
    render(<Harness
      initialRoute="setup"
      initialSettings={{ ...settings, firstRunComplete: false }}
    />);
    fireEvent.click(screen.getByRole("button", { name: "管理语音转写服务" }));
    expect(screen.getByRole("heading", { name: "语音转写 / 服务管理" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "返回" }));
    expect(screen.getByRole("heading", { name: "开始使用 Nota" })).toBeInTheDocument();
  });

  it("commits shortcut drafts on blur and keeps a registration error inline", async () => {
    mocks.saveSettings.mockRejectedValueOnce(new Error("快捷键冲突"));
    render(<Harness initialRoute="shortcuts" />);
    const input = screen.getByRole("textbox", { name: "开始暂停快捷键" });
    fireEvent.change(input, { target: { value: "Ctrl+Alt+X" } });
    fireEvent.blur(input);
    expect((await screen.findAllByRole("alert")).some((alert) => alert.textContent?.includes("快捷键冲突"))).toBe(true);
    expect(input).toHaveValue("Ctrl+Alt+X");
  });

  it("trims and commits the shortcut pair when Enter moves focus", async () => {
    render(<Harness initialRoute="shortcuts" />);
    const input = screen.getByRole("textbox", { name: "开始暂停快捷键" });
    fireEvent.change(input, { target: { value: "  Ctrl+Alt+X  " } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(mocks.saveSettings).toHaveBeenCalledWith(
      expect.objectContaining({
        toggleShortcut: "Ctrl+Alt+X",
        stopShortcut: "Ctrl+Alt+F10",
      }),
    ));
  });

  it("opens the local diagnostic directory without saving settings", async () => {
    render(<Harness initialRoute="diagnostics" />);
    fireEvent.click(screen.getByRole("button", { name: "打开日志目录" }));
    await waitFor(() => expect(mocks.openLogDirectory).toHaveBeenCalled());
    expect(mocks.saveSettings).not.toHaveBeenCalled();
  });
});
