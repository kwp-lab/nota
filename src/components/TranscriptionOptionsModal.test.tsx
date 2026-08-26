import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { TranscriptionOptionsModal } from "./TranscriptionOptionsModal";

afterEach(cleanup);

const renderModal = () => {
  const onCancel = vi.fn();
  const onConfirm = vi.fn();
  render(
    <TranscriptionOptionsModal
      recordingTitle="产品周会"
      retranscription={false}
      providerName="FunASR"
      speakerCountMin={1}
      speakerCountMax={64}
      cloudUpload={false}
      maxDurationMinutes={null}
      onCancel={onCancel}
      onConfirm={onConfirm}
    />,
  );
  return { onCancel, onConfirm };
};

describe("TranscriptionOptionsModal", () => {
  it("defaults to automatic speaker detection", () => {
    const { onConfirm } = renderModal();

    expect(screen.getByRole("radio", { name: /自动判断/ })).toBeChecked();
    fireEvent.click(screen.getByRole("button", { name: "开始转写" }));
    expect(onConfirm).toHaveBeenCalledWith(null);
  });

  it("accepts the 1 and 64 speaker boundaries", () => {
    const { onConfirm } = renderModal();
    fireEvent.click(screen.getByRole("radio", { name: /指定目标人数/ }));
    const input = screen.getByRole("spinbutton", { name: "说话人数" });

    fireEvent.change(input, { target: { value: "1" } });
    fireEvent.click(screen.getByRole("button", { name: "开始转写" }));
    expect(onConfirm).toHaveBeenLastCalledWith(1);

    fireEvent.change(input, { target: { value: "64" } });
    fireEvent.click(screen.getByRole("button", { name: "开始转写" }));
    expect(onConfirm).toHaveBeenLastCalledWith(64);
  });

  it("rejects invalid values and supports cancellation", () => {
    const { onCancel, onConfirm } = renderModal();
    fireEvent.click(screen.getByRole("radio", { name: /指定目标人数/ }));
    const input = screen.getByRole("spinbutton", { name: "说话人数" });
    const submit = screen.getByRole("button", { name: "开始转写" });

    for (const value of ["0", "65", "1.5", ""]) {
      fireEvent.change(input, { target: { value } });
      expect(submit).toBeDisabled();
    }
    expect(onConfirm).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    expect(onCancel).toHaveBeenCalledOnce();
  });

  it("explains that a specified count is a safety target", () => {
    renderModal();

    expect(screen.getByText(/结果可能更多/)).toBeInTheDocument();
    expect(screen.getByText(/不会为凑人数强行合并/)).toBeInTheDocument();
  });

  it("uses the DashScope 2–100 range and cloud disclosure", () => {
    const onConfirm = vi.fn();
    render(
      <TranscriptionOptionsModal
        recordingTitle="客户会议"
        retranscription={false}
        providerName="千问云转写"
        speakerCountMin={2}
        speakerCountMax={100}
        cloudUpload
        maxDurationMinutes={120}
        onCancel={vi.fn()}
        onConfirm={onConfirm}
      />,
    );
    fireEvent.click(screen.getByRole("radio", { name: /指定目标人数/ }));
    const input = screen.getByRole("spinbutton", { name: "说话人数" });
    fireEvent.change(input, { target: { value: "1" } });
    expect(screen.getByRole("button", { name: "开始转写" })).toBeDisabled();
    fireEvent.change(input, { target: { value: "100" } });
    fireEvent.click(screen.getByRole("button", { name: "开始转写" }));
    expect(onConfirm).toHaveBeenCalledWith(100);
    expect(screen.getByText(/上传至千问云转写/)).toBeInTheDocument();
    expect(screen.getByText(/最长 120 分钟/)).toBeInTheDocument();
  });

  it("selects one compatible hotword list and discloses cloud transfer", () => {
    const onConfirm = vi.fn();
    render(
      <TranscriptionOptionsModal
        recordingTitle="客户会议"
        retranscription={false}
        providerName="阿里云千问"
        speakerCountMin={2}
        speakerCountMax={100}
        cloudUpload
        maxDurationMinutes={120}
        hotwordLists={[
          {
            id: "products",
            name: "产品词",
            entryCount: 3,
            weightedEntryCount: 1,
            superHotwordCount: 1,
          },
          {
            id: "empty",
            name: "空列表",
            entryCount: 0,
            weightedEntryCount: 0,
            superHotwordCount: 0,
          },
        ]}
        hotwordsSupported
        hotwordMode="inline"
        hotwordWeightsSupported
        hotwordDefaultWeight={4}
        onOpenHotwordLibrary={vi.fn()}
        onCancel={vi.fn()}
        onConfirm={onConfirm}
      />,
    );

    fireEvent.change(screen.getByRole("combobox"), { target: { value: "products" } });
    expect(screen.getByText(/3 个热词及完整录音将发送至阿里云千问/)).toBeInTheDocument();
    expect(screen.getByText(/1 个超级热词/)).toBeInTheDocument();
    expect(screen.getByText(/2 个未指定权重的热词使用默认权重 4/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "开始转写" }));
    expect(onConfirm).toHaveBeenCalledWith(null, "products");
    expect(screen.getByRole("option", { name: /空列表/ })).toBeDisabled();
  });

  it("explains that Nota Server ignores custom weights", () => {
    render(
      <TranscriptionOptionsModal
        recordingTitle="客户会议"
        retranscription={false}
        providerName="Nota Server"
        speakerCountMin={1}
        speakerCountMax={64}
        cloudUpload={false}
        maxDurationMinutes={null}
        hotwordLists={[{
          id: "products",
          name: "产品词",
          entryCount: 2,
          weightedEntryCount: 1,
          superHotwordCount: 1,
        }]}
        hotwordsSupported
        hotwordMode="decoder_bias"
        hotwordWeightsSupported={false}
        onCancel={vi.fn()}
        onConfirm={vi.fn()}
      />,
    );
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "products" } });
    expect(screen.getByText(/不支持自定义权重，将忽略权重/)).toBeInTheDocument();
  });
});
