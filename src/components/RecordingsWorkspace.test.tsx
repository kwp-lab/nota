import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AsrProviderKind, RecordingItem, TranscriptDocument } from "../types";
import "../styles.css";
import { RecordingsWorkspace } from "./RecordingsWorkspace";

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (path: string) => `asset://${path}`,
}));

const completed: RecordingItem = {
  id: "completed",
  title: "产品周会",
  path: "C:\\Recordings\\weekly.ogg",
  createdAt: "2026-07-28T04:00:00Z",
  durationMs: 62_000,
  sizeBytes: 1024 * 1024,
  recovered: false,
  transcription: {
    status: "completed",
    completedChunks: 1,
    totalChunks: 1,
    providerName: "Local FunASR",
    modelId: "sensevoice",
    speakerCount: null,
    errorMessage: null,
    hasText: true,
    protocol: "legacy_chunks",
    progressPhase: null,
    progressCurrent: 1,
    progressTotal: 1,
    progressUnit: "chunks",
  },
};

const failed: RecordingItem = {
  ...completed,
  id: "failed",
  title: "客户访谈",
  transcription: {
    ...completed.transcription!,
    status: "failed",
    completedChunks: 1,
    totalChunks: 3,
    errorMessage: "服务暂时不可用",
    hasText: false,
  },
};

const transcript: TranscriptDocument = {
  recordingId: completed.id,
  status: "completed",
  providerName: "Local FunASR",
  modelId: "sensevoice",
  language: "zh",
  text: "先确认本周目标。",
  segments: [
    {
      startMs: 12_000,
      endMs: 16_000,
      text: "先确认本周目标。",
      speaker: "speaker_1",
    },
  ],
  speakerNames: {},
  completedChunks: 1,
  totalChunks: 1,
  errorMessage: null,
  updatedAt: "2026-07-28T04:02:00Z",
};

const renderWorkspace = (
  items: RecordingItem[] = [completed, failed],
  selectedId: string | null = completed.id,
  document: TranscriptDocument | null = transcript,
  activeProviderKind: AsrProviderKind | null = "funAsr",
) => {
  const actions = {
    onSelect: vi.fn(),
    onReturnToRecorder: vi.fn(),
    onPreparePlayback: vi.fn(async () => completed.path),
    onPlaybackError: vi.fn(),
    onStartTranscription: vi.fn(),
    onResumeTranscription: vi.fn(),
    onCancelTranscription: vi.fn(),
    onCopyTranscript: vi.fn(),
    onExportTranscript: vi.fn(),
    onIdentifySpeakers: vi.fn(),
    onSaveSpeakerIdentification: vi.fn(),
    onDiscardSpeakerIdentification: vi.fn(),
    onReveal: vi.fn(),
    onDelete: vi.fn(),
    onRecover: vi.fn(),
    onDiscardRecovery: vi.fn(),
    onRename: vi.fn(),
    onPermanentDelete: vi.fn(),
  };
  render(
    <RecordingsWorkspace
      items={items}
      recoverable={[]}
      selectedId={selectedId}
      transcript={document}
      transcriptLoading={false}
      recordingActive={false}
      hasProvider
      activeProviderKind={activeProviderKind}
      hasVoiceprintProvider
      participants={[]}
      {...actions}
    />,
  );
  return actions;
};

afterEach(cleanup);

