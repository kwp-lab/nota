import { act, cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { activityAmplitude, activityWavePath, AudioActivityWaveform } from "./AudioActivityWaveform";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("audio activity waveform", () => {
  it("uses finite fresh volume only and suppresses background-level flicker", () => {
    expect(activityAmplitude(1, true, 100)).toBe(1);
    expect(activityAmplitude(0.0001, true, 10)).toBe(0);
    expect(activityAmplitude(0.8, false, 10)).toBe(0);
    expect(activityAmplitude(0.8, true, 600)).toBe(0);
    expect(activityAmplitude(Number.NaN, true, 10)).toBe(0);
    expect(activityAmplitude(Number.POSITIVE_INFINITY, true, 10)).toBe(0);
  });

  it("gives quiet speech visible peaks instead of treating it as silence", () => {
    for (const level of [0.005, 0.01, 0.02, 0.03]) {
      const amplitude = activityAmplitude(level, true, 10);
      expect(amplitude).toBeGreaterThan(0.2);
      const path = activityWavePath(amplitude, 0.4);
      const heights = [...path.matchAll(/[ML] \d+ ([\d.-]+)/g)]
        .map(([, height]) => Number(height));
      expect(Math.max(...heights) - Math.min(...heights)).toBeGreaterThan(3);
    }
  });

  it("draws a flat silent line and a bounded symmetric signal with anchored ends", () => {
    expect(activityWavePath(0, 0)).toBe("M 0 14 L 100 14");
    expect(activityWavePath(Number.NaN, 0)).toBe("M 0 14 L 100 14");
    const path = activityWavePath(1, 0.4);
    const values = [...path.matchAll(/[ML] (\d+) ([\d.-]+)/g)];
    expect(values).toHaveLength(51);
    expect(values[0].slice(1)).toEqual(["0", "14.00"]);
    expect(values.at(-1)?.slice(1)).toEqual(["100", "14.00"]);
    expect(values.every(([, , height]) => Number(height) >= 2 && Number(height) <= 26)).toBe(true);
    expect(values.some(([, , height]) => Number(height) > 14)).toBe(true);
    expect(values.some(([, , height]) => Number(height) < 14)).toBe(true);
  });

  it("returns to a line after event loss and freezes on pause without leaking animation frames", () => {
    let frameCallback: FrameRequestCallback | undefined;
    vi.stubGlobal("requestAnimationFrame", vi.fn((callback: FrameRequestCallback) => {
      frameCallback = callback;
      return 1;
    }));
    vi.stubGlobal("cancelAnimationFrame", vi.fn());
    vi.stubGlobal("matchMedia", vi.fn(() => ({ matches: false })));
    const { container, rerender, unmount } = render(
      <AudioActivityWaveform level={0.01} active updatedAt={Date.now()} paused={false} />,
    );
    act(() => frameCallback?.(100));
    const path = container.querySelector("path")!;
    expect(path.getAttribute("d")).not.toBe("M 0 14 L 100 14");
    rerender(<AudioActivityWaveform level={0.8} active updatedAt={0} paused={false} />);
    act(() => {
      for (let time = 200; time < 3000; time += 100) frameCallback?.(time);
    });
    expect(path).toHaveAttribute("d", "M 0 14 L 100 14");
    rerender(<AudioActivityWaveform level={0.8} active={false} updatedAt={Date.now()} paused />);
    act(() => frameCallback?.(4000));
    expect(path).toHaveAttribute("d", "M 0 14 L 100 14");
    expect(container.querySelector("svg")).toHaveClass("is-paused");
    unmount();
    expect(cancelAnimationFrame).toHaveBeenCalled();
  });

  it("keeps a stable phase under reduced motion and reacts to system preference changes", () => {
    const media = { matches: true };
    let frameCallback: FrameRequestCallback | undefined;
    vi.stubGlobal("requestAnimationFrame", vi.fn((callback: FrameRequestCallback) => {
      frameCallback = callback;
      return 1;
    }));
    vi.stubGlobal("cancelAnimationFrame", vi.fn());
    vi.stubGlobal("matchMedia", vi.fn(() => media));
    const { container } = render(
      <AudioActivityWaveform level={1} active updatedAt={Date.now()} paused={false} />,
    );
    act(() => {
      for (let time = 0; time <= 3000; time += 250) frameCallback?.(time);
    });
    const path = container.querySelector("path")!;
    expect(path).toHaveAttribute("d", activityWavePath(1, 0));
    media.matches = false;
    act(() => frameCallback?.(4000));
    expect(path.getAttribute("d")).not.toBe(activityWavePath(1, 0));
  });
});
