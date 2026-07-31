import { act, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  enqueueToast,
  ToastRegion,
  type AppToast,
} from "./ToastRegion";

function ToastHarness({ initial }: { initial: AppToast[] }) {
  const [toasts, setToasts] = useState(initial);
  return (
    <ToastRegion
      toasts={toasts}
      onDismiss={(id) =>
        setToasts((current) => current.filter((toast) => toast.id !== id))
      }
    />
  );
}

afterEach(() => {
  vi.useRealTimers();
});

describe("ToastRegion", () => {
  it("automatically removes a success message after three seconds", () => {
    vi.useFakeTimers();
    render(
      <ToastHarness
        initial={[
          {
            id: 1,
            message: "录音已安全保存",
            tone: "success",
            durationMs: 3_000,
          },
        ]}
      />,
    );

    expect(screen.getByText("录音已安全保存")).toBeInTheDocument();
    act(() => vi.advanceTimersByTime(2_999));
    expect(screen.getByText("录音已安全保存")).toBeInTheDocument();
    act(() => vi.advanceTimersByTime(161));
    expect(screen.queryByText("录音已安全保存")).not.toBeInTheDocument();
  });

  it("keeps an error visible until the user closes it", () => {
    vi.useFakeTimers();
    render(
      <ToastHarness
        initial={[
          {
            id: 2,
            message: "无法写入录音文件",
            tone: "error",
            durationMs: null,
          },
        ]}
      />,
    );

    act(() => vi.advanceTimersByTime(30_000));
    expect(screen.getByRole("alert")).toHaveTextContent("无法写入录音文件");
    fireEvent.click(screen.getByRole("button", { name: "关闭通知" }));
    act(() => vi.advanceTimersByTime(161));
    expect(screen.queryByText("无法写入录音文件")).not.toBeInTheDocument();
  });

  it("deduplicates faults and preserves persistent messages over transient ones", () => {
    const errors: AppToast[] = [1, 2, 3].map((id) => ({
      id,
      message: `错误 ${id}`,
      tone: "error",
      durationMs: null,
      dedupeKey: `fault-${id}`,
    }));

    expect(
      enqueueToast(errors, {
        id: 4,
        message: "保存成功",
        tone: "success",
        durationMs: 3_000,
      }),
    ).toEqual(errors);
    expect(
      enqueueToast(errors, {
        id: 5,
        message: "重复错误",
        tone: "error",
        durationMs: null,
        dedupeKey: "fault-2",
      }),
    ).toEqual(errors);
  });
});
