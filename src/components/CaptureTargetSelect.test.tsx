import { useState } from "react";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { CaptureTarget } from "../types";
import { CaptureTargetSelect } from "./CaptureTargetSelect";

afterEach(cleanup);

const targets: CaptureTarget[] = [
  {
    id: "process:42",
    kind: "process",
    displayName: "Zoom Meeting",
    processId: 42,
    executablePath: "C:\\Apps\\Zoom.exe",
    iconDataUrl: "data:image/png;base64,em9vbQ==",
    browser: false,
    priority: 100,
  },
  {
    id: "process:84",
    kind: "process",
    displayName: "腾讯会议",
    processId: 84,
    executablePath: "C:\\Apps\\wemeetapp.exe",
    iconDataUrl: null,
    browser: false,
    priority: 90,
  },
];

function Harness({
  onOpenChange = vi.fn(),
  onHide = vi.fn(async () => undefined),
  onManageHidden = vi.fn(),
}: {
  onOpenChange?: (open: boolean) => void;
  onHide?: (target: CaptureTarget) => Promise<void>;
  onManageHidden?: () => void;
}) {
  const [value, setValue] = useState(targets[0].id);
  const [visibleTargets, setVisibleTargets] = useState(targets);
  return (
    <>
      <label id="capture-source-label" htmlFor="capture-source">录音来源</label>
      <CaptureTargetSelect
        id="capture-source"
        labelId="capture-source-label"
        targets={visibleTargets}
        value={value}
        unavailableTarget={null}
        refreshing={false}
        hiddenCount={0}
        onHide={async (target) => {
          await onHide(target);
          setVisibleTargets((current) => current.filter((candidate) => candidate.id !== target.id));
        }}
        onManageHidden={onManageHidden}
        onValueChange={setValue}
        onOpenChange={onOpenChange}
      />
    </>
  );
}

