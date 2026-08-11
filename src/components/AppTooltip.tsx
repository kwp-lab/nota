import * as TooltipPrimitive from "@radix-ui/react-tooltip";
import { createContext, useContext, type ReactElement, type ReactNode } from "react";

const AppTooltipProviderContext = createContext(false);

interface AppTooltipProps {
  content: ReactNode;
  children: ReactElement;
  side?: "top" | "right" | "bottom" | "left";
  align?: "start" | "center" | "end";
  wrapDisabled?: boolean;
}

export function AppTooltipProvider({ children }: { children: ReactNode }) {
  return (
    <AppTooltipProviderContext.Provider value>
      <TooltipPrimitive.Provider delayDuration={400} skipDelayDuration={150}>
        {children}
      </TooltipPrimitive.Provider>
    </AppTooltipProviderContext.Provider>
  );
}

export function AppTooltip({
  content,
  children,
  side = "top",
  align = "center",
  wrapDisabled = false,
}: AppTooltipProps) {
  const hasProvider = useContext(AppTooltipProviderContext);
  if (!content) return children;
  const trigger = wrapDisabled
    ? <span className="app-tooltip-disabled-trigger">{children}</span>
    : children;

  const tooltip = (
    <TooltipPrimitive.Root>
      <TooltipPrimitive.Trigger asChild>{trigger}</TooltipPrimitive.Trigger>
      <TooltipPrimitive.Portal>
        <TooltipPrimitive.Content
          className="app-tooltip-content"
          side={side}
          align={align}
          sideOffset={7}
          collisionPadding={10}
        >
          {content}
          <TooltipPrimitive.Arrow className="app-tooltip-arrow" width={9} height={5} />
        </TooltipPrimitive.Content>
      </TooltipPrimitive.Portal>
    </TooltipPrimitive.Root>
  );
  return hasProvider ? tooltip : (
    <TooltipPrimitive.Provider delayDuration={400} skipDelayDuration={150}>
      {tooltip}
    </TooltipPrimitive.Provider>
  );
}
