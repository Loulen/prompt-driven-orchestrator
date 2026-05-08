import * as React from "react";
import * as TooltipPrimitive from "@radix-ui/react-tooltip";

export function TooltipProvider({ children }: { children: React.ReactNode }) {
  return (
    <TooltipPrimitive.Provider delayDuration={300}>
      {children}
    </TooltipPrimitive.Provider>
  );
}

export function Tooltip({
  content,
  children,
  delay,
  side = "bottom",
}: {
  content: string;
  children: React.ReactNode;
  delay?: number;
  side?: "top" | "bottom" | "left" | "right";
}) {
  return (
    <TooltipPrimitive.Provider delayDuration={delay ?? 300}>
      <TooltipPrimitive.Root delayDuration={delay}>
        <TooltipPrimitive.Trigger asChild>{children}</TooltipPrimitive.Trigger>
        <TooltipPrimitive.Portal>
          <TooltipPrimitive.Content
            side={side}
            sideOffset={6}
            className="z-50 rounded border border-line bg-bg-4 px-2 py-1 text-fg shadow-lg"
            style={{ fontSize: "11px", maxWidth: 260, lineHeight: 1.4 }}
            data-testid="tooltip-content"
          >
            {content}
          </TooltipPrimitive.Content>
        </TooltipPrimitive.Portal>
      </TooltipPrimitive.Root>
    </TooltipPrimitive.Provider>
  );
}
