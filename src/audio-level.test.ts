import { describe, expect, it } from "vitest";
import { audioLevelToDisplay } from "./audio-level";

describe("audio level display mapping", () => {
  it("makes low microphone speech visible without saturating it", () => {
    expect(audioLevelToDisplay(0.005)).toBeCloseTo(0.233, 3);
    expect(audioLevelToDisplay(0.01)).toBeCloseTo(1 / 3);
    expect(audioLevelToDisplay(0.02)).toBeCloseTo(0.434, 3);
    expect(audioLevelToDisplay(0.03)).toBeCloseTo(0.492, 3);
  });

  it("keeps silence and sub-floor noise at zero and rejects invalid input", () => {
    for (const rms of [0, 0.0001, 0.001, -0.01, Number.NaN, Infinity, -Infinity]) {
      expect(audioLevelToDisplay(rms)).toBe(0);
    }
  });

  it("preserves ordering and caps overloads", () => {
    const values = [0.001, 0.002, 0.005, 0.01, 0.03, 0.1, 0.3, 1];
    const mapped = values.map(audioLevelToDisplay);
    expect(mapped.every((value, index) => index === 0 || value > mapped[index - 1])).toBe(true);
    expect(audioLevelToDisplay(1)).toBe(1);
    expect(audioLevelToDisplay(2)).toBe(1);
  });
});
