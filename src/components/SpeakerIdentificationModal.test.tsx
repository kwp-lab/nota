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
    fireEvent.click(screen.getByRole("button", { name: "保存声纹并更新说话人" }));
    expect(onSave).toHaveBeenCalledWith([{
      rawSpeaker: "speaker_0",
      participantId: "participant",
      newDisplayName: null,
    }]);
  });
});
