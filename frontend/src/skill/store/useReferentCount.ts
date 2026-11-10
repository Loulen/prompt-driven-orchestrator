import { useEffect, useState } from "react";
import { fetchSkillReferents } from "../api/bank";
import { referentCount } from "../model/skill";

/**
 * How many tiers select the skill `skillId`, read from the daemon (CORE-232: the
 * skill detail said a literal 0); `null` until read, or when it cannot be.
 */
export function useReferentCount(skillId: string): number | null {
  const [read, setRead] = useState<{ id: string; count: number } | null>(null);
  useEffect(() => {
    let cancelled = false;
    // Inside the chain, so a fetch that throws synchronously (or answers nothing)
    // lands in the `catch` like a rejection: the line is then left out.
    Promise.resolve()
      .then(() => fetchSkillReferents(skillId))
      .then((r) => {
        if (!cancelled) setRead({ id: skillId, count: referentCount(r) });
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [skillId]);
  return read && read.id === skillId ? read.count : null;
}
