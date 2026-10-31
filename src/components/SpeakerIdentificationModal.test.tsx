import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ParticipantProfile, SpeakerIdentificationSession } from "../types";
import {
  SpeakerIdentificationModal,
  type SpeakerManagementSpeaker,
} from "./SpeakerIdentificationModal";

afterEach(cleanup);

const participants: ParticipantProfile[] = [{
  id: "confirmed",
  displayName: "小明",
  createdAt: "2026-08-02T00:00:00Z",
  updatedAt: "2026-08-02T00:00:00Z",
  samples: [],
}, {
  id: "suggested",
  displayName: "小红",
  createdAt: "2026-08-02T00:00:00Z",
  updatedAt: "2026-08-02T00:00:00Z",
  samples: [],
}];

const speakers: SpeakerManagementSpeaker[] = [{
  rawSpeaker: "speaker_0",
  currentParticipantId: "confirmed",
  currentDisplayName: "小明",
  totalSpeechMs: 12_000,
  utterances: [
    { startMs: 2_000, endMs: 7_000, text: "先确认一下本周目标。" },
    { startMs: 20_000, endMs: 26_000, text: "然后我们再看风险。" },
  ],
}];

const session: SpeakerIdentificationSession = {
  id: "session",
  recordingId: "recording",
  speakerCount: 1,
  voiceprintCount: 1,
  candidates: [{
    rawSpeaker: "speaker_0",
    totalSpeechMs: 12_000,
    previewStartMs: 2_000,
    previewEndMs: 9_000,
    embeddingExtracted: true,
    sampleStatus: "enrollable",
    statusMessage: "已找到满足入库标准的纯净单人声音",
    errorMessage: null,
    suggestedParticipantId: "suggested",
    suggestedParticipantName: "小红",
    matchScore: 0.91,
  }],
};

const defaultProps = () => ({
  speakers, session: null, analysisStatus: "idle" as const, analysisError: null,
  participants, initialSpeaker: null, saving: false,
  voiceprintAvailability: { kind: "available" } as const,
  activePreviewId: null, previewPlaying: false,
  onAnalyze: vi.fn(), onConfigureVoiceprints: vi.fn(), onPreview: vi.fn(),
  onStopPreview: vi.fn(), onCancel: vi.fn(), onSave: vi.fn(),
});

