import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { api } from "./api";
import type { RecordingSnapshot } from "./types";

const testState = vi.hoisted(() => ({
  snapshot: {} as RecordingSnapshot,
  devicesError: false,
  devices: [] as Array<{
    id: string;
    name: string;
    direction: "render" | "capture";
    isDefaultCommunications: boolean;
    formFactor: string;
    active: boolean;
  }>,
  recoverable: [] as Array<Record<string, unknown>>,
  firstRunComplete: true,
  noticeAcknowledged: true,
  activeAsrProviderId: null as string | null,
  providers: [] as Array<{
    id: string;
    name: string;
    kind: "funAsr" | "openAiCompatible";
    baseUrl: string;
    modelId: string;
    hasApiKey: boolean;
    createdAt: string;
    updatedAt: string;
  }>,
  targets: [] as Array<{
    id: string;
    kind: "process";
    displayName: string;
    processId: number;
    executablePath: string;
    browser: boolean;
    priority: number;
  }>,
  requestStart: null as
    | ((mode?: "process" | "system" | "current") => void)
    | null,
  snapshotListener: null as
    | ((snapshot: RecordingSnapshot) => void)
    | null,
}));

vi.mock("./api", () => ({
  api: {
    getAppVersion: vi.fn(async () => "0.2.0"),
    listCaptureTargets: vi.fn(async () => testState.targets),
    listAudioDevices: vi.fn(async () => {
      if (testState.devicesError) throw new Error("麦克风权限已关闭");
      return testState.devices;
    }),
    getSettings: vi.fn(async () => ({
      outputDirectory: "C:\\Recordings",
      aecMode: "auto",
      microphoneEnabled: true,
      consentTemplate: "已告知",
      firstRunComplete: testState.firstRunComplete,
      recordingNoticeAcknowledged: testState.noticeAcknowledged,
      shortcutsEnabled: true,
      toggleShortcut: "Ctrl+Alt+F9",
      stopShortcut: "Ctrl+Alt+F10",
      activeAsrProviderId: testState.activeAsrProviderId,
      autoTranscribe: false,
    })),
    getSnapshot: vi.fn(async () => testState.snapshot),
    listRecordings: vi.fn(async () => []),
    listRecoverable: vi.fn(async () => testState.recoverable),
    listAsrProviders: vi.fn(async () => testState.providers),
    onSnapshot: vi.fn(
      async (handler: (snapshot: RecordingSnapshot) => void) => {
        testState.snapshotListener = handler;
        return () => undefined;
      },
    ),
    onLevels: vi.fn(async () => () => undefined),
    onAsrStatus: vi.fn(async () => () => undefined),
    saveSettings: vi.fn(async () => undefined),
    startRecording: vi.fn(async () => snapshot("recording")),
    pauseRecording: vi.fn(async () => snapshot("paused")),
    resumeRecording: vi.fn(async () => snapshot("recording")),
    stopRecording: vi.fn(async () => snapshot("completed")),
    onRequestStart: vi.fn(
      async (
        handler: (mode?: "process" | "system" | "current") => void,
      ) => {
      testState.requestStart = handler;
      return () => undefined;
      },
    ),
    onRequestExit: vi.fn(async () => () => undefined),
    saveAsrProvider: vi.fn(),
    deleteAsrProvider: vi.fn(async () => undefined),
    testAsrProvider: vi.fn(async () => ({
      reachable: true,
      level: "success",
      message: "服务可用",
      models: [],
      device: null,
    })),
    listAsrModels: vi.fn(async () => []),
    getTranscript: vi.fn(async () => {
      throw new Error("没有转写结果");
    }),
  },
}));

const snapshot = (state: RecordingSnapshot["state"]): RecordingSnapshot => ({
  sessionId: state === "idle" ? null : "session",
  state,
  startedAt: state === "idle" ? null : new Date().toISOString(),
  activeDurationMs: 12_000,
  bytesWritten: 120_000,
  outputPath: null,
  system: { healthy: true, label: "会议声音" },
  microphone: { healthy: true, label: "麦克风" },
  aecStatus: "enabled",
  fault: null,
});

