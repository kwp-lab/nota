import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { LevelEvent, RecordingSnapshot } from "../types";
import { RecordingOverlayWindow } from "./RecordingOverlayWindow";

const mocks = vi.hoisted(() => ({
  getSnapshot: vi.fn(),
  onSnapshot: vi.fn(),
  onLevels: vi.fn(),
  recordingOverlayReady: vi.fn(),
  resizeRecordingOverlay: vi.fn(),
  dragRecordingOverlay: vi.fn(),
  nudgeRecordingOverlay: vi.fn(),
  showRecordingMainWindow: vi.fn(),
  pauseRecording: vi.fn(),
  resumeRecording: vi.fn(),
  stopRecording: vi.fn(),
}));
vi.mock("../api", () => ({ api: mocks }));

const snapshot = (state: RecordingSnapshot["state"] = "recording"): RecordingSnapshot => ({
  sessionId: "session-1",
  state,
  startedAt: null,
  activeDurationMs: 2538000,
  bytesWritten: 500,
  outputPath: null,
  system: { healthy: true, label: "会议声音" },
  microphone: { healthy: true, label: "麦克风" },
  microphoneSelection: { kind: "followDefaultCommunications" },
  aecStatus: "disabled",
  fault: null,
});
let sendSnapshot: (value: RecordingSnapshot) => void;
let sendLevels: (value: LevelEvent) => void;
let stopSnapshot: ReturnType<typeof vi.fn>;
let stopLevels: ReturnType<typeof vi.fn>;

