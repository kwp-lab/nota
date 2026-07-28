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
  noticeAcknowledged: true,
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
}));

vi.mock("./api", () => ({
  api: {
    getAppVersion: vi.fn(async () => "0.1.1"),
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
      firstRunComplete: true,
      recordingNoticeAcknowledged: testState.noticeAcknowledged,
      shortcutsEnabled: true,
      toggleShortcut: "Ctrl+Alt+F9",
      stopShortcut: "Ctrl+Alt+F10",
    })),
    getSnapshot: vi.fn(async () => testState.snapshot),
    listRecordings: vi.fn(async () => []),
    listRecoverable: vi.fn(async () => testState.recoverable),
    onSnapshot: vi.fn(async () => () => undefined),
    onLevels: vi.fn(async () => () => undefined),
    saveSettings: vi.fn(async () => undefined),
    startRecording: vi.fn(async () => snapshot("recording")),
    onRequestStart: vi.fn(
      async (
        handler: (mode?: "process" | "system" | "current") => void,
      ) => {
      testState.requestStart = handler;
      return () => undefined;
      },
    ),
    onRequestExit: vi.fn(async () => () => undefined),
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
    testState.noticeAcknowledged = true;
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
    vi.clearAllMocks();
  });
  afterEach(cleanup);

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
    expect(screen.getByText("v0.1.1")).toBeInTheDocument();
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
    expect(await screen.findByText("发现 1 个未完成录音")).toBeInTheDocument();
    expect(screen.getByText("立即恢复")).toBeInTheDocument();
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
