import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type {
  AsrProviderKind,
  ParticipantProfile,
  RecordingItem,
  SpeakerIdentificationSession,
  TranscriptDocument,
} from "../types";
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
  speakerAssignments: {},
  completedChunks: 1,
  totalChunks: 1,
  errorMessage: null,
  updatedAt: "2026-07-28T04:02:00Z",
};

const participantProfiles: ParticipantProfile[] = [{
  id: "participant-1",
  displayName: "小明",
  createdAt: "2026-08-05T00:00:00Z",
  updatedAt: "2026-08-05T00:00:00Z",
  samples: [],
}, {
  id: "participant-2",
  displayName: "小红",
  createdAt: "2026-08-05T00:00:00Z",
  updatedAt: "2026-08-05T00:00:00Z",
  samples: [],
}];

const identificationSession: SpeakerIdentificationSession = {
  id: "speaker-session",
  recordingId: completed.id,
  speakerCount: 1,
  voiceprintCount: 0,
  candidates: [{
    rawSpeaker: "speaker_1",
    totalSpeechMs: 4_000,
    previewStartMs: 12_000,
    previewEndMs: 16_000,
    embeddingExtracted: false,
    sampleStatus: "preview_only",
    statusMessage: "可试听并手动标记姓名",
    errorMessage: null,
    suggestedParticipantId: null,
    suggestedParticipantName: null,
    matchScore: null,
  }],
};