beforeEach(() => {
  vi.clearAllMocks();
  vi.stubGlobal("matchMedia", vi.fn(() => ({ matches: false })));
  vi.stubGlobal("requestAnimationFrame", vi.fn(() => 1));
  vi.stubGlobal("cancelAnimationFrame", vi.fn());
  stopSnapshot = vi.fn();
  stopLevels = vi.fn();
  mocks.getSnapshot.mockResolvedValue(snapshot());
  mocks.onSnapshot.mockImplementation(async (handler) => {
    sendSnapshot = handler;
    return stopSnapshot;
  });
  mocks.onLevels.mockImplementation(async (handler) => {
    sendLevels = handler;
    return stopLevels;
  });
  mocks.recordingOverlayReady.mockResolvedValue(undefined);
  mocks.resizeRecordingOverlay.mockResolvedValue(undefined);
  mocks.dragRecordingOverlay.mockResolvedValue(undefined);
  mocks.nudgeRecordingOverlay.mockResolvedValue(undefined);
  mocks.showRecordingMainWindow.mockResolvedValue(undefined);
  mocks.pauseRecording.mockResolvedValue(snapshot("paused"));
  mocks.resumeRecording.mockResolvedValue(snapshot());
  mocks.stopRecording.mockResolvedValue(snapshot("finalizing"));
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("recording overlay", () => {
  it("subscribes before acknowledging readiness and shows quiet volume-independent elapsed time", async () => {
    const { unmount } = render(<RecordingOverlayWindow />);
    await waitFor(() => expect(mocks.recordingOverlayReady).toHaveBeenCalledWith("session-1"));
    expect(mocks.onSnapshot.mock.invocationCallOrder[0]).toBeLessThan(mocks.getSnapshot.mock.invocationCallOrder[0]);
    expect(mocks.onLevels.mock.invocationCallOrder[0]).toBeLessThan(mocks.recordingOverlayReady.mock.invocationCallOrder[0]);
    expect(screen.getByText("00:42:18")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "停止并保存" })).not.toBeInTheDocument();
    act(() => {
      sendLevels({ system: 0, microphone: 0.6 });
      sendSnapshot({ ...snapshot(), activeDurationMs: 2539000 });
    });
    expect(screen.getByText("00:42:19")).toBeInTheDocument();
    unmount();
    expect(stopSnapshot).toHaveBeenCalledOnce();
    expect(stopLevels).toHaveBeenCalledOnce();
    expect(document.body).not.toHaveClass("recording-overlay-body");
  });

  it("uses the normal pause, resume and asynchronous stop path and disables duplicate commands", async () => {
    render(<RecordingOverlayWindow />);
    fireEvent.click(await screen.findByRole("button", { name: "正在录音，展开录音操作" }));
    const pause = screen.getByRole("button", { name: "暂停录音" });
    pause.focus();
    fireEvent.click(pause);
    const resume = await screen.findByRole("button", { name: "继续录音" });
    await waitFor(() => expect(resume).toBeEnabled());
    expect(document.activeElement).toBe(resume);
    expect(mocks.pauseRecording).toHaveBeenCalledOnce();
    expect(screen.getByRole("button", { name: "已暂停，收起录音操作" })).toBeInTheDocument();
    fireEvent.click(resume);
    await waitFor(() => expect(mocks.resumeRecording).toHaveBeenCalledOnce());
    let finishStop!: (next: RecordingSnapshot) => void;
    mocks.stopRecording.mockImplementationOnce(() => new Promise((resolve) => { finishStop = resolve; }));
    const stop = screen.getByRole("button", { name: "停止并保存" });
    await waitFor(() => expect(stop).toBeEnabled());
    fireEvent.click(stop);
    fireEvent.click(stop);
    expect(mocks.stopRecording).toHaveBeenCalledOnce();
    expect(screen.getByRole("button", { name: "停止并保存" })).toBeDisabled();
    await act(async () => finishStop(snapshot("finalizing")));
    expect(screen.getByRole("button", { name: "正在保存，收起录音操作" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "停止并保存" })).toBeDisabled();
  });

  it("keeps failures visible and leaves the main window accessible", async () => {
    mocks.stopRecording.mockRejectedValueOnce(new Error("保存失败，恢复文件仍保留"));
    render(<RecordingOverlayWindow />);
    fireEvent.click(await screen.findByRole("button", { name: "正在录音，展开录音操作" }));
    fireEvent.click(screen.getByRole("button", { name: "停止并保存" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("保存失败");
    expect(mocks.resizeRecordingOverlay).toHaveBeenLastCalledWith(true, true);
    fireEvent.click(screen.getByRole("button", { name: "打开主窗口" }));
    expect(mocks.showRecordingMainWindow).toHaveBeenCalledOnce();
  });

  it("retains a finalization fault received through the authoritative snapshot", async () => {
    render(<RecordingOverlayWindow />);
    await waitFor(() => expect(mocks.recordingOverlayReady).toHaveBeenCalled());
    act(() => sendSnapshot({
      ...snapshot("error"),
      fault: { component: "encoder", code: "FINALIZE_FAILED", recoverable: true, userMessage: "保存失败", occurredAt: "" },
    }));
    expect(screen.getByRole("alert")).toHaveTextContent("打开主窗口");
    expect(screen.getByRole("button", { name: "停止并保存" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "打开主窗口" })).toBeEnabled();
  });

  it("does not let a slow initial read or action response replace a newer snapshot", async () => {
    let finishRead!: (value: RecordingSnapshot) => void;
    mocks.getSnapshot.mockImplementationOnce(() => new Promise((resolve) => { finishRead = resolve; }));
    render(<RecordingOverlayWindow />);
    await waitFor(() => expect(mocks.getSnapshot).toHaveBeenCalled());
    act(() => sendSnapshot(snapshot("paused")));
    await act(async () => finishRead(snapshot()));
    fireEvent.click(screen.getByRole("button", { name: "已暂停，展开录音操作" }));
    let finishResume!: (value: RecordingSnapshot) => void;
    mocks.resumeRecording.mockImplementationOnce(() => new Promise((resolve) => { finishResume = resolve; }));
    fireEvent.click(screen.getByRole("button", { name: "继续录音" }));
    act(() => sendSnapshot(snapshot("finalizing")));
    await act(async () => finishResume(snapshot()));
    expect(screen.getByRole("button", { name: "正在保存，收起录音操作" })).toBeInTheDocument();
  });

  it("does not acknowledge readiness when its subscriptions fail", async () => {
    mocks.onLevels.mockRejectedValueOnce(new Error("无法连接状态事件"));
    render(<RecordingOverlayWindow />);
    expect(await screen.findByRole("alert")).toHaveTextContent("无法连接状态事件");
    expect(mocks.recordingOverlayReady).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "重试连接" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "重试连接" }));
    await waitFor(() => expect(mocks.recordingOverlayReady).toHaveBeenCalledWith("session-1"));
  });

  it("preserves expanded controls when initial readiness finishes after user interaction", async () => {
    let finishRead!: (value: RecordingSnapshot) => void;
    mocks.getSnapshot.mockImplementationOnce(() => new Promise((resolve) => { finishRead = resolve; }));
    render(<RecordingOverlayWindow />);
    await waitFor(() => expect(mocks.getSnapshot).toHaveBeenCalled());
    act(() => sendSnapshot(snapshot()));
    fireEvent.click(screen.getByRole("button", { name: "正在录音，展开录音操作" }));
    expect(screen.getByRole("button", { name: "停止并保存" })).toBeEnabled();
    await act(async () => finishRead(snapshot()));
    await waitFor(() => expect(mocks.recordingOverlayReady).toHaveBeenCalledWith("session-1"));
    expect(screen.getByRole("button", { name: "停止并保存" })).toBeEnabled();
  });

  it("expands through the keyboard and returns focus on Escape", async () => {
    render(<RecordingOverlayWindow />);
    const trigger = await screen.findByRole("button", { name: "正在录音，展开录音操作" });
    fireEvent.click(trigger);
    expect(mocks.resizeRecordingOverlay).toHaveBeenLastCalledWith(true, false);
    fireEvent.keyDown(screen.getByRole("button", { name: "暂停录音" }), { key: "Escape" });
    expect(trigger).toHaveAttribute("aria-expanded", "false");
    expect(document.activeElement).toBe(trigger);
    fireEvent.keyDown(screen.getByRole("button", { name: "拖动状态卡片" }), { key: "ArrowLeft" });
    expect(mocks.nudgeRecordingOverlay).toHaveBeenCalledWith("left");
  });

  it("ignores disabled or unhealthy microphone activity but accepts microphone-only recording", async () => {
    let frameCallback: FrameRequestCallback | undefined;
    vi.stubGlobal("requestAnimationFrame", vi.fn((callback: FrameRequestCallback) => {
      frameCallback = callback;
      return 1;
    }));
    const { container } = render(<RecordingOverlayWindow />);
    await waitFor(() => expect(mocks.recordingOverlayReady).toHaveBeenCalled());
    act(() => sendLevels({ system: 0, microphone: 0.01 }));
    act(() => frameCallback?.(100));
    const path = container.querySelector("path")!;
    expect(path.getAttribute("d")).not.toBe("M 0 14 L 100 14");
    act(() => {
      sendSnapshot({ ...snapshot(), microphoneSelection: null });
      for (let time = 200; time <= 2500; time += 100) frameCallback?.(time);
    });
    act(() => {
      for (let time = 2600; time <= 5000; time += 100) frameCallback?.(time);
    });
    expect(path).toHaveAttribute("d", "M 0 14 L 100 14");
  });
});
