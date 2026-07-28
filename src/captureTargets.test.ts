import { describe, expect, it } from "vitest";
import {
  captureTargetLabel,
  preferenceForTarget,
  resolveCaptureTarget,
} from "./captureTargets";
import type { CaptureTarget } from "./types";

const target = (
  id: string,
  executablePath: string,
  displayName = "Meeting app",
): CaptureTarget => ({
  id,
  kind: "process",
  displayName,
  processId: Number(id.split(":")[1]),
  executablePath,
  browser: false,
  priority: 90,
});

describe("capture target reconciliation", () => {
  it("prefixes the window title with its executable name", () => {
    expect(
      captureTargetLabel(
        target("process:42", "C:\\Program Files\\Feishu\\Feishu.exe", "飞书"),
      ),
    ).toBe("[Feishu.exe]: 飞书");
  });

  it("keeps the title readable when no executable path is available", () => {
    expect(captureTargetLabel(target("process:42", "", "飞书"))).toBe("飞书");
  });

  it("follows the same executable when its process id changes", () => {
    const previous = target("process:42", "C:\\Apps\\Meeting.exe");
    const restarted = target("process:84", "c:/apps/meeting.exe");

    expect(
      resolveCaptureTarget(
        [restarted],
        previous.id,
        preferenceForTarget(previous),
      ),
    ).toEqual(restarted);
  });

  it("does not silently switch to another application", () => {
    const previous = target("process:42", "C:\\Apps\\Meeting.exe");
    const browser = target("process:99", "C:\\Chrome\\chrome.exe", "Chrome");

    expect(
      resolveCaptureTarget(
        [browser],
        previous.id,
        preferenceForTarget(previous),
      ),
    ).toBeUndefined();
  });
});
