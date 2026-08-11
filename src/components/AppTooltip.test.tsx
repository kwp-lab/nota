import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { AppTooltip, AppTooltipProvider } from "./AppTooltip";

afterEach(cleanup);

describe("AppTooltip", () => {
  it("shows the custom accessible tooltip on keyboard focus without a native title", async () => {
    render(
      <AppTooltipProvider>
        <AppTooltip content="复制文件路径">
          <button aria-label="复制文件路径">icon</button>
        </AppTooltip>
      </AppTooltipProvider>,
    );

    const button = screen.getByRole("button", { name: "复制文件路径" });
    expect(button).not.toHaveAttribute("title");
    fireEvent.focus(button);
    expect(await screen.findByRole("tooltip")).toHaveTextContent("复制文件路径");
  });
});
