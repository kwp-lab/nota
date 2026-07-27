import { describe, expect, it } from "vitest";
import { preferenceForTarget, resolveCaptureTarget } from "./captureTargets";
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
