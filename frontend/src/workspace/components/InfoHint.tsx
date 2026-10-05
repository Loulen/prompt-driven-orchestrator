import { Info } from "lucide-react";
import { Tooltip, TooltipProvider } from "../../shared/ui/tooltip";

/** An info icon whose explanation is a Radix tooltip, reachable by hover and by
 *  keyboard focus. It brings its own provider: the editor is also mounted
 *  outside `App` (tests, isolated surfaces). */
export default function InfoHint({ label, content }: { label: string; content: string }) {
  return (
    <TooltipProvider>
      <Tooltip content={content} side="top">
        <button
          type="button"
          aria-label={label}
          className="inline-flex shrink-0 cursor-help items-center text-fg-4 hover:text-fg-2 focus-visible:text-fg-2"
        >
          <Info size={11} aria-hidden="true" />
        </button>
      </Tooltip>
    </TooltipProvider>
  );
}
