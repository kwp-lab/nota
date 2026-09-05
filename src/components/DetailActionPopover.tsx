import { useEffect, useRef, type ReactNode } from "react";
import { MoreHorizontal } from "lucide-react";
import { AppTooltip } from "./AppTooltip";

/** Low-frequency actions keep native disclosure keyboard behavior. */
export function DetailActionPopover(props: { label: string; children: ReactNode }) {
  const ref = useRef<HTMLDetailsElement>(null);
  useEffect(() => {
    const close = (event: PointerEvent) => {
      if (!ref.current?.contains(event.target as Node)) ref.current?.removeAttribute("open");
    };
    document.addEventListener("pointerdown", close);
    return () => document.removeEventListener("pointerdown", close);
  }, []);
  return (
    <details ref={ref} className="detail-action-popover" onKeyDown={(event) => {
      if (event.key === "Escape" && ref.current?.open) {
        event.stopPropagation();
        ref.current.open = false;
        ref.current.querySelector("summary")?.focus();
      }
    }}>
      <AppTooltip content={props.label}>
        <summary className="icon-button" aria-label={props.label}><MoreHorizontal size={16} /></summary>
      </AppTooltip>
      <div className="detail-action-popover-content" onClick={(event) => {
        if ((event.target as HTMLElement).closest("button:not(:disabled)") && ref.current) {
          ref.current.open = false;
          ref.current.querySelector("summary")?.focus();
        }
      }}>{props.children}</div>
    </details>
  );
}
