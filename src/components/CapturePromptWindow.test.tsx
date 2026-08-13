import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CapturePrompt } from "../types";
import { CapturePromptWindow } from "./CapturePromptWindow";

const apiMocks = vi.hoisted(() => ({
  getCapturePrompt: vi.fn(),
  respondCapturePrompt: vi.fn(),
  resizeCapturePrompt: vi.fn(),
  onCapturePromptUpdated: vi.fn(),
}));

vi.mock("../api", () => ({ api: apiMocks }));

describe("CapturePromptWindow", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    apiMocks.getCapturePrompt.mockResolvedValue({
      sessionId: "session-1",
      targetName: "腾讯会议",
      kind: "captureInterrupted",
    });
    apiMocks.respondCapturePrompt.mockResolvedValue({});
    apiMocks.resizeCapturePrompt.mockResolvedValue(undefined);
    apiMocks.onCapturePromptUpdated.mockResolvedValue(vi.fn());
  });

  it("requests a content-sized prompt window", async () => {
    render(<CapturePromptWindow />);

    expect(
      await screen.findByRole("heading", { name: "应用音频捕获已中断" }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "Nota 暂时无法继续获取“腾讯会议”的声音。你可以继续等待恢复，或停止并保存录音。",
      ),
    ).toBeInTheDocument();
    await waitFor(() => {
      expect(apiMocks.resizeCapturePrompt).toHaveBeenCalled();
    });
  });

  it("keeps recording when the user chooses continue", async () => {
    render(<CapturePromptWindow />);

    expect(await screen.findByText(/腾讯会议/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "继续等待" }));

    await waitFor(() => {
      expect(apiMocks.respondCapturePrompt).toHaveBeenCalledWith("session-1", false);
    });
  });

  it("requests the normal stop-and-save path only after confirmation", async () => {
    render(<CapturePromptWindow />);

    expect(await screen.findByText(/腾讯会议/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "停止并保存" }));

    await waitFor(() => {
      expect(apiMocks.respondCapturePrompt).toHaveBeenCalledWith("session-1", true);
    });
  });

  it("explains prolonged silence without claiming that the meeting ended", async () => {
    apiMocks.getCapturePrompt.mockResolvedValue({
      sessionId: "session-1",
      targetName: "腾讯会议",
      kind: "prolongedSilence",
    });

    render(<CapturePromptWindow />);

    expect(
      await screen.findByRole("heading", {
        name: "应用已持续一段时间没有声音",
      }),
    ).toBeInTheDocument();
    expect(screen.getByText(/连续 3 分钟未检测到/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "继续录音" })).toBeInTheDocument();
    expect(screen.queryByText(/会议已结束/)).not.toBeInTheDocument();
  });

  it("replaces an open silence reminder when capture becomes interrupted", async () => {
    let updatePrompt: ((prompt: CapturePrompt) => void) | undefined;
    apiMocks.getCapturePrompt.mockResolvedValue({
      sessionId: "session-1",
      targetName: "腾讯会议",
      kind: "prolongedSilence",
    });
    apiMocks.onCapturePromptUpdated.mockImplementation(async (handler) => {
      updatePrompt = handler;
      return vi.fn();
    });

    render(<CapturePromptWindow />);
    expect(
      await screen.findByRole("heading", {
        name: "应用已持续一段时间没有声音",
      }),
    ).toBeInTheDocument();

    act(() => {
      updatePrompt?.({
        sessionId: "session-1",
        targetName: "腾讯会议",
        kind: "captureInterrupted",
      });
    });

    expect(
      screen.getByRole("heading", { name: "应用音频捕获已中断" }),
    ).toBeInTheDocument();
  });
});
