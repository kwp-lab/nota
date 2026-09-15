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
