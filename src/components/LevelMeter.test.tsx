import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { LevelMeter } from "./LevelMeter";

afterEach(cleanup);

describe("recording level meter", () => {
  it("shows low microphone speech at a readable proportion at any track width", () => {
    const { container } = render(<LevelMeter value={0.01} label="我的麦克风" healthy />);
    expect(screen.getByLabelText("我的麦克风电平")).toBeInTheDocument();
    const fill = container.querySelector<HTMLElement>(".level-fill")!;
    expect(fill.style.transform).toBe(`scaleX(${1 / 3})`);
  });

  it("returns silence, invalid values and unavailable sources to an empty track", () => {
    const { container, rerender } = render(<LevelMeter value={0.02} label="会议声音" healthy />);
    const fill = container.querySelector<HTMLElement>(".level-fill")!;
    for (const value of [0, 0.0001, Number.NaN]) {
      rerender(<LevelMeter value={value} label="会议声音" healthy />);
      expect(fill.style.transform).toBe("scaleX(0)");
    }
    rerender(<LevelMeter value={0.8} label="会议声音" healthy={false} />);
    expect(fill.style.transform).toBe("scaleX(0)");
    expect(screen.getByText("等待")).toBeInTheDocument();
  });
});
