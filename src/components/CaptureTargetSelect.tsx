import { useCallback, useEffect, useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import { createPortal } from "react-dom";
import { AppWindow, Check, ChevronDown, EyeOff } from "lucide-react";
import {
  captureTargetExecutableName,
  captureTargetLabel,
  type CaptureTargetPreference,
} from "../captureTargets";
import type { CaptureTarget } from "../types";
import { AppTooltip } from "./AppTooltip";

interface CaptureTargetSelectProps {
  id: string;
  labelId: string;
  targets: CaptureTarget[];
  value: string;
  unavailableTarget: CaptureTargetPreference | null;
  refreshing: boolean;
  hiddenCount: number;
  onValueChange: (value: string) => void;
  onOpenChange: (open: boolean) => void;
  onHide: (target: CaptureTarget) => Promise<void>;
  onManageHidden: () => void;
}

type TargetIdentity = Pick<
  CaptureTarget,
  "displayName" | "executablePath" | "iconDataUrl"
>;

function CaptureTargetIcon({ target }: { target: TargetIdentity | null }) {
  return target?.iconDataUrl ? (
    <img className="capture-target-icon" src={target.iconDataUrl} alt="" aria-hidden="true" />
  ) : (
    <span className="capture-target-icon fallback" aria-hidden="true"><AppWindow /></span>
  );
}

export function CaptureTargetSelect(props: CaptureTargetSelectProps) {
  const [open, setOpen] = useState(false);
  const [hiding, setHiding] = useState(false);
  const [position, setPosition] = useState<CSSProperties>({});
  const triggerRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const targetButtonsRef = useRef(new Map<string, HTMLButtonElement>());
  const typeaheadRef = useRef({ text: "", at: 0 });
  const hideFocusIndexRef = useRef<number | null>(null);
  const onOpenChangeRef = useRef(props.onOpenChange);
  onOpenChangeRef.current = props.onOpenChange;
  const selected = props.targets.find((target) => target.id === props.value);
  const identity = selected ?? props.unavailableTarget;
  const label = identity
    ? `${captureTargetLabel(identity)}${!selected ? "（未运行）" : ""}`
    : "请选择要录制的应用";

  const close = useCallback((restoreFocus = true) => {
    setOpen(false);
    if (restoreFocus) triggerRef.current?.focus();
  }, []);

  useEffect(() => {
    onOpenChangeRef.current(open);
  }, [open]);

  useLayoutEffect(() => {
    if (!open) return;
    const place = () => {
      const anchor = triggerRef.current?.getBoundingClientRect();
      if (!anchor) return;
      const zoom = [document.documentElement, document.body].reduce(
        (scale, element) => scale * (Number.parseFloat(getComputedStyle(element).getPropertyValue("zoom")) || 1),
        1,
      );
      const viewportWidth = window.innerWidth / zoom;
      const viewportHeight = window.innerHeight / zoom;
      const gap = 6;
      const gutter = 12;
      const width = Math.min(Math.max(anchor.width / zoom, 320), 480, viewportWidth - gutter * 2);
      const below = viewportHeight - anchor.bottom / zoom - gap - gutter;
      const above = anchor.top / zoom - gap - gutter;
      const useBelow = below >= Math.min(360, above);
      const maxHeight = Math.max(80, Math.min(360, useBelow ? below : above));
      setPosition({
        left: Math.max(gutter, Math.min(anchor.left / zoom, viewportWidth - width - gutter)),
        top: useBelow ? anchor.bottom / zoom + gap : undefined,
        bottom: useBelow ? undefined : viewportHeight - anchor.top / zoom + gap,
        width,
        maxHeight,
      });
    };
    place();
    window.addEventListener("resize", place);
    window.addEventListener("scroll", place, true);
    return () => {
      window.removeEventListener("resize", place);
      window.removeEventListener("scroll", place, true);
    };
  }, [open]);

  useLayoutEffect(() => {
    if (!open) return;
    typeaheadRef.current = { text: "", at: 0 };
    const selectedButton = panelRef.current?.querySelector<HTMLButtonElement>("[aria-pressed='true']");
    const firstButton = panelRef.current?.querySelector<HTMLButtonElement>("button");
    (selectedButton ?? firstButton)?.focus();
  }, [open]);

  useLayoutEffect(() => {
    const index = hideFocusIndexRef.current;
    if (!open || index === null) return;
    if (!hiding || document.activeElement === document.body) {
      const remaining = [...targetButtonsRef.current.values()];
      (remaining[Math.min(index, remaining.length - 1)] ??
        panelRef.current?.querySelector<HTMLButtonElement>(".capture-target-manage"))?.focus();
    }
    if (!hiding) hideFocusIndexRef.current = null;
  }, [hiding, open, props.targets]);

  useEffect(() => {
    if (!open) return;
    const dismiss = (event: PointerEvent) => {
      if (
        !panelRef.current?.contains(event.target as Node) &&
        !triggerRef.current?.contains(event.target as Node)
      ) close(false);
    };
    document.addEventListener("pointerdown", dismiss);
    return () => document.removeEventListener("pointerdown", dismiss);
  }, [close, open]);

  const focusTarget = (target: CaptureTarget | undefined) => {
    if (!target) return;
    const button = targetButtonsRef.current.get(target.id);
    button?.focus();
    button?.scrollIntoView({ block: "nearest" });
  };

  const hideTarget = async (target: CaptureTarget, index: number) => {
    hideFocusIndexRef.current = index;
    setHiding(true);
    try {
      await props.onHide(target);
    } finally {
      setHiding(false);
    }
  };

  return (
    <>
      <button
        type="button"
        id={props.id}
        ref={triggerRef}
        className="capture-target-trigger"
        aria-labelledby={props.labelId}
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-controls={open ? `${props.id}-panel` : undefined}
        aria-busy={props.refreshing}
        onClick={() => setOpen((current) => !current)}
        onKeyDown={(event) => {
          if (["ArrowDown", "ArrowUp"].includes(event.key)) {
            event.preventDefault();
            setOpen(true);
          } else if (event.key === "Escape") {
            close();
          }
        }}
      >
        <span className="capture-target-trigger-value">
          <CaptureTargetIcon target={identity} />
          <span>{label}</span>
        </span>
        <span className="capture-target-chevron"><ChevronDown aria-hidden="true" /></span>
      </button>
      {open && createPortal(
        <div
          id={`${props.id}-panel`}
          ref={panelRef}
          className="capture-target-content"
          role="dialog"
          aria-label="选择录音来源"
          style={position}
          onBlur={(event) => {
            if (
              (!event.relatedTarget && !hiding) ||
              (event.relatedTarget instanceof Node &&
                !event.currentTarget.contains(event.relatedTarget) &&
                event.relatedTarget !== triggerRef.current)
            ) close(false);
          }}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.preventDefault();
              event.stopPropagation();
              close();
              return;
            }
            const index = props.targets.findIndex((target) =>
              event.target instanceof Element &&
              event.target.closest("li")?.dataset.targetId === target.id,
            );
            if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
              event.preventDefault();
              const next = event.key === "Home" ? 0
                : event.key === "End" ? props.targets.length - 1
                  : (index + (event.key === "ArrowDown" ? 1 : -1) + props.targets.length)
                    % props.targets.length;
              focusTarget(props.targets[next]);
            } else if (
              event.key.length === 1 && event.key !== " " &&
              !event.ctrlKey && !event.metaKey && !event.altKey
            ) {
              const now = Date.now();
              const text = (now - typeaheadRef.current.at < 700 ? typeaheadRef.current.text : "") +
                event.key.toLowerCase();
              typeaheadRef.current = { text, at: now };
              const match = props.targets.find((target) =>
                target.displayName.toLowerCase().startsWith(text) ||
                captureTargetExecutableName(target).toLowerCase().startsWith(text),
              );
              if (match) {
                event.preventDefault();
                focusTarget(match);
              }
            }
          }}
        >
          <ul className="capture-target-viewport" aria-label="可录制的应用">
            {props.targets.length === 0 ? (
              <li className="capture-target-empty">
                <span>{props.unavailableTarget ? "所选应用未运行" : "没有找到可录制的应用"}</span>
                <span>请启动会议应用后刷新，或管理已隐藏应用。</span>
              </li>
            ) : props.targets.map((target, index) => (
              <li className="capture-target-row" key={target.id} data-target-id={target.id}>
                <button
                  type="button"
                  className="capture-target-item"
                  aria-label={captureTargetLabel(target)}
                  aria-pressed={target.id === props.value}
                  ref={(element) => {
                    if (element) targetButtonsRef.current.set(target.id, element);
                    else targetButtonsRef.current.delete(target.id);
                  }}
                  onClick={() => {
                    props.onValueChange(target.id);
                    close();
                  }}
                >
                  <CaptureTargetIcon target={target} />
                  <span className="capture-target-copy">
                    <span>{target.displayName}</span>
                    <span>{captureTargetExecutableName(target) || "未知程序"}</span>
                  </span>
                  <span className="capture-target-indicator">
                    {target.id === props.value && <Check aria-hidden="true" />}
                  </span>
                </button>
                <AppTooltip content={`隐藏 ${captureTargetExecutableName(target)}`}>
                  <button
                    type="button"
                    className="icon-button compact"
                    aria-label={`隐藏 ${captureTargetExecutableName(target)}`}
                    disabled={hiding || !captureTargetExecutableName(target)}
                    onClick={() => void hideTarget(target, index)}
                  ><EyeOff /></button>
                </AppTooltip>
              </li>
            ))}
          </ul>
          <button
            type="button"
            className="capture-target-manage"
            onClick={() => {
              close(false);
              props.onManageHidden();
            }}
          >管理已隐藏应用（{props.hiddenCount}）</button>
        </div>,
        document.body,
      )}
    </>
  );
}
