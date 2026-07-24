interface LevelMeterProps {
  value: number;
  label: string;
  healthy: boolean;
}

export function LevelMeter({ value, label, healthy }: LevelMeterProps) {
  const safeValue = Math.max(0, Math.min(1, value));
  return (
    <div className="level-meter" aria-label={`${label}电平`}>
      <div className="level-label">
        <span>{label}</span>
        <span className={healthy ? "health-ok" : "health-bad"}>
          {healthy ? "正常" : "等待"}
        </span>
      </div>
      <div className="level-track">
        <div
          className="level-fill"
          style={{ transform: `scaleX(${safeValue})` }}
        />
      </div>
    </div>
  );
}
