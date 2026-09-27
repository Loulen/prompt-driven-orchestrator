import { useState } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import type { NodeStatus } from "../../../shared/generated/NodeStatus";

export default function PromptSection({
  promptText,
  status,
  isArchived,
}: {
  promptText: string | null;
  status: NodeStatus;
  isArchived?: boolean;
}) {
  const [expanded, setExpanded] = useState(false);

  return (
    <div className="border-t border-line">
      <button
        type="button"
        onClick={() => setExpanded(!expanded)}
        className="flex w-full cursor-pointer items-center gap-1.5 px-3 py-1.5 text-fg-3 transition-colors hover:text-fg-2"
        style={{ fontSize: "11px" }}
        data-testid="prompt-toggle"
      >
        {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        Initial Prompt
      </button>
      {expanded && (
        <pre
          className="prompt-block overflow-auto bg-bg-0 p-2 font-mono text-fg-3"
          style={{ fontSize: "10px", lineHeight: "1.5" }}
        >
          {promptText ?? (
            <span className="text-fg-4">
              {isArchived
                ? "Prompt not preserved for archived runs."
                : status === "pending"
                  ? "Prompt available after node starts."
                  : "Loading prompt..."}
            </span>
          )}
        </pre>
      )}
    </div>
  );
}
