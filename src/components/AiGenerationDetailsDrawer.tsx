import { useEffect, useRef, type ReactNode } from "react";
import { X } from "lucide-react";
import { AppTooltip } from "./AppTooltip";
import { useBackdropDismiss } from "./useBackdropDismiss";

export function AiGenerationDetailsDrawer(props: { onClose: () => void; children: ReactNode }) {
  const ref = useRef<HTMLDialogElement>(null);
  const backdrop = useBackdropDismiss(props.onClose, true);
  useEffect(() => {
    const dialog = ref.current!;
    const trigger = document.activeElement as HTMLElement | null;
    dialog.showModal();
    return () => { dialog.close(); trigger?.focus(); };
  }, []);
  return (
    <dialog ref={ref} className="ai-details-drawer" aria-labelledby="ai-details-title" {...backdrop}
      onCancel={(event) => { event.preventDefault(); event.stopPropagation(); props.onClose(); }}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          event.stopPropagation();
          props.onClose();
        }
      }}>
      <header className="ai-details-drawer-header">
        <h2 id="ai-details-title">生成详情</h2>
        <AppTooltip content="关闭生成详情"><button className="icon-button" aria-label="关闭生成详情" onClick={props.onClose}><X size={16} /></button></AppTooltip>
      </header>
      {props.children}
    </dialog>
  );
}