describe("Nota UI states", () => {
  beforeEach(() => {
    testState.snapshot = snapshot("idle");
    testState.devicesError = false;
    testState.devices = [];
    testState.recoverable = [];
    testState.firstRunComplete = true;
    testState.noticeAcknowledged = true;
    testState.activeAsrProviderId = null;
    testState.providers = [];
    testState.targets = [
      {
        id: "process:42",
        kind: "process",
        displayName: "Zoom",
        processId: 42,
        executablePath: "C:\\Zoom.exe",
        browser: false,
        priority: 100,
      },
    ];
    testState.requestStart = null;
    testState.snapshotListener = null;
    vi.clearAllMocks();
  });
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  for (const state of ["idle", "recording", "paused", "interrupted"] as const) {
    it(`renders ${state}`, async () => {
      testState.snapshot = snapshot(state);
      render(<App />);
      expect(
        await screen.findByText(state === "idle" ? "准备好记录会议" : "会议录音进行中"),
      ).toBeInTheDocument();
      if (state !== "idle") {
        expect(await screen.findByText("停止并保存")).toBeInTheDocument();
      }
    });
  }

  it("shows a transient success toast after stopping and saving", async () => {
    testState.snapshot = snapshot("recording");
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "停止并保存" }));

    expect(await screen.findByRole("status")).toHaveTextContent(
      "录音已安全保存",
    );
    expect(vi.mocked(api.stopRecording)).toHaveBeenCalledOnce();
  });

  it("shows and deduplicates persistent recording faults received at runtime", async () => {
    render(<App />);
    await waitFor(() => expect(testState.snapshotListener).not.toBeNull());
    const faultySnapshot: RecordingSnapshot = {
      ...snapshot("recording"),
      fault: {
        component: "microphone",
        code: "DEVICE_DISCONNECTED",
        recoverable: true,
        userMessage: "麦克风连接已中断，正在重试。",
        occurredAt: "2026-07-30T12:00:00Z",
      },
    };

    act(() => testState.snapshotListener?.(faultySnapshot));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "麦克风连接已中断，正在重试。",
    );

    act(() => testState.snapshotListener?.(faultySnapshot));
    expect(
      screen.getAllByText("麦克风连接已中断，正在重试。"),
    ).toHaveLength(1);

    fireEvent.click(screen.getByRole("button", { name: "关闭通知" }));
    await waitFor(() =>
      expect(
        screen.queryByText("麦克风连接已中断，正在重试。"),
      ).not.toBeInTheDocument(),
    );
    act(() => testState.snapshotListener?.(faultySnapshot));
    expect(
      screen.queryByText("麦克风连接已中断，正在重试。"),
    ).not.toBeInTheDocument();
  });

  it("shows microphone permission failures", async () => {
    testState.devicesError = true;
    render(<App />);
    expect(await screen.findByRole("alert")).toHaveTextContent("麦克风权限已关闭");
  });

  it("shows the current default device names in follow-default options", async () => {
    testState.devices = [
      {
        id: "render:default",
        name: "扬声器 (Realtek(R) Audio)",
        direction: "render",
        isDefaultCommunications: true,
        formFactor: "Speakers",
        active: true,
      },
      {
        id: "capture:default",
        name: "麦克风阵列 (Realtek(R) Audio)",
        direction: "capture",
        isDefaultCommunications: true,
        formFactor: "Microphone",
        active: true,
      },
    ];
    render(<App />);

    expect(
      await screen.findByRole("option", {
        name: "跟随默认通信设备（麦克风阵列 (Realtek(R) Audio)）",
      }),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "全部系统声音" }));
    expect(
      screen.getByRole("option", {
        name: "跟随默认通信设备（扬声器 (Realtek(R) Audio)）",
      }),
    ).toBeInTheDocument();
  });

  it("shows the application version in settings", async () => {
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "设置" }));
    expect(await screen.findByText("关于此应用")).toBeInTheDocument();
    expect(screen.getByText("v0.2.0")).toBeInTheDocument();
    expect(screen.queryByRole("dialog", { name: "设置" })).not.toBeInTheDocument();
  });

  it("never exposes a saved ASR API key to the settings form", async () => {
    testState.activeAsrProviderId = "lan";
    testState.providers = [
      {
        id: "lan",
        name: "LAN FunASR",
        kind: "funAsr",
        baseUrl: "http://192.168.1.20:8000/v1",
        modelId: "sensevoice",
        hasApiKey: true,
        createdAt: "2026-07-28T00:00:00Z",
        updatedAt: "2026-07-28T00:00:00Z",
      },
    ];
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "设置" }));
    fireEvent.click(await screen.findByRole("button", { name: /LAN FunASR/ }));
    const key = screen.getByLabelText("API Key");
    expect(key).toHaveAttribute("type", "password");
    expect(key).toHaveValue("••••••••");
    expect(screen.queryByText(/明文保存在本机 Nota SQLite 数据库/)).not.toBeInTheDocument();
  });

  it("switches between the recorder and recording library from the sidebar", async () => {
    render(<App />);
    expect(await screen.findByText("准备好记录会议")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "录音记录" }));
    expect(await screen.findByText("还没有录音")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "录音" }));
    expect(await screen.findByText("准备好记录会议")).toBeInTheDocument();
  });

  it("uses the settings workspace, saves explicitly, and guards dirty navigation", async () => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValueOnce(false).mockReturnValueOnce(true);
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "设置" }));
    const aec = await screen.findByRole("combobox", { name: "回声消除" });
    fireEvent.change(aec, { target: { value: "off" } });
    expect(screen.getByText("有未保存的更改")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "录音" }));
    expect(confirm).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("heading", { name: "设置" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "录音" }));
    expect(confirm).toHaveBeenCalledTimes(2);
    expect(await screen.findByText("准备好记录会议")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "设置" }));
    fireEvent.change(screen.getByRole("combobox", { name: "回声消除" }), {
      target: { value: "off" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存设置" }));
    await waitFor(() =>
      expect(vi.mocked(api.saveSettings)).toHaveBeenCalledWith(
        expect.objectContaining({ aecMode: "off", firstRunComplete: true }),
      ),
    );
    expect(await screen.findByText("所有普通设置均已保存")).toBeInTheDocument();
  });

  it("opens first run as a non-blocking settings page and can skip it", async () => {
    testState.firstRunComplete = false;
    render(<App />);
    expect(await screen.findByText("欢迎使用 Nota")).toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "录音" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "稍后设置" }));
    await waitFor(() =>
      expect(vi.mocked(api.saveSettings)).toHaveBeenCalledWith(
        expect.objectContaining({ firstRunComplete: true }),
      ),
    );
    expect(await screen.findByText("准备好记录会议")).toBeInTheDocument();
  });

  it("keeps an active recording visible while browsing settings", async () => {
    testState.snapshot = snapshot("recording");
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "设置" }));
    expect(
      await screen.findByText(/当前录音不会被设置页操作中断/),
    ).toBeInTheDocument();
  });

  it("offers crash recovery", async () => {
    testState.recoverable = [
      {
        id: "partial",
        title: "未完成录音",
        path: "partial.ogg",
        createdAt: new Date().toISOString(),
        durationMs: 0,
        sizeBytes: 1024,
        recovered: true,
      },
    ];
    render(<App />);
    const recovery = await screen.findByRole("button", {
      name: "发现 1 个未完成录音，前往录音记录恢复",
    });
    fireEvent.click(recovery);
    expect(await screen.findByText("1 个录音可恢复")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "恢复" })).toBeInTheDocument();
  });

  it("starts directly after the first notice was acknowledged", async () => {
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "开始录音" }));
    await waitFor(() =>
      expect(vi.mocked(api.startRecording)).toHaveBeenCalledWith(
        expect.objectContaining({ consentConfirmed: true }),
      ),
    );
    expect(screen.queryByText("首次录音提示")).not.toBeInTheDocument();
  });

  it("starts directly from the tray after acknowledgement", async () => {
    render(<App />);
    await waitFor(() => expect(testState.requestStart).not.toBeNull());
    act(() => testState.requestStart?.("current"));
    await waitFor(() =>
      expect(vi.mocked(api.startRecording)).toHaveBeenCalledWith(
        expect.objectContaining({ consentConfirmed: true }),
      ),
    );
    expect(screen.queryByText("首次录音提示")).not.toBeInTheDocument();
  });

  it("starts system audio from the dedicated tray action", async () => {
    render(<App />);
    await waitFor(() => expect(testState.requestStart).not.toBeNull());
    act(() => testState.requestStart?.("system"));
    await waitFor(() =>
      expect(vi.mocked(api.startRecording)).toHaveBeenCalledWith(
        expect.objectContaining({
          capture: expect.objectContaining({ kind: "system" }),
        }),
      ),
    );
  });

  it("refreshes applications when the window regains focus", async () => {
    render(<App />);
    expect(
      await screen.findByRole("option", { name: "[Zoom.exe]: Zoom" }),
    ).toBeInTheDocument();
    testState.targets = [
      {
        id: "process:77",
        kind: "process",
        displayName: "腾讯会议",
        processId: 77,
        executablePath: "C:\\Program Files\\Tencent\\wemeetapp.exe",
        browser: false,
        priority: 90,
      },
      ...testState.targets,
    ];

    act(() => window.dispatchEvent(new Event("focus")));
    expect(
      await screen.findByRole("option", {
        name: "[wemeetapp.exe]: 腾讯会议",
      }),
    ).toBeInTheDocument();
  });

  it("rebinds the selected application after its process id changes", async () => {
    render(<App />);
    const source = await screen.findByRole("combobox", { name: "录音来源" });
    expect(source).toHaveValue("process:42");
    testState.targets = [
      {
        id: "process:84",
        kind: "process",
        displayName: "Zoom Meeting",
        processId: 84,
        executablePath: "c:\\zoom.exe",
        browser: false,
        priority: 100,
      },
    ];

    act(() => window.dispatchEvent(new Event("focus")));
    await waitFor(() => expect(source).toHaveValue("process:84"));
  });

  it("shows and persists the notice only before the first recording", async () => {
    testState.noticeAcknowledged = false;
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "开始录音" }));
    expect(await screen.findByText("首次录音提示")).toBeInTheDocument();
    fireEvent.click(screen.getByText("我已了解上述提示，后续开始录音时不再提醒"));
    fireEvent.click(screen.getByRole("button", { name: "确认并开始录音" }));
    await waitFor(() =>
      expect(vi.mocked(api.saveSettings)).toHaveBeenCalledWith(
        expect.objectContaining({ recordingNoticeAcknowledged: true }),
      ),
    );
    expect(vi.mocked(api.startRecording)).toHaveBeenCalled();
  });
});
