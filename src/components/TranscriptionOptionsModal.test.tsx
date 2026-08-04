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
    fireEvent.click(screen.getByRole("radio", { name: /指定人数/ }));
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
    fireEvent.click(screen.getByRole("radio", { name: /指定人数/ }));
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
});
