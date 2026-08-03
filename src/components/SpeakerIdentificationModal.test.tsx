import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SpeakerIdentificationModal } from "./SpeakerIdentificationModal";

afterEach(cleanup);

describe("SpeakerIdentificationModal", () => {
  it("prefills confident suggestions and keeps final confirmation explicit", () => {
    const onPreview = vi.fn();
    const onSave = vi.fn();
    render(
      <SpeakerIdentificationModal
        session={{
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
            suggestedParticipantId: "participant",
            suggestedParticipantName: "小明",
            matchScore: 0.91,
          }],
        }}
        participants={[{
          id: "participant",
          displayName: "小明",
          createdAt: "2026-08-02T00:00:00Z",
          updatedAt: "2026-08-02T00:00:00Z",
          samples: [],
        }]}
        saving={false}
        onPreview={onPreview}
        onCancel={vi.fn()}
        onSave={onSave}
      />,
    );

    expect(screen.getByRole("combobox")).toHaveValue("participant");
    fireEvent.click(screen.getByRole("button", { name: "试听" }));
    expect(onPreview).toHaveBeenCalledWith(2_000, 9_000);
    fireEvent.click(screen.getByRole("button", { name: "保存可用声纹并更新说话人" }));
    expect(onSave).toHaveBeenCalledWith([{
      rawSpeaker: "speaker_0",
      participantId: "participant",
      newDisplayName: null,
    }]);
  });

  it("keeps preview and meeting-local naming available for preview-only samples", () => {
    const onPreview = vi.fn();
    const onSave = vi.fn();
    render(
      <SpeakerIdentificationModal
        session={{
          id: "session",
          recordingId: "recording",
          speakerCount: 1,
          voiceprintCount: 0,
          candidates: [{
            rawSpeaker: "speaker_0",
            totalSpeechMs: 12_000,
            previewStartMs: 3_000,
            previewEndMs: 9_000,
            embeddingExtracted: false,
            sampleStatus: "preview_only",
            statusMessage: "可试听并手动标记姓名；当前声音不满足声纹入库标准",
            errorMessage: null,
            suggestedParticipantId: null,
            suggestedParticipantName: null,
            matchScore: null,
          }],
        }}
        participants={[]}
        saving={false}
        onPreview={onPreview}
        onCancel={vi.fn()}
        onSave={onSave}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "试听" }));
    expect(onPreview).toHaveBeenCalledWith(3_000, 9_000);
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "__new__" } });
    fireEvent.change(screen.getByRole("textbox", { name: "speaker_0 新参会人姓名" }), {
      target: { value: "小红" },
    });
    fireEvent.click(screen.getByRole("button", { name: "更新说话人" }));
    expect(onSave).toHaveBeenCalledWith([{
      rawSpeaker: "speaker_0",
      participantId: null,
      newDisplayName: "小红",
    }]);
  });
});
