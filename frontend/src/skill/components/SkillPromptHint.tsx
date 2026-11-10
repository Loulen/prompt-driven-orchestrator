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
    <p className="ln-skillhint" data-testid={testId}>
      <Sparkles size={12} strokeWidth={1.5} className="ln-skillhint__ic" aria-hidden />
      {lastAdded
        ? `/${lastAdded} was added to this ${scope}'s skills.`
        : `Type / to insert a skill; one this ${scope} does not receive is added to its skills.`}
    </p>
  );
}
