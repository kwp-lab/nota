import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { MeetingEndPromptWindow } from "./MeetingEndPromptWindow";

const apiMocks = vi.hoisted(() => ({
  getMeetingEndPrompt: vi.fn(),
  respondMeetingEndPrompt: vi.fn(),
  resizeMeetingEndPrompt: vi.fn(),
}));

vi.mock("../api", () => ({ api: apiMocks }));

describe("MeetingEndPromptWindow", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    apiMocks.getMeetingEndPrompt.mockResolvedValue({
      sessionId: "session-1",
      targetName: "腾讯会议",
    });
    apiMocks.respondMeetingEndPrompt.mockResolvedValue({});
    apiMocks.resizeMeetingEndPrompt.mockResolvedValue(undefined);
  });

  it("requests a content-sized prompt window", async () => {
    render(<MeetingEndPromptWindow />);

    await waitFor(() => {
      expect(apiMocks.resizeMeetingEndPrompt).toHaveBeenCalled();
    });
  });

  it("keeps recording when the user chooses continue", async () => {
    render(<MeetingEndPromptWindow />);

    expect(await screen.findByText(/腾讯会议/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "继续录音" }));

    await waitFor(() => {
      expect(apiMocks.respondMeetingEndPrompt).toHaveBeenCalledWith("session-1", false);
    });
  });

  it("requests the normal stop-and-save path only after confirmation", async () => {
    render(<MeetingEndPromptWindow />);

    expect(await screen.findByText(/腾讯会议/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "停止并保存" }));

    await waitFor(() => {
      expect(apiMocks.respondMeetingEndPrompt).toHaveBeenCalledWith("session-1", true);
    });
  });
});
