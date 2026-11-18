import { describe, expect, it } from "vitest";
import {
  captureTargetLabel,
  isCaptureTargetHidden,
  normalizeCaptureExecutable,
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
  iconDataUrl: null,
  browser: false,
  priority: 90,
});

describe("capture target reconciliation", () => {
  it("matches hidden names across process ids, paths, and filename case", () => {
    const hidden = ["meeting.exe"];
    expect(isCaptureTargetHidden(target("process:1", "C:\\Apps\\Meeting.EXE"), hidden)).toBe(true);
    expect(isCaptureTargetHidden(target("process:2", "D:/Other/meeting.exe"), hidden)).toBe(true);
    expect(isCaptureTargetHidden(target("process:3", "C:\\OtherMeeting.exe"), hidden)).toBe(false);
    expect(normalizeCaptureExecutable("  MEETING.EXE  ")).toBe("meeting.exe");
  });

  it.each(["", ".exe", "meeting", "*.exe", "C:\\Meeting.exe", "a/b.exe", "a\u0000.exe"])(
    "rejects invalid hidden filename %j",
    (name) => expect(() => normalizeCaptureExecutable(name)).toThrow("完整的进程文件名"),
  );

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

  it("keeps the application icon in the in-memory restart preference", () => {
    const selected = {
      ...target("process:42", "C:\\Apps\\Meeting.exe"),
      iconDataUrl: "data:image/png;base64,aWNvbg==",
    };
    expect(preferenceForTarget(selected)).toEqual({
      executablePath: "C:\\Apps\\Meeting.exe",
      displayName: "Meeting app",
      iconDataUrl: "data:image/png;base64,aWNvbg==",
    });
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
