import { Info } from "lucide-react";
import { Tooltip } from "../../shared/ui/tooltip";

/** An info icon whose explanation is a Radix tooltip, reachable by hover and by
 *  keyboard focus. It brings its own provider: the editor is also mounted
 *  outside `App` (tests, isolated surfaces). */
export default function InfoHint({ label, content }: { label: string; content: string }) {
  return (
    <>
      <Tooltip content={content} side="top">
        <button
          type="button"
          aria-label={label}
          className="inline-flex shrink-0 cursor-help items-center text-text-3 hover:text-text-2 focus-visible:text-text-2"
        >
          <Info size={11} aria-hidden="true" />
        </button>
      </Tooltip>
    </>
  );
}
