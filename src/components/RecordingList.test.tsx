import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RecordingList } from "./RecordingList";
import type { RecordingItem } from "../types";

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (path: string) => `asset://${path}`,
}));

class FakeAudio {
  src: string;
  preload = "";
  paused = true;
  onplay: (() => void) | null = null;
  onpause: (() => void) | null = null;
  onended: (() => void) | null = null;
  onerror: (() => void) | null = null;

  constructor(src: string) {
    this.src = src;
  }

  async play() {
    this.paused = false;
    this.onplay?.();
  }

  pause() {
    this.paused = true;
    this.onpause?.();
  }
}

const item: RecordingItem = {
  id: "recording-1",
  title: "测试录音",
  path: "F:\\Recordings\\test.ogg",
  createdAt: new Date().toISOString(),
  durationMs: 10_000,
  sizeBytes: 1024,
  recovered: false,
};

describe("RecordingList playback", () => {
  beforeEach(() => {
    vi.stubGlobal("Audio", FakeAudio);
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it("prepares, plays, and pauses an indexed recording", async () => {
    const prepare = vi.fn(async () => item.path);
    render(
      <RecordingList
        items={[item]}
        recoverable={[]}
        onPreparePlayback={prepare}
        onPlaybackError={vi.fn()}
        onReveal={vi.fn()}
        onDelete={vi.fn()}
        onRecover={vi.fn()}
        onDiscardRecovery={vi.fn()}
        onRename={vi.fn()}
        onPermanentDelete={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "播放 测试录音" }));
    await waitFor(() => expect(prepare).toHaveBeenCalledWith(item.id));
    expect(await screen.findByRole("button", { name: "暂停 测试录音" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "暂停 测试录音" }));
    expect(await screen.findByRole("button", { name: "播放 测试录音" })).toBeInTheDocument();
  });
});