describe("RecordingsWorkspace", () => {
  it("filters recordings and exposes transcription states", async () => {
    const actions = renderWorkspace();
    await waitFor(() => expect(actions.onPreparePlayback).toHaveBeenCalled());
    expect(screen.getAllByText("已转写")).toHaveLength(2);
    expect(screen.getByText("转写失败")).toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: "搜索录音" }), {
      target: { value: "客户" },
    });
    expect(screen.queryByRole("button", { name: /产品周会/ })).not.toBeInTheDocument();
    expect(screen.getByText("客户访谈")).toBeInTheDocument();
  });

  it("shows provider speaker labels and timestamp controls without inventing roles", () => {
    renderWorkspace();
    expect(screen.getByText("speaker_1")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "0:12" })).toBeInTheDocument();
    expect(screen.queryByText("我")).not.toBeInTheDocument();
    expect(screen.queryByText("参会者")).not.toBeInTheDocument();
  });

  it("renders confirmed participant names without changing the raw segment", () => {
    renderWorkspace(undefined, undefined, {
      ...transcript,
      speakerNames: { speaker_1: "小明" },
    });

    expect(screen.getByText("小明")).toBeInTheDocument();
    expect(screen.queryByText("speaker_1")).not.toBeInTheDocument();
    expect(transcript.segments[0].speaker).toBe("speaker_1");
  });

  it("offers copy, export, and resume actions for their respective states", () => {
    const completedActions = renderWorkspace();
    fireEvent.click(screen.getByRole("button", { name: "复制全文" }));
    fireEvent.click(screen.getByRole("button", { name: "导出 TXT" }));
    expect(completedActions.onCopyTranscript).toHaveBeenCalledWith(completed.id);
    expect(completedActions.onExportTranscript).toHaveBeenCalledWith(completed.id, completed.title);
    cleanup();

    const failedActions = renderWorkspace([failed], failed.id, null);
    fireEvent.click(screen.getByRole("button", { name: "继续转写" }));
    expect(failedActions.onResumeTranscription).toHaveBeenCalledWith(failed.id);
    expect(screen.getByText("服务暂时不可用")).toBeInTheDocument();
  });

  it("asks for FunASR speaker options before manual retranscription", () => {
    const actions = renderWorkspace();
    fireEvent.click(screen.getByRole("button", { name: "重新转写" }));

    expect(screen.getByRole("dialog", { name: "重新转写" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("radio", { name: /指定人数/ }));
    fireEvent.change(screen.getByRole("spinbutton", { name: "说话人数" }), {
      target: { value: "3" },
    });
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", {
      name: "重新转写",
    }));

    expect(actions.onStartTranscription).toHaveBeenCalledWith(completed.id, 3);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("keeps OpenAI-compatible retranscription as a one-click action", () => {
    const actions = renderWorkspace(undefined, undefined, undefined, "openAiCompatible");
    fireEvent.click(screen.getByRole("button", { name: "重新转写" }));

    expect(actions.onStartTranscription).toHaveBeenCalledWith(completed.id, null);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("shows the snapshotted speaker-count mode for batch jobs", () => {
    const specified: RecordingItem = {
      ...completed,
      transcription: {
        ...completed.transcription!,
        protocol: "nota_batch_v1",
        speakerCount: 3,
      },
    };
    renderWorkspace([specified], specified.id);
    expect(screen.getByText(/指定 3 人/)).toBeInTheDocument();
  });

  it("shows whole-meeting upload and server processing progress for FunASR", () => {
    const uploading: RecordingItem = {
      ...completed,
      id: "uploading",
      transcription: {
        ...completed.transcription!,
        status: "preparing",
        protocol: "nota_batch_v1",
        progressPhase: "uploading",
        progressCurrent: 4 * 1024 * 1024,
        progressTotal: 8 * 1024 * 1024,
        progressUnit: "bytes",
      },
    };
    renderWorkspace([uploading], uploading.id, null);
    expect(screen.getAllByText("上传录音")).toHaveLength(2);
    expect(screen.getByText("已上传 4.0 MB / 8.0 MB")).toBeInTheDocument();
    expect(screen.getByText("上传录音 50%")).toBeInTheDocument();
    cleanup();

    const diarizing: RecordingItem = {
      ...uploading,
      id: "diarizing",
      transcription: {
        ...uploading.transcription!,
        status: "transcribing",
        progressPhase: "diarizing",
        progressCurrent: 12,
        progressTotal: 12,
        progressUnit: "windows",
      },
    };
    renderWorkspace([diarizing], diarizing.id, null);
    expect(screen.getAllByText("统一说话人")).toHaveLength(2);
    expect(screen.getByText("已处理 12 / 12 个音频窗口")).toBeInTheDocument();
  });

  it("keeps a long recording list in its own vertical scroll area", () => {
    renderWorkspace(
      Array.from({ length: 30 }, (_, index) => ({
        ...completed,
        id: `recording-${index}`,
        title: `会议录音 ${index + 1}`,
      })),
      null,
      null,
    );

    const list = screen.getByLabelText("录音列表");
    expect(getComputedStyle(list).overflowY).toBe("auto");
    expect(list.closest(".history-pane")).toHaveClass("history-pane");
  });

  it("replaces the native row context menu with the shared recording actions", async () => {
    const actions = renderWorkspace();
    await waitFor(() => expect(actions.onPreparePlayback).toHaveBeenCalled());
    const row = within(screen.getByLabelText("录音列表"))
      .getByText("产品周会")
      .closest("article");
    expect(row).not.toBeNull();

    const contextMenuEvent = new MouseEvent("contextmenu", {
      bubbles: true,
      cancelable: true,
      clientX: 160,
      clientY: 120,
    });
    fireEvent(row!, contextMenuEvent);

    expect(contextMenuEvent.defaultPrevented).toBe(true);
    expect(actions.onSelect).not.toHaveBeenCalled();
    const menu = screen.getByRole("menu", { name: "产品周会 操作" });
    expect(within(menu).getAllByRole("menuitem").map((item) => item.textContent)).toEqual([
      "重命名",
      "移至回收站",
      "永久删除",
    ]);

    fireEvent.click(within(menu).getByRole("menuitem", { name: "重命名" }));
    expect(actions.onRename).toHaveBeenCalledWith(completed.id, completed.title);
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("uses the same controlled menu in details and closes it before deletion", () => {
    const actions = renderWorkspace();
    fireEvent.click(screen.getByRole("button", { name: "更多" }));

    const menu = screen.getByRole("menu", { name: "产品周会 操作" });
    fireEvent.click(within(menu).getByRole("menuitem", { name: "永久删除" }));

    expect(actions.onPermanentDelete).toHaveBeenCalledWith(completed.id);
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });
});
