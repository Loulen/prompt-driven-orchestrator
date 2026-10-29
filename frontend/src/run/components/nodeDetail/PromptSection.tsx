import type { NodeStatus } from "../../../shared/generated/NodeStatus";
import { Section } from "../../../shared/ui/kit";

/** The node's initial prompt, folded at first. */
export default function PromptSection({
  promptText,
  status,
  isArchived,
}: {
  promptText: string | null;
  status: NodeStatus;
  isArchived?: boolean;
}) {
  return (
    <div data-testid="prompt-section">
      <Section title="Prompt" closed flush>
        <pre className="prompt-block ri-code">
          {promptText ?? (
            <span className="text-text-3">
              {isArchived ? "Not preserved" : status === "pending" ? "After the node starts" : "Loading…"}
            </span>
          )}
        </pre>
      </Section>
    </div>
  );
}
