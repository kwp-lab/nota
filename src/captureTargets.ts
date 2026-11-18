import type { CaptureTarget } from "./types";

export interface CaptureTargetPreference {
  executablePath: string;
  displayName: string;
  iconDataUrl: string | null;
}

export const captureTargetExecutableName = (
  target: Pick<CaptureTarget, "executablePath">,
) => target.executablePath.split(/[\\/]/).filter(Boolean).at(-1) ?? "";

export const captureTargetLabel = (
  target: Pick<CaptureTarget, "displayName" | "executablePath">,
) => {
  const executableName = captureTargetExecutableName(target);
  return executableName
    ? `[${executableName}]: ${target.displayName}`
    : target.displayName;
};

export const normalizeCaptureExecutable = (value: string) => {
  const name = value.trim().toLowerCase();
  if (
    name.length <= 4 ||
    !name.endsWith(".exe") ||
    /[\\/:*?"<>|\u0000-\u001f\u007f-\u009f]/.test(name)
  ) {
    throw new Error("请输入完整的进程文件名（例如 chrome.exe），不要包含路径或通配符");
  }
  return name;
};

export const isCaptureTargetHidden = (
  target: Pick<CaptureTarget, "executablePath">,
  hiddenExecutables: string[],
) => hiddenExecutables.includes(captureTargetExecutableName(target).toLowerCase());

const normalizeExecutablePath = (path: string) =>
  path.replaceAll("/", "\\").toLocaleLowerCase();

export const preferenceForTarget = (
  target: CaptureTarget,
): CaptureTargetPreference => ({
  executablePath: target.executablePath,
  displayName: target.displayName,
  iconDataUrl: target.iconDataUrl,
});

export const resolveCaptureTarget = (
  targets: CaptureTarget[],
  currentTargetId: string,
  preference: CaptureTargetPreference | null,
) => {
  const currentProcess = targets.find((target) => target.id === currentTargetId);
  if (
    currentProcess &&
    (!preference ||
      normalizeExecutablePath(currentProcess.executablePath) ===
        normalizeExecutablePath(preference.executablePath))
  ) {
    return currentProcess;
  }

  if (preference) {
    const preferredPath = normalizeExecutablePath(preference.executablePath);
    const matchingProcess = targets.find(
      (target) =>
        normalizeExecutablePath(target.executablePath) === preferredPath,
    );
    if (matchingProcess) return matchingProcess;
  }

  return preference ? undefined : targets[0];
};