describe("CaptureTargetSelect", () => {
  it("exposes independent hide actions without selecting their rows and keeps focus recoverable", async () => {
    const onHide = vi.fn(async () => undefined);
    const onManageHidden = vi.fn();
    render(<Harness onHide={onHide} onManageHidden={onManageHidden} />);
    const trigger = screen.getByRole("button", { name: "录音来源" });
    fireEvent.click(trigger);
    const hide = screen.getByRole("button", { name: "隐藏 Zoom.exe" });
    expect(hide.parentElement?.closest("button")).toBeNull();
    fireEvent.click(hide);
    await waitFor(() => expect(screen.queryByRole("button", {
      name: "[Zoom.exe]: Zoom Meeting",
    })).not.toBeInTheDocument());
    expect(onHide).toHaveBeenCalledWith(targets[0]);
    expect(trigger).toHaveAttribute("aria-expanded", "true");
    expect(document.activeElement).toBe(screen.getByRole("button", {
      name: "[wemeetapp.exe]: 腾讯会议",
    }));
    fireEvent.click(screen.getByRole("button", { name: "管理已隐藏应用（0）" }));
    expect(onManageHidden).toHaveBeenCalledOnce();
    expect(trigger).toHaveAttribute("aria-expanded", "false");
  });

  it("closes on outside clicks and keyboard focus leaving the panel", () => {
    render(<><Harness /><button type="button">其他操作</button></>);
    const trigger = screen.getByRole("button", { name: "录音来源" });
    const other = screen.getByRole("button", { name: "其他操作" });
    fireEvent.click(trigger);
    fireEvent.pointerDown(other);
    expect(trigger).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(trigger);
    fireEvent.blur(screen.getByRole("dialog"), { relatedTarget: other });
    expect(trigger).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(trigger);
    fireEvent.blur(screen.getByRole("dialog"), { relatedTarget: null });
    expect(trigger).toHaveAttribute("aria-expanded", "false");
  });

  it("shows the selected application icon and two-line option details", async () => {
    const onOpenChange = vi.fn();
    const { container } = render(<Harness onOpenChange={onOpenChange} />);

    const trigger = screen.getByRole("button", { name: "录音来源" });
    expect(trigger).toHaveTextContent("[Zoom.exe]: Zoom Meeting");
    expect(trigger.querySelector("img")).toHaveAttribute(
      "src",
      targets[0].iconDataUrl,
    );

    fireEvent.click(trigger);
    const zoom = await screen.findByRole("button", {
      name: "[Zoom.exe]: Zoom Meeting",
    });
    expect(Array.from(zoom.children, (child) => child.className)).toEqual([
      "capture-target-icon",
      "capture-target-copy",
      "capture-target-indicator",
    ]);
    expect(zoom).toHaveTextContent("Zoom Meeting");
    expect(zoom).toHaveTextContent("Zoom.exe");
    expect(
      screen.getByRole("button", { name: "[wemeetapp.exe]: 腾讯会议" })
        .querySelector(".capture-target-icon.fallback"),
    ).toBeInTheDocument();
    expect(container.querySelectorAll("img[aria-hidden='true']").length).toBeGreaterThan(0);
    expect(onOpenChange).toHaveBeenCalledWith(true);
  });

  it("supports keyboard navigation, selection, escape, and focus restoration", async () => {
    render(<Harness />);
    const trigger = screen.getByRole("button", { name: "录音来源" });
    trigger.focus();
    fireEvent.keyDown(trigger, { key: "ArrowDown" });

    const zoom = await screen.findByRole("button", {
      name: "[Zoom.exe]: Zoom Meeting",
    });
    const tencent = screen.getByRole("button", {
      name: "[wemeetapp.exe]: 腾讯会议",
    });
    await waitFor(() => expect(document.activeElement).toBe(zoom));
    fireEvent.keyDown(zoom, { key: "ArrowDown" });
    await waitFor(() => expect(document.activeElement).toBe(tencent));
    fireEvent.keyDown(tencent, { key: "Z" });
    await waitFor(() => expect(document.activeElement).toBe(zoom));
    fireEvent.keyDown(zoom, { key: "ArrowDown" });
    await waitFor(() => expect(document.activeElement).toBe(tencent));
    fireEvent.click(tencent);

    await waitFor(() => {
      expect(trigger).toHaveTextContent("[wemeetapp.exe]: 腾讯会议");
      expect(trigger).toHaveAttribute("aria-expanded", "false");
      expect(document.activeElement).toBe(trigger);
    });

    fireEvent.click(trigger);
    await screen.findByRole("button", { name: "[wemeetapp.exe]: 腾讯会议" });
    fireEvent.keyDown(document.activeElement ?? trigger, { key: "Escape" });
    await waitFor(() => {
      expect(trigger).toHaveAttribute("aria-expanded", "false");
      expect(document.activeElement).toBe(trigger);
    });
  });

  it("keeps the previous icon and marks a stopped application unavailable", () => {
    render(
      <>
        <label id="capture-source-label" htmlFor="capture-source">录音来源</label>
        <CaptureTargetSelect
          id="capture-source"
          labelId="capture-source-label"
          targets={[]}
          value=""
          unavailableTarget={{
            executablePath: "C:\\Apps\\Zoom.exe",
            displayName: "Zoom Meeting",
            iconDataUrl: "data:image/png;base64,em9vbQ==",
          }}
          refreshing={false}
          hiddenCount={0}
          onHide={vi.fn(async () => undefined)}
          onManageHidden={vi.fn()}
          onValueChange={vi.fn()}
          onOpenChange={vi.fn()}
        />
      </>,
    );

    const trigger = screen.getByRole("button", { name: "录音来源" });
    expect(trigger).toHaveTextContent("[Zoom.exe]: Zoom Meeting（未运行）");
    expect(trigger.querySelector("img")).toHaveAttribute(
      "src",
      "data:image/png;base64,em9vbQ==",
    );
  });
});
