import * as SelectPrimitive from "@radix-ui/react-select";
import { AppWindow, Check, ChevronDown, ChevronUp } from "lucide-react";
import {
  captureTargetExecutableName,
  captureTargetLabel,
  type CaptureTargetPreference,
} from "../captureTargets";
import type { CaptureTarget } from "../types";

interface CaptureTargetSelectProps {
  id: string;
  labelId: string;
  targets: CaptureTarget[];
  value: string;
  unavailableTarget: CaptureTargetPreference | null;
  refreshing: boolean;
  onValueChange: (value: string) => void;
  onOpenChange: (open: boolean) => void;
}

type TargetIdentity = Pick<
  CaptureTarget,
  "displayName" | "executablePath" | "iconDataUrl"
>;

function CaptureTargetIcon({ target }: { target: TargetIdentity | null }) {
  return target?.iconDataUrl ? (
    <img
      className="capture-target-icon"
      src={target.iconDataUrl}
      alt=""
      aria-hidden="true"
    />
  ) : (
    <span className="capture-target-icon fallback" aria-hidden="true">
      <AppWindow />
    </span>
  );
}

function TriggerValue({
  target,
  unavailable,
}: {
  target: TargetIdentity | null;
  unavailable?: boolean;
}) {
  const label = target
    ? `${captureTargetLabel(target)}${unavailable ? "（未运行）" : ""}`
    : "请选择要录制的应用";
  return (
    <span className="capture-target-trigger-value">
      <CaptureTargetIcon target={target} />
      <span>{label}</span>
    </span>
  );
}

export function CaptureTargetSelect({
  id,
  labelId,
  targets,
  value,
  unavailableTarget,
  refreshing,
  onValueChange,
  onOpenChange,
}: CaptureTargetSelectProps) {
  const selectedTarget = targets.find((target) => target.id === value) ?? null;
  const placeholderTarget = value ? null : unavailableTarget;

  return (
    <SelectPrimitive.Root
      value={selectedTarget?.id ?? ""}
      onValueChange={onValueChange}
      onOpenChange={onOpenChange}
    >
      <SelectPrimitive.Trigger
        id={id}
        className="capture-target-trigger"
        aria-labelledby={labelId}
        aria-busy={refreshing}
      >
        <SelectPrimitive.Value
          placeholder={(
            <TriggerValue
              target={placeholderTarget}
              unavailable={Boolean(placeholderTarget)}
            />
          )}
        >
          {selectedTarget ? <TriggerValue target={selectedTarget} /> : undefined}
        </SelectPrimitive.Value>
        <SelectPrimitive.Icon className="capture-target-chevron">
          <ChevronDown aria-hidden="true" />
        </SelectPrimitive.Icon>
      </SelectPrimitive.Trigger>

      <SelectPrimitive.Portal>
        <SelectPrimitive.Content
          className="capture-target-content"
          position="popper"
          align="start"
          sideOffset={6}
          collisionPadding={12}
        >
          <SelectPrimitive.ScrollUpButton className="capture-target-scroll-button">
            <ChevronUp aria-hidden="true" />
          </SelectPrimitive.ScrollUpButton>
          <SelectPrimitive.Viewport className="capture-target-viewport">
            {targets.length === 0 ? (
              <SelectPrimitive.Item
                className="capture-target-item empty"
                value="__no-capture-targets"
                disabled
              >
                <CaptureTargetIcon target={placeholderTarget} />
                <span className="capture-target-copy">
                  <SelectPrimitive.ItemText>
                    {placeholderTarget ? "所选应用未运行" : "没有找到可录制的应用"}
                  </SelectPrimitive.ItemText>
                  <span aria-hidden="true">请启动会议应用后刷新</span>
                </span>
              </SelectPrimitive.Item>
            ) : targets.map((target) => {
              const executableName = captureTargetExecutableName(target) || "未知程序";
              return (
                <SelectPrimitive.Item
                  className="capture-target-item"
                  value={target.id}
                  key={target.id}
                  textValue={target.displayName}
                  aria-label={captureTargetLabel(target)}
                  aria-labelledby={undefined}
                >
                  <CaptureTargetIcon target={target} />
                  <span className="capture-target-copy">
                    <SelectPrimitive.ItemText>{target.displayName}</SelectPrimitive.ItemText>
                    <span aria-hidden="true">{executableName}</span>
                  </span>
                  <SelectPrimitive.ItemIndicator className="capture-target-indicator">
                    <Check aria-hidden="true" />
                  </SelectPrimitive.ItemIndicator>
                </SelectPrimitive.Item>
              );
            })}
          </SelectPrimitive.Viewport>
          <SelectPrimitive.ScrollDownButton className="capture-target-scroll-button">
            <ChevronDown aria-hidden="true" />
          </SelectPrimitive.ScrollDownButton>
        </SelectPrimitive.Content>
      </SelectPrimitive.Portal>
    </SelectPrimitive.Root>
  );
}
