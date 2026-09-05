import { useRef, type MouseEvent, type PointerEvent } from "react";

/** Dismiss only a primary click that starts and ends on the backdrop. */
export function useBackdropDismiss(onDismiss: () => void, nativeDialog = false) {
  const startedOnBackdrop = useRef(false);
  const isBackdrop = (event: MouseEvent<HTMLElement>) => {
    if (event.target !== event.currentTarget) return false;
    if (!nativeDialog) return true;
    // Native ::backdrop events target the dialog; its own padding is not backdrop.
    const rect = event.currentTarget.getBoundingClientRect();
    return event.clientX < rect.left || event.clientX >= rect.right
      || event.clientY < rect.top || event.clientY >= rect.bottom;
  };

  return {
    onPointerDown(event: PointerEvent<HTMLElement>) {
      startedOnBackdrop.current = event.button === 0 && isBackdrop(event);
    },
    onPointerCancel() {
      startedOnBackdrop.current = false;
    },
    onClick(event: MouseEvent<HTMLElement>) {
      const dismiss = startedOnBackdrop.current && event.button === 0 && isBackdrop(event);
      startedOnBackdrop.current = false;
      if (dismiss) onDismiss();
    },
  };
}
