import {
  CircleAlert,
  CircleCheck,
  Info,
  TriangleAlert,
  X,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";

export type ToastTone = "success" | "info" | "warning" | "error";

export interface AppToast {
  id: number;
  message: string;
  tone: ToastTone;
  durationMs: number | null;
  dedupeKey?: string;
  action?: {
    label: string;
    onClick: () => void;
  };
}

interface ToastRegionProps {
  toasts: AppToast[];
  onDismiss: (id: number) => void;
}

const MAX_VISIBLE_TOASTS = 3;
const EXIT_ANIMATION_MS = 160;

const toneDetails = {
  success: { label: "完成", icon: CircleCheck },
  info: { label: "提示", icon: Info },
  warning: { label: "请注意", icon: TriangleAlert },
  error: { label: "操作未完成", icon: CircleAlert },
} satisfies Record<
  ToastTone,
  { label: string; icon: typeof CircleCheck }
>;

export function enqueueToast(current: AppToast[], next: AppToast): AppToast[] {
  if (
    next.dedupeKey &&
    current.some((toast) => toast.dedupeKey === next.dedupeKey)
  ) {
    return current;
  }

  const queued = [...current, next];
  if (queued.length <= MAX_VISIBLE_TOASTS) {
    return queued;
  }

  const transientIndex = queued
    .slice(0, -1)
    .findIndex((toast) => toast.durationMs !== null);
  if (transientIndex >= 0) {
    queued.splice(transientIndex, 1);
    return queued;
  }

  if (next.durationMs !== null) {
    return current;
  }

  return queued.slice(1);
}

function ToastItem({
  toast,
  onDismiss,
}: {
  toast: AppToast;
  onDismiss: (id: number) => void;
}) {
  const [closing, setClosing] = useState(false);
  const closingRef = useRef(false);
  const removalTimerRef = useRef<number | null>(null);
  const details = toneDetails[toast.tone];
  const Icon = details.icon;
  const persistent = toast.durationMs === null;
  const urgent = toast.tone === "warning" || toast.tone === "error";

  const beginDismiss = useCallback(() => {
    if (closingRef.current) return;
    closingRef.current = true;
    setClosing(true);
    removalTimerRef.current = window.setTimeout(
      () => onDismiss(toast.id),
      EXIT_ANIMATION_MS,
    );
  }, [onDismiss, toast.id]);

  useEffect(() => {
    if (toast.durationMs === null) return;
    const timer = window.setTimeout(beginDismiss, toast.durationMs);
    return () => window.clearTimeout(timer);
  }, [beginDismiss, toast.durationMs]);

  useEffect(
    () => () => {
      if (removalTimerRef.current !== null) {
        window.clearTimeout(removalTimerRef.current);
      }
    },
    [],
  );

  return (
    <article
      className={`toast-card toast-${toast.tone}${closing ? " toast-closing" : ""}`}
      role={urgent ? "alert" : "status"}
      aria-live={urgent ? "assertive" : "polite"}
      aria-atomic="true"
    >
      <span className="toast-icon" aria-hidden="true">
        <Icon size={18} strokeWidth={2.1} />
      </span>
      <span className="toast-copy">
        <strong>{details.label}</strong>
        <span>{toast.message}</span>
        {toast.action && (
          <button
            type="button"
            className="toast-action"
            onClick={() => {
              toast.action?.onClick();
              beginDismiss();
            }}
          >
            {toast.action.label}
          </button>
        )}
      </span>
      {persistent && (
        <button
          type="button"
          className="toast-close"
          aria-label="关闭通知"
          onClick={beginDismiss}
        >
          <X size={16} />
        </button>
      )}
    </article>
  );
}

export function ToastRegion({ toasts, onDismiss }: ToastRegionProps) {
  if (toasts.length === 0) return null;

  return (
    <section className="toast-region" aria-label="应用通知">
      {toasts.map((toast) => (
        <ToastItem key={toast.id} toast={toast} onDismiss={onDismiss} />
      ))}
    </section>
  );
}