const renderWorkspace = (
  items: RecordingItem[] = [completed, failed],
  selectedId: string | null = completed.id,
  document: TranscriptDocument | null = transcript,
  activeProviderKind: AsrProviderKind | null = "funAsr",
  participants: ParticipantProfile[] = [],
  hasVoiceprintProvider = true,
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
    onIdentifySpeakers: vi.fn<() => Promise<SpeakerIdentificationSession>>(),
    onSaveSpeakerIdentification: vi.fn(async () => undefined),
    onUpdateSpeakerAssignments: vi.fn(async () => undefined),
    onDiscardSpeakerIdentification: vi.fn(),
    onOpenVoiceprintSettings: vi.fn(),
    onReveal: vi.fn(),
    onDelete: vi.fn(),
    onRecover: vi.fn(),
    onDiscardRecovery: vi.fn(),
    onRename: vi.fn(),
    onPermanentDelete: vi.fn(),
    onAiMessage: vi.fn(),
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
      hasVoiceprintProvider={hasVoiceprintProvider}
      llmProviders={[]}
      activeLlmProviderId={null}
      participants={participants}
      {...actions}
    />,
  );
  return actions;
};

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

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
    expect(screen.getByRole("button", { name: "00:00:12" })).toBeInTheDocument();
    expect(screen.queryByText("我")).not.toBeInTheDocument();
    expect(screen.queryByText("参会者")).not.toBeInTheDocument();
  });

  it("renders transcript timestamps as zero-padded hours, minutes, and seconds", () => {
    renderWorkspace(undefined, undefined, {
      ...transcript,
      segments: [
        { startMs: 0, endMs: 1_000, text: "first", speaker: "speaker_0" },
        { startMs: 62_000, endMs: 63_000, text: "second", speaker: "speaker_0" },
        { startMs: 3_723_999, endMs: 3_724_999, text: "third", speaker: "speaker_0" },
      ],
    });

    expect(screen.getByRole("button", { name: "00:00:00" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "00:01:02" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "01:02:03" })).toBeInTheDocument();
  });

  it("keeps the player and transcription toolbar in one sticky control region", async () => {
    const actions = renderWorkspace();
    await waitFor(() => expect(actions.onPreparePlayback).toHaveBeenCalled());

    const sticky = document.querySelector(".record-detail-sticky-controls");
    expect(sticky).not.toBeNull();
    expect(sticky?.querySelector("audio")).not.toBeNull();
    expect(sticky?.querySelector(".transcript-toolbar")).not.toBeNull();
    expect(sticky?.querySelector(".transcript-body")).toBeNull();
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

  it("opens speaker management without analysis and starts it only after an explicit click", async () => {
    let resolveAnalysis!: (session: SpeakerIdentificationSession) => void;
    const actions = renderWorkspace();
    actions.onIdentifySpeakers.mockReturnValue(new Promise((resolve) => {
      resolveAnalysis = resolve;
    }));

    fireEvent.click(screen.getByRole("button", { name: "说话人识别" }));

    expect(screen.getByRole("dialog", { name: "管理说话人" })).toBeInTheDocument();
    expect(screen.getByText(/声纹分析是可选功能/)).toBeInTheDocument();
    expect(actions.onIdentifySpeakers).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "开始声纹分析" }));
    expect(screen.getByText(/正在分析声纹/)).toBeInTheDocument();
    expect(actions.onIdentifySpeakers).toHaveBeenCalledOnce();
    resolveAnalysis(identificationSession);
    await waitFor(() => expect(screen.getByText(/声纹分析完成/)).toBeInTheDocument());
  });

  it("keeps meeting-local speaker management available without a voiceprint server", () => {
    const actions = renderWorkspace(
      undefined,
      undefined,
      undefined,
      "funAsr",
      [],
      false,
    );

    fireEvent.click(screen.getByRole("button", { name: "说话人识别" }));

    expect(screen.getByRole("dialog", { name: "管理说话人" })).toBeInTheDocument();
    expect(screen.getByText(/可直接手动设置姓名/)).toBeInTheDocument();
    expect(actions.onIdentifySpeakers).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "前往声纹管理" }));
    expect(actions.onOpenVoiceprintSettings).toHaveBeenCalledOnce();
    expect(screen.queryByRole("dialog", { name: "管理说话人" })).not.toBeInTheDocument();
  });

  it("edits a confirmed speaker from its transcript label without calling the server", async () => {
    const assignedTranscript: TranscriptDocument = {
      ...transcript,
      speakerNames: { speaker_1: "小明" },
      speakerAssignments: {
        speaker_1: { participantId: "participant-1", displayName: "小明" },
      },
    };
    const actions = renderWorkspace(
      undefined,
      undefined,
      assignedTranscript,
      "funAsr",
      participantProfiles,
    );

    fireEvent.click(screen.getByRole("button", { name: "小明" }));

    expect(actions.onIdentifySpeakers).not.toHaveBeenCalled();
    expect(screen.getByText(/声纹分析是可选功能/)).toBeInTheDocument();
    fireEvent.change(screen.getByRole("combobox", { name: "speaker_1 真实姓名" }), {
      target: { value: "participant-2" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存姓名更改" }));
    await waitFor(() => expect(actions.onUpdateSpeakerAssignments).toHaveBeenCalledWith(
      completed.id,
      [{
        rawSpeaker: "speaker_1",
        participantId: "participant-2",
        newDisplayName: null,
      }],
    ));
  });

  it("discards a voiceprint session that arrives after the dialog closes", async () => {
    let resolveAnalysis!: (session: SpeakerIdentificationSession) => void;
    const actions = renderWorkspace();
    actions.onIdentifySpeakers.mockReturnValue(new Promise((resolve) => {
      resolveAnalysis = resolve;
    }));
    fireEvent.click(screen.getByRole("button", { name: "说话人识别" }));
    fireEvent.click(screen.getByRole("button", { name: "开始声纹分析" }));
    fireEvent.click(screen.getByRole("button", { name: "稍后继续" }));

    resolveAnalysis(identificationSession);
    await waitFor(() => expect(actions.onDiscardSpeakerIdentification)
      .toHaveBeenCalledWith("speaker-session"));
    expect(screen.queryByRole("dialog", { name: "管理说话人" })).not.toBeInTheDocument();
  });

  it("saves meeting names locally unless voiceprint enrollment is explicitly enabled", async () => {
    const enrollableSession: SpeakerIdentificationSession = {
      ...identificationSession,
      voiceprintCount: 1,
      candidates: [{
        ...identificationSession.candidates[0],
        embeddingExtracted: true,
        sampleStatus: "enrollable",
      }],
    };
    const actions = renderWorkspace(
      undefined,
      undefined,
      transcript,
      "funAsr",
      participantProfiles,
    );
    actions.onIdentifySpeakers.mockResolvedValue(enrollableSession);

    fireEvent.click(screen.getByRole("button", { name: "说话人识别" }));
    fireEvent.click(screen.getByRole("button", { name: "开始声纹分析" }));
    await screen.findByText(/声纹分析完成/);
    fireEvent.change(screen.getByRole("combobox", { name: "speaker_1 真实姓名" }), {
      target: { value: "participant-1" },
    });
    expect(screen.getByRole("checkbox", { name: /同时保存 1 份可用声纹/ })).not.toBeChecked();
    fireEvent.click(screen.getByRole("button", { name: "保存姓名更改" }));

    await waitFor(() => expect(actions.onUpdateSpeakerAssignments).toHaveBeenCalledWith(
      completed.id,
      [{
        rawSpeaker: "speaker_1",
        participantId: "participant-1",
        newDisplayName: null,
      }],
    ));
    expect(actions.onSaveSpeakerIdentification).not.toHaveBeenCalled();
    expect(actions.onDiscardSpeakerIdentification).toHaveBeenCalledWith("speaker-session");
  });

  it("enrolls available voiceprints only after the optional checkbox is selected", async () => {
    const enrollableSession: SpeakerIdentificationSession = {
      ...identificationSession,
      voiceprintCount: 1,
      candidates: [{
        ...identificationSession.candidates[0],
        embeddingExtracted: true,
        sampleStatus: "enrollable",
      }],
    };
    const actions = renderWorkspace(
      undefined,
      undefined,
      transcript,
      "funAsr",
      participantProfiles,
    );
    actions.onIdentifySpeakers.mockResolvedValue(enrollableSession);

    fireEvent.click(screen.getByRole("button", { name: "说话人识别" }));
    fireEvent.click(screen.getByRole("button", { name: "开始声纹分析" }));
    await screen.findByText(/声纹分析完成/);
    fireEvent.change(screen.getByRole("combobox", { name: "speaker_1 真实姓名" }), {
      target: { value: "participant-1" },
    });
    fireEvent.click(screen.getByRole("checkbox", { name: /同时保存 1 份可用声纹/ }));
    fireEvent.click(screen.getByRole("button", { name: "保存姓名与 1 份声纹" }));

    await waitFor(() => expect(actions.onSaveSpeakerIdentification).toHaveBeenCalledWith(
      "speaker-session",
      [{
        rawSpeaker: "speaker_1",
        participantId: "participant-1",
        newDisplayName: null,
      }],
    ));
    expect(actions.onUpdateSpeakerAssignments).not.toHaveBeenCalled();
  });

  it("reflects clean-preview play and pause state in the modal", async () => {
    const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
    const pause = vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => undefined);
    const actions = renderWorkspace();
    actions.onIdentifySpeakers.mockResolvedValue(identificationSession);
    await waitFor(() => expect(actions.onPreparePlayback).toHaveBeenCalled());

    fireEvent.click(screen.getByRole("button", { name: "说话人识别" }));
    fireEvent.click(screen.getByRole("button", { name: "开始声纹分析" }));
    await screen.findByText(/声纹分析完成/);
    fireEvent.click(screen.getByRole("button", { name: "纯净试听" }));
    const audio = document.querySelector("audio")!;
    fireEvent.play(audio);

    expect(play).toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "暂停纯净试听" }));
    expect(pause).toHaveBeenCalled();
    fireEvent.pause(audio);
    expect(screen.getByRole("button", { name: "纯净试听" })).toBeInTheDocument();
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
    fireEvent.click(screen.getByRole("radio", { name: /指定目标人数/ }));
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
    expect(screen.getByText(/目标 3 人（安全优先）/)).toBeInTheDocument();
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
      "打开所在文件夹",
      "重命名",
      "移至回收站",
      "永久删除",
    ]);

    fireEvent.click(within(menu).getByRole("menuitem", { name: "打开所在文件夹" }));
    expect(actions.onReveal).toHaveBeenCalledWith(completed.id);
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();

    fireEvent(row!, new MouseEvent("contextmenu", {
      bubbles: true,
      cancelable: true,
      clientX: 160,
      clientY: 120,
    }));
    fireEvent.click(screen.getByRole("menuitem", { name: "重命名" }));
    expect(actions.onRename).toHaveBeenCalledWith(completed.id, completed.title);
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("uses the same controlled menu in details and closes it before deletion", () => {
    const actions = renderWorkspace();
    fireEvent.click(screen.getByRole("button", { name: "更多录音操作" }));

    const menu = screen.getByRole("menu", { name: "产品周会 操作" });
    expect(within(menu).queryByRole("menuitem", { name: "打开所在文件夹" })).not.toBeInTheDocument();
    fireEvent.click(within(menu).getByRole("menuitem", { name: "永久删除" }));

    expect(actions.onPermanentDelete).toHaveBeenCalledWith(completed.id);
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });
});
