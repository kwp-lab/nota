const DISPLAY_FLOOR_DB = -60;
const DISPLAY_CEILING_DB = 0;

export function audioLevelToDisplay(rms: number): number {
  if (!Number.isFinite(rms) || rms <= 0) return 0;
  const decibels = 20 * Math.log10(Math.min(1, rms));
  return Math.max(0, Math.min(
    1,
    (decibels - DISPLAY_FLOOR_DB) / (DISPLAY_CEILING_DB - DISPLAY_FLOOR_DB),
  ));
}
