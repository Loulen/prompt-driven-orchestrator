import { Sparkles } from "lucide-react";
import type { SkillCompletionScope } from "./SkillCompletionList";

/** The discreet line under a prompt that completes skills. */
export default function SkillPromptHint({
  scope,
  lastAdded,
  testId,
}: {
  scope: SkillCompletionScope;
  lastAdded: string | null;
  testId: string;
}) {
  return (
    <p className="flex items-center gap-1 text-text-3" style={{ fontSize: "10px" }} data-testid={testId}>
      <Sparkles size={10} className="shrink-0 text-st-await" />
      {lastAdded
        ? `/${lastAdded} was added to this ${scope}'s skills.`
        : `Type / to insert a skill; one this ${scope} does not receive is added to its skills.`}
    </p>
  );
}
