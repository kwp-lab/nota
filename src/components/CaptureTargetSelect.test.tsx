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

function Harness({ onOpenChange = vi.fn() }: { onOpenChange?: (open: boolean) => void }) {
  const [value, setValue] = useState(targets[0].id);
  return (
    <>
      <label id="capture-source-label" htmlFor="capture-source">录音来源</label>
      <CaptureTargetSelect
        id="capture-source"
        labelId="capture-source-label"
        targets={targets}
        value={value}
        unavailableTarget={null}
        refreshing={false}
        onValueChange={setValue}
        onOpenChange={onOpenChange}
      />
    </>
  );
}

describe("CaptureTargetSelect", () => {
  it("shows the selected application icon and two-line option details", async () => {
    const onOpenChange = vi.fn();
    const { container } = render(<Harness onOpenChange={onOpenChange} />);

    const trigger = screen.getByRole("combobox", { name: "录音来源" });
    expect(trigger).toHaveTextContent("[Zoom.exe]: Zoom Meeting");
    expect(trigger.querySelector("img")).toHaveAttribute(
      "src",
      targets[0].iconDataUrl,
    );

    fireEvent.click(trigger);
    const zoom = await screen.findByRole("option", {
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
      screen.getByRole("option", { name: "[wemeetapp.exe]: 腾讯会议" })
        .querySelector(".capture-target-icon.fallback"),
    ).toBeInTheDocument();
    expect(container.querySelectorAll("img[aria-hidden='true']").length).toBeGreaterThan(0);
    expect(onOpenChange).toHaveBeenCalledWith(true);
  });

  it("supports keyboard navigation, selection, escape, and focus restoration", async () => {
    render(<Harness />);
    const trigger = screen.getByRole("combobox", { name: "录音来源" });
    trigger.focus();
    fireEvent.keyDown(trigger, { key: " " });

    const zoom = await screen.findByRole("option", {
      name: "[Zoom.exe]: Zoom Meeting",
    });
    const tencent = screen.getByRole("option", {
      name: "[wemeetapp.exe]: 腾讯会议",
    });
    await waitFor(() => expect(document.activeElement).toBe(zoom));
    fireEvent.keyDown(zoom, { key: "ArrowDown" });
    await waitFor(() => expect(document.activeElement).toBe(tencent));
    fireEvent.keyDown(tencent, { key: "Z" });
    await waitFor(() => expect(document.activeElement).toBe(zoom));
    fireEvent.keyDown(zoom, { key: "ArrowDown" });
    await waitFor(() => expect(document.activeElement).toBe(tencent));
    fireEvent.keyDown(tencent, { key: "Enter" });

    await waitFor(() => {
      expect(trigger).toHaveTextContent("[wemeetapp.exe]: 腾讯会议");
      expect(trigger).toHaveAttribute("aria-expanded", "false");
      expect(document.activeElement).toBe(trigger);
    });

    fireEvent.keyDown(trigger, { key: "Enter" });
    await screen.findByRole("option", { name: "[wemeetapp.exe]: 腾讯会议" });
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
          onValueChange={vi.fn()}
          onOpenChange={vi.fn()}
        />
      </>,
    );

    const trigger = screen.getByRole("combobox", { name: "录音来源" });
    expect(trigger).toHaveTextContent("[Zoom.exe]: Zoom Meeting（未运行）");
    expect(trigger.querySelector("img")).toHaveAttribute(
      "src",
      "data:image/png;base64,em9vbQ==",
    );
  });
});
