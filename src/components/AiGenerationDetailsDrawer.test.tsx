import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { AiGenerationDetailsDrawer } from "./AiGenerationDetailsDrawer";

afterEach(cleanup);

it("dismisses only clicks started on the native backdrop, not panel padding or content drags", () => {
  const onClose = vi.fn();
  render(<AiGenerationDetailsDrawer onClose={onClose}><p>JSON content</p></AiGenerationDetailsDrawer>);
  const drawer = screen.getByRole("dialog");
  vi.spyOn(drawer, "getBoundingClientRect").mockReturnValue({ left: 560, right: 980, top: 0, bottom: 640 } as DOMRect);
  const pointerDown = (target: HTMLElement, clientX: number, button = 0) =>
    fireEvent(target, new MouseEvent("pointerdown", { bubbles: true, button, clientX, clientY: 100 }));

  pointerDown(drawer, 570);
  fireEvent.click(drawer, { clientX: 570, clientY: 100 });
  pointerDown(screen.getByText("JSON content"), 600);
  fireEvent.click(drawer, { clientX: 100, clientY: 100 });
  pointerDown(drawer, 100);
  fireEvent.click(screen.getByText("JSON content"), { clientX: 600, clientY: 100 });
  pointerDown(drawer, 100, 2);
  fireEvent.click(drawer, { clientX: 100, clientY: 100 });
  pointerDown(drawer, 100);
  fireEvent.pointerCancel(drawer);
  fireEvent.click(drawer, { clientX: 100, clientY: 100 });
  expect(onClose).not.toHaveBeenCalled();

  pointerDown(drawer, 100);
  fireEvent.click(drawer, { clientX: 100, clientY: 100 });
  expect(onClose).toHaveBeenCalledTimes(1);
});

it("restores the trigger focus after backdrop dismissal unmounts the drawer", () => {
  const trigger = document.createElement("button");
  document.body.append(trigger);
  trigger.focus();
  const { unmount } = render(<AiGenerationDetailsDrawer onClose={() => unmount()}><p>Details</p></AiGenerationDetailsDrawer>);
  const drawer = screen.getByRole("dialog");
  vi.spyOn(drawer, "getBoundingClientRect").mockReturnValue({ left: 560, right: 980, top: 0, bottom: 640 } as DOMRect);
  screen.getByRole("button", { name: "关闭生成详情" }).focus();
  fireEvent(drawer, new MouseEvent("pointerdown", { bubbles: true, button: 0, clientX: 100, clientY: 100 }));
  fireEvent.click(drawer, { clientX: 100, clientY: 100 });
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(trigger).toHaveFocus();
  trigger.remove();
});
