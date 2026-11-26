import { useEffect, useRef } from "react";
import { audioLevelToDisplay } from "../audio-level";

export const activityAmplitude = (
  level: number,
  active: boolean,
  ageMs: number,
) => active && ageMs < 600
  ? audioLevelToDisplay(level)
  : 0;

export const activityWavePath = (amplitude: number, phase: number) => {
  const safeAmplitude = Number.isFinite(amplitude) ? Math.max(0, Math.min(1, amplitude)) : 0;
  if (safeAmplitude < 0.005) return "M 0 14 L 100 14";
  return Array.from({ length: 51 }, (_, index) => {
    const position = index * 2;
    const envelope = Math.sin(Math.PI * position / 100) ** 1.5;
    const signal = (
      Math.sin(position * 0.31 - phase * 5) +
      Math.sin(position * 0.63 + phase * 3) * 0.28
    ) / 1.28;
    const height = 14 + signal * envelope * 12 * safeAmplitude;
    return `${index === 0 ? "M" : "L"} ${position} ${height.toFixed(2)}`;
  }).join(" ");
};

export function AudioActivityWaveform(props: {
  level: number;
  updatedAt: number;
  active: boolean;
  paused: boolean;
}) {
  const pathRef = useRef<SVGPathElement>(null);
  const latestRef = useRef(props);
  latestRef.current = props;

  useEffect(() => {
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
    let frame = 0;
    let previousTime: number | null = null;
    let amplitude = 0;
    const draw = (time: number) => {
      frame = requestAnimationFrame(draw);
      if (document.visibilityState === "hidden") {
        amplitude = 0;
        return;
      }
      const interval = reducedMotion.matches ? 250 : 1000 / 30;
      if (previousTime !== null && time - previousTime < interval) return;
      const elapsed = previousTime === null ? interval : Math.min(250, time - previousTime);
      previousTime = time;
      const latest = latestRef.current;
      const target = activityAmplitude(latest.level, latest.active, Date.now() - latest.updatedAt);
      const response = 1 - Math.exp(-elapsed / (target > amplitude ? 65 : 180));
      amplitude = latest.active ? amplitude + (target - amplitude) * response : 0;
      if (target === 0 && amplitude < 0.005) amplitude = 0;
      pathRef.current?.setAttribute(
        "d",
        activityWavePath(amplitude, reducedMotion.matches ? 0 : time / 1000),
      );
    };
    frame = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(frame);
  }, []);

  return (
    <svg
      className={`recording-overlay-wave${props.paused ? " is-paused" : ""}`}
      viewBox="0 0 100 28"
      aria-hidden="true"
    >
      <path ref={pathRef} d="M 0 14 L 100 14" />
    </svg>
  );
}
