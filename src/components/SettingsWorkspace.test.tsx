import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type {
  AppSettings,
  AsrConnectionTest,
  AsrModel,
  AsrProvider,
} from "../types";
import { SettingsWorkspace } from "./SettingsWorkspace";

const settings: AppSettings = {
  outputDirectory: "C:\\Recordings",
  aecMode: "auto",
  microphoneEnabled: true,
  firstRunComplete: true,
  shortcutsEnabled: true,
  toggleShortcut: "Ctrl+Alt+F9",
  stopShortcut: "Ctrl+Alt+F10",
  activeAsrProviderId: "lan",
  voiceprintProviderId: null,
  autoTranscribe: false,
};

const provider: AsrProvider = {
  id: "lan",
  name: "LAN FunASR",
  kind: "funAsr",
  baseUrl: "http://192.168.1.20:8000/v1",
  modelId: "sensevoice",
  hasApiKey: true,
  createdAt: "2026-07-28T00:00:00Z",
  updatedAt: "2026-07-28T00:00:00Z",
};

const renderSettings = (
  overrides: Partial<Parameters<typeof SettingsWorkspace>[0]> = {},
) => {
  const actions = {
    onChange: vi.fn(),
    onChooseOutput: vi.fn(),
    onOpenMicrophoneSettings: vi.fn(),
    onSaveProvider: vi.fn(async () => provider),
    onDeleteProvider: vi.fn(async () => undefined),
    onTestProvider: vi.fn(async (): Promise<AsrConnectionTest> => ({
      reachable: true,
      level: "success",
      message: "服务可用",
      models: [],
      device: null,
    })),
    onListModels: vi.fn(async (): Promise<AsrModel[]> => []),
    onDiscardChanges: vi.fn(),
    onSave: vi.fn(),
    onSkipFirstRun: vi.fn(),
  };
  render(
    <SettingsWorkspace
      firstRun={false}
      dirty={false}
      recordingActive={false}
      settings={settings}
      providers={[provider]}
      microphoneCount={1}
      appVersion="0.2.0"
      {...actions}
      {...overrides}
    />,
  );
  return actions;
};

const openProvider = async () => {
  fireEvent.click(await screen.findByRole("button", { name: /LAN FunASR/ }));
};

afterEach(cleanup);

describe("SettingsWorkspace ASR provider feedback", () => {
  it("tests the current unsaved draft and shows loading then success inline", async () => {
    let resolveProbe: ((result: AsrConnectionTest) => void) | undefined;
    const actions = renderSettings();
    actions.onTestProvider.mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveProbe = resolve;
        }),
    );
    await openProvider();
    fireEvent.change(screen.getByDisplayValue(provider.baseUrl), {
      target: { value: "http://draft-host:9000/v1" },
    });
    fireEvent.change(screen.getByLabelText("API Key"), {
      target: { value: "draft-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "测试连接" }));

    expect(screen.getByRole("button", { name: "测试中…" })).toBeDisabled();
    expect(screen.getByText("正在连接服务并检查接口…")).toBeInTheDocument();
    expect(actions.onTestProvider).toHaveBeenCalledWith({
      id: "lan",
      kind: "funAsr",
      baseUrl: "http://draft-host:9000/v1",
      modelId: "sensevoice",
      apiKey: { kind: "replace", value: "draft-key" },
    });

    await act(async () => {
      resolveProbe?.({
        reachable: true,
        level: "success",
        message: "服务可用",
        models: [
          {
            id: "sensevoice",
            ownedBy: "FunASR",
            ready: true,
          },
        ],
        device: "cuda",
      });
    });
    expect(
      await screen.findByText("服务可用 · 设备：cuda · 发现 1 个模型"),
    ).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "服务返回的模型" })).toBeInTheDocument();
  });

  it("shows warning and failure results without relying on a global toast", async () => {
    const actions = renderSettings();
    actions.onTestProvider
      .mockResolvedValueOnce({
        reachable: true,
        level: "warning",
        message: "服务健康检查通过，但模型接口不可用",
        models: [],
        device: "cpu",
      })
      .mockRejectedValueOnce(new Error("401 Unauthorized"));
    await openProvider();

    fireEvent.click(screen.getByRole("button", { name: "测试连接" }));
    expect(
      await screen.findByText(/服务健康检查通过，但模型接口不可用/),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "测试连接" }));
    expect(await screen.findByText(/连接失败：Error: 401 Unauthorized/)).toBeInTheDocument();
  });

  it("loads a visible model list from the draft and fills a single model", async () => {
    const actions = renderSettings();
    actions.onListModels.mockResolvedValue([
      {
        id: "paraformer-zh",
        ownedBy: "FunASR",
        ready: true,
      },
    ]);
    await openProvider();
    fireEvent.change(screen.getByDisplayValue(provider.baseUrl), {
      target: { value: "http://new-host:8000/v1" },
    });
    fireEvent.change(screen.getByDisplayValue(provider.modelId), {
      target: { value: "" },
    });
    fireEvent.click(screen.getByRole("button", { name: "获取模型列表" }));

    expect(actions.onListModels).toHaveBeenCalledWith(
      expect.objectContaining({
        id: "lan",
        baseUrl: "http://new-host:8000/v1",
        modelId: "",
        apiKey: { kind: "keep" },
      }),
    );
    expect(await screen.findByText("已读取 1 个模型，请从下方列表选择。")).toBeInTheDocument();
    expect(screen.getByDisplayValue("paraformer-zh")).toBeInTheDocument();
  });

  it("keeps manual model entry available when model discovery fails", async () => {
    const actions = renderSettings();
    actions.onListModels.mockRejectedValue(new Error("404 Not Found"));
    await openProvider();
    fireEvent.click(screen.getByRole("button", { name: "获取模型列表" }));
    expect(
      await screen.findByText(/无法获取模型列表：Error: 404 Not Found/),
    ).toBeInTheDocument();
    expect(screen.getByDisplayValue("sensevoice")).toBeEnabled();
  });

  it("can probe a new provider before it is saved", async () => {
    const actions = renderSettings();
    fireEvent.click(screen.getByRole("button", { name: "添加" }));
    fireEvent.click(screen.getByRole("button", { name: "测试连接" }));
    await waitFor(() =>
      expect(actions.onTestProvider).toHaveBeenCalledWith(
        expect.objectContaining({
          id: null,
          baseUrl: "http://127.0.0.1:8000/v1",
          apiKey: { kind: "clear" },
        }),
      ),
    );
  });

  it("uses a normal password field and clears a stored key when saved empty", async () => {
    const actions = renderSettings();
    await openProvider();

    const apiKey = screen.getByLabelText("API Key");
    expect(apiKey).toHaveAttribute("type", "password");
    expect(apiKey).toHaveValue("••••••••");
    expect(screen.queryByText("清除已保存的 API Key")).not.toBeInTheDocument();
    expect(
      screen.queryByText(/API Key 将以明文保存在本机 Nota SQLite 数据库中/),
    ).not.toBeInTheDocument();

    fireEvent.change(apiKey, { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: "保存服务" }));

    await waitFor(() =>
      expect(actions.onSaveProvider).toHaveBeenCalledWith(
        expect.objectContaining({
          id: "lan",
          apiKey: { kind: "clear" },
        }),
      ),
    );
  });
});