describe("SpeakerIdentificationModal", () => {
  it("defaults to a persistent meeting-local name and does not let late suggestions replace it", () => {
    const props = defaultProps();
    const localSpeaker = { ...speakers[0], currentParticipantId: null, currentDisplayName: "客户采购负责人" };
    const { rerender } = render(<SpeakerIdentificationModal {...props} speakers={[localSpeaker]} />);
    expect(screen.getByRole("textbox", { name: "speaker_0 本场显示名称" })).toHaveValue("客户采购负责人");
    expect(screen.getByText(/重启后保留，不加入声纹库/)).toBeInTheDocument();
    rerender(<SpeakerIdentificationModal {...props} speakers={[localSpeaker]} session={session} analysisStatus="ready" />);
    expect(screen.getByRole("textbox", { name: "speaker_0 本场显示名称" })).toHaveValue("客户采购负责人");
    expect(screen.queryByText("建议：小红")).not.toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: "speaker_0 本场显示名称" }), { target: { value: "张经理" } });
    fireEvent.click(screen.getByRole("button", { name: "保存姓名更改" }));
    expect(props.onSave).toHaveBeenCalledWith({
      mappingAssignments: [{ rawSpeaker: "speaker_0", participantId: null, newDisplayName: "张经理" }],
      sessionAssignments: [{ rawSpeaker: "speaker_0", participantId: null, newDisplayName: "张经理" }],
      saveVoiceprints: false,
    });
  });

  it("edits an existing person's name locally without renaming their global profile", () => {
    const props = defaultProps();
    render(<SpeakerIdentificationModal {...props} />);
    fireEvent.change(screen.getByRole("textbox", { name: "speaker_0 本场显示名称" }), { target: { value: "本场称呼" } });
    fireEvent.click(screen.getByRole("button", { name: "保存姓名更改" }));
    expect(props.onSave.mock.calls[0][0].mappingAssignments).toEqual([
      { rawSpeaker: "speaker_0", participantId: null, newDisplayName: "本场称呼" },
    ]);
  });

  it("enrolls only the individually checked speaker and resets consent when identity changes", () => {
    const props = defaultProps();
    const second = { ...speakers[0], rawSpeaker: "speaker_1", currentParticipantId: null, currentDisplayName: null };
    render(<SpeakerIdentificationModal {...props} speakers={[speakers[0], second]}
      initialSpeaker="speaker_0" session={{ ...session, candidates: [session.candidates[0], { ...session.candidates[0], rawSpeaker: "speaker_1" }] }} analysisStatus="ready" />);
    fireEvent.click(screen.getByRole("checkbox", { name: "speaker_0 保存声纹供以后会议识别" }));
    fireEvent.click(within(screen.getByRole("navigation", { name: "会议说话人" })).getAllByRole("button")[0]);
    fireEvent.change(screen.getByRole("textbox", { name: "speaker_1 本场显示名称" }), { target: { value: "客户" } });
    expect(screen.getByRole("checkbox", { name: "speaker_1 保存声纹供以后会议识别" })).not.toBeChecked();
    fireEvent.click(screen.getByRole("button", { name: "保存姓名与 1 份声纹" }));
    const request = props.onSave.mock.calls[0][0];
    expect(request.sessionAssignments).toEqual(expect.arrayContaining([
      { rawSpeaker: "speaker_0", participantId: "confirmed", newDisplayName: null, saveVoiceprint: true },
      { rawSpeaker: "speaker_1", participantId: null, newDisplayName: "客户" },
    ]));
    fireEvent.click(screen.getByRole("checkbox", { name: "speaker_1 保存声纹供以后会议识别" }));
    fireEvent.change(screen.getByRole("textbox", { name: "speaker_1 本场显示名称" }), { target: { value: "另一位客户" } });
    expect(screen.getByRole("checkbox", { name: "speaker_1 保存声纹供以后会议识别" })).not.toBeChecked();
  });

  it("keeps manual naming but hides voiceprint actions for a DashScope generation", () => {
    render(
      <SpeakerIdentificationModal
        speakers={speakers}
        session={null}
        analysisStatus="idle"
        analysisError={null}
        participants={participants}
        initialSpeaker={null}
        saving={false}
        voiceprintAvailability={{
          kind: "transcriptionProviderUnsupported",
          providerName: "千问云转写",
        }}
        activePreviewId={null}
        previewPlaying={false}
        onAnalyze={vi.fn()}
        onConfigureVoiceprints={vi.fn()}
        onPreview={vi.fn()}
        onStopPreview={vi.fn()}
        onCancel={vi.fn()}
        onSave={vi.fn()}
      />,
    );
    expect(screen.getByText(/支持区分匿名说话人，但不支持 Nota 声纹分析/))
      .toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "speaker_0 本场显示名称" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "声纹分析不可用" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "前往声纹管理" })).not.toBeInTheDocument();
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
  });

  it("keeps a confirmed assignment ahead of a new voiceprint suggestion", () => {
    const onPreview = vi.fn();
    const onSave = vi.fn();
    render(
      <SpeakerIdentificationModal
        speakers={speakers}
        session={session}
        analysisStatus="ready"
        analysisError={null}
        participants={participants}
        initialSpeaker={null}
        saving={false}
        voiceprintAvailability={{ kind: "available" }}
        activePreviewId={null}
        previewPlaying={false}
        onAnalyze={vi.fn()}
        onConfigureVoiceprints={vi.fn()}
        onPreview={onPreview}
        onStopPreview={vi.fn()}
        onCancel={vi.fn()}
        onSave={onSave}
      />,
    );

    expect(screen.getByRole("combobox", { name: "speaker_0 关联已有人员" })).toHaveValue("confirmed");
    expect(screen.queryByText("建议：小红")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "重新分析" }))
      .toHaveClass("button", "secondary", "speaker-analysis-action");
    fireEvent.click(screen.getByRole("button", { name: "纯净试听" }));
    expect(onPreview).toHaveBeenCalledWith({
      id: "clean:speaker_0:2000-9000",
      startMs: 2_000,
      endMs: 9_000,
    });
    const saveVoiceprints = screen.getByRole("checkbox", { name: "speaker_0 保存声纹供以后会议识别" });
    expect(saveVoiceprints).not.toBeChecked();
    fireEvent.click(saveVoiceprints);
    fireEvent.click(screen.getByRole("button", { name: "保存 1 份声纹" }));
    expect(onSave).toHaveBeenCalledWith({
      mappingAssignments: [],
      sessionAssignments: [{
        rawSpeaker: "speaker_0",
        participantId: "confirmed",
        newDisplayName: null,
        saveVoiceprint: true,
      }],
      saveVoiceprints: true,
    });
  });

  it("offers multiple transcript utterances when a reusable voiceprint is unavailable", () => {
    const onPreview = vi.fn();
    const onSave = vi.fn();
    render(
      <SpeakerIdentificationModal
        speakers={[{ ...speakers[0], currentParticipantId: null, currentDisplayName: null }]}
        session={{
          ...session,
          voiceprintCount: 0,
          candidates: [{
            ...session.candidates[0],
            embeddingExtracted: false,
            sampleStatus: "preview_only",
            suggestedParticipantId: null,
            suggestedParticipantName: null,
            matchScore: null,
          }],
        }}
        analysisStatus="ready"
        analysisError={null}
        participants={[]}
        initialSpeaker="speaker_0"
        saving={false}
        voiceprintAvailability={{ kind: "available" }}
        activePreviewId={null}
        previewPlaying={false}
        onAnalyze={vi.fn()}
        onConfigureVoiceprints={vi.fn()}
        onPreview={onPreview}
        onStopPreview={vi.fn()}
        onCancel={vi.fn()}
        onSave={onSave}
      />,
    );

    const utterances = screen.getByRole("region", { name: "speaker_0 代表发言" });
    expect(within(utterances).getAllByRole("button")).toHaveLength(2);
    fireEvent.click(within(utterances).getByRole("button", { name: "试听 00:00:20 的发言" }));
    expect(onPreview).toHaveBeenCalledWith({
      id: "utterance:speaker_0:20000-26000",
      startMs: 20_000,
      endMs: 26_000,
    });
    fireEvent.change(screen.getByRole("textbox", { name: "speaker_0 本场显示名称" }), {
      target: { value: "小绿" },
    });
    expect(screen.getByRole("checkbox", { name: "speaker_0 保存声纹供以后会议识别" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "保存姓名更改" }));
    expect(onSave).toHaveBeenCalledWith({
      mappingAssignments: [{
        rawSpeaker: "speaker_0",
        participantId: null,
        newDisplayName: "小绿",
      }],
      sessionAssignments: [{
        rawSpeaker: "speaker_0",
        participantId: null,
        newDisplayName: "小绿",
      }],
      saveVoiceprints: false,
    });
  });

  it("allows meeting-local naming while voiceprint analysis is still running", () => {
    const onSave = vi.fn();
    render(
      <SpeakerIdentificationModal
        speakers={[{ ...speakers[0], currentParticipantId: null, currentDisplayName: null }]}
        session={null}
        analysisStatus="loading"
        analysisError={null}
        participants={participants}
        initialSpeaker={null}
        saving={false}
        voiceprintAvailability={{ kind: "available" }}
        activePreviewId={null}
        previewPlaying={false}
        onAnalyze={vi.fn()}
        onConfigureVoiceprints={vi.fn()}
        onPreview={vi.fn()}
        onStopPreview={vi.fn()}
        onCancel={vi.fn()}
        onSave={onSave}
      />,
    );

    expect(screen.getByText(/正在分析声纹/)).toBeInTheDocument();
    fireEvent.click(screen.getByText("从声纹库选择已有人员"));
    fireEvent.change(screen.getByRole("combobox", { name: "speaker_0 关联已有人员" }), {
      target: { value: "confirmed" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存姓名更改" }));
    expect(onSave).toHaveBeenCalledWith({
      mappingAssignments: [{
        rawSpeaker: "speaker_0",
        participantId: "confirmed",
        newDisplayName: null,
      }],
      sessionAssignments: [{
        rawSpeaker: "speaker_0",
        participantId: "confirmed",
        newDisplayName: null,
      }],
      saveVoiceprints: false,
    });
  });

  it("orders unresolved speakers first and does not overwrite a dirty choice with a late suggestion", () => {
    const unresolved = {
      ...speakers[0],
      rawSpeaker: "speaker_1",
      currentParticipantId: null,
      currentDisplayName: null,
    };
    const common = {
      speakers: [speakers[0], unresolved],
      analysisError: null,
      participants,
      initialSpeaker: "speaker_1",
      saving: false,
      voiceprintAvailability: { kind: "available" } as const,
      activePreviewId: null,
      previewPlaying: false,
      onAnalyze: vi.fn(),
      onConfigureVoiceprints: vi.fn(),
      onPreview: vi.fn(),
      onStopPreview: vi.fn(),
      onCancel: vi.fn(),
      onSave: vi.fn(),
    };
    const { rerender } = render(
      <SpeakerIdentificationModal
        {...common}
        session={null}
        analysisStatus="loading"
      />,
    );
    const speakerList = screen.getByRole("navigation", { name: "会议说话人" });
    expect(within(speakerList).getAllByRole("button")[0]).toHaveTextContent("speaker_1");
    fireEvent.click(screen.getByText("从声纹库选择已有人员"));
    fireEvent.change(screen.getByRole("combobox", { name: "speaker_1 关联已有人员" }), {
      target: { value: "confirmed" },
    });

    rerender(
      <SpeakerIdentificationModal
        {...common}
        session={{
          ...session,
          candidates: [{
            ...session.candidates[0],
            rawSpeaker: "speaker_1",
            suggestedParticipantId: "suggested",
            suggestedParticipantName: "小红",
          }],
        }}
        analysisStatus="ready"
      />,
    );

    expect(screen.getByRole("combobox", { name: "speaker_1 关联已有人员" })).toHaveValue("confirmed");
  });

  it("shows which clean or representative preview is currently playing", () => {
    const common = {
      speakers,
      session,
      analysisStatus: "ready" as const,
      analysisError: null,
      participants,
      initialSpeaker: null,
      saving: false,
      voiceprintAvailability: { kind: "available" } as const,
      previewPlaying: true,
      onAnalyze: vi.fn(),
      onConfigureVoiceprints: vi.fn(),
      onPreview: vi.fn(),
      onStopPreview: vi.fn(),
      onCancel: vi.fn(),
      onSave: vi.fn(),
    };
    const { rerender } = render(
      <SpeakerIdentificationModal
        {...common}
        activePreviewId="clean:speaker_0:2000-9000"
      />,
    );

    const cleanPreview = screen.getByRole("button", { name: "暂停纯净试听" });
    expect(cleanPreview).toHaveClass("is-playing");
    expect(cleanPreview).toHaveAttribute("aria-pressed", "true");
    expect(cleanPreview).toHaveTextContent("试听中…");

    rerender(
      <SpeakerIdentificationModal
        {...common}
        activePreviewId="utterance:speaker_0:20000-26000"
      />,
    );
    const utterance = screen.getByRole("button", { name: "暂停 00:00:20 的发言" });
    expect(utterance).toHaveClass("is-playing");
    expect(utterance.closest("article")).toHaveClass("is-playing");
  });

  it("keeps analysis unavailable until a voiceprint service is selected", () => {
    const onAnalyze = vi.fn();
    const onConfigureVoiceprints = vi.fn();
    render(
      <SpeakerIdentificationModal
        speakers={speakers}
        session={null}
        analysisStatus="idle"
        analysisError={null}
        participants={participants}
        initialSpeaker={null}
        saving={false}
        voiceprintAvailability={{ kind: "providerNotConfigured" }}
        activePreviewId={null}
        previewPlaying={false}
        onAnalyze={onAnalyze}
        onConfigureVoiceprints={onConfigureVoiceprints}
        onPreview={vi.fn()}
        onStopPreview={vi.fn()}
        onCancel={vi.fn()}
        onSave={vi.fn()}
      />,
    );

    expect(screen.queryByRole("button", { name: "开始声纹分析" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "前往声纹管理" }));
    expect(onConfigureVoiceprints).toHaveBeenCalledOnce();
    expect(onAnalyze).not.toHaveBeenCalled();
  });

  it("closes from the header action or backdrop but not from dialog content", () => {
    const onCancel = vi.fn();
    const { container } = render(
      <SpeakerIdentificationModal
        speakers={speakers}
        session={null}
        analysisStatus="idle"
        analysisError={null}
        participants={participants}
        initialSpeaker={null}
        saving={false}
        voiceprintAvailability={{ kind: "available" }}
        activePreviewId={null}
        previewPlaying={false}
        onAnalyze={vi.fn()}
        onConfigureVoiceprints={vi.fn()}
        onPreview={vi.fn()}
        onStopPreview={vi.fn()}
        onCancel={onCancel}
        onSave={vi.fn()}
      />,
    );

    const analyze = screen.getByRole("button", { name: "开始声纹分析" });
    expect(analyze).toHaveClass("button", "secondary", "speaker-analysis-action");
    expect(screen.queryByText("SPEAKER MANAGEMENT")).not.toBeInTheDocument();
    expect(screen.queryByText(/姓名采用增量更新/)).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("dialog", { name: "管理说话人" }));
    expect(onCancel).not.toHaveBeenCalled();
    fireEvent.click(container.querySelector(".modal-backdrop")!);
    expect(onCancel).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole("button", { name: "关闭管理说话人" }));
    expect(onCancel).toHaveBeenCalledTimes(2);
  });

  it("prevents backdrop and close-button dismissal while a save is running", () => {
    const onCancel = vi.fn();
    const { container } = render(
      <SpeakerIdentificationModal
        speakers={speakers}
        session={null}
        analysisStatus="idle"
        analysisError={null}
        participants={participants}
        initialSpeaker={null}
        saving
        voiceprintAvailability={{ kind: "available" }}
        activePreviewId={null}
        previewPlaying={false}
        onAnalyze={vi.fn()}
        onConfigureVoiceprints={vi.fn()}
        onPreview={vi.fn()}
        onStopPreview={vi.fn()}
        onCancel={onCancel}
        onSave={vi.fn()}
      />,
    );

    expect(screen.getByRole("button", { name: "关闭管理说话人" })).toBeDisabled();
    fireEvent.click(container.querySelector(".modal-backdrop")!);
    expect(onCancel).not.toHaveBeenCalled();
  });
});
