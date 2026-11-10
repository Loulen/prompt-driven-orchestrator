/**
 * The banner above the canvas: the parse's lint, missing profiles and skills, and the dismissible fan-out nudges (#268).
 */

import type { OpenPipeline } from "./editState";
import { useMemo } from "react";
import { collectionFanoutNudges } from "../../lib/loopRegions";
import type { LintBannerItem } from "../components/LintBanner";
import { useDismissedNudges } from "../../hooks/useDismissedNudges";
import { profileLabel } from "../../settings/model/profileNames";
// #723 — orchestrator pastilles on the run-view node card.

export function useCanvasLint(tab: OpenPipeline | undefined, agentProfileIds: Set<string>, skillIds: Set<string>, skillBankLoaded: boolean) {
  // #268: advisory fan-out nudges are dismissible (persisted per pipeline);
  // correctness lint is not. `tab?.id ?? ""` keeps the hook call unconditional
  // (rules of hooks) ahead of the early return below; "" is never rendered.
  const { dismissed, dismiss } = useDismissedNudges(tab?.id ?? "");
  const allItems = useMemo<LintBannerItem[]>(() => {
    if (!tab) return [];
    const lint = (tab.diagnostics ?? []).map((m, i) => ({
      id: `lint:${i}`,
      kind: "lint" as const,
      message: m,
    }));
    const nudges = collectionFanoutNudges(tab.pipeline).map((n) => ({
      ...n,
      kind: "nudge" as const,
    }));
    const missingProfiles = tab.pipeline.nodes.flatMap((node) => {
      const choice = node.agent_choice;
      if (choice?.mode !== "profile" || agentProfileIds.has(choice.profile_id)) return [];
      return [{
        id: `agent-profile:${node.id}:${choice.profile_id}`,
        kind: "lint" as const,
        message: `Agent profile ${profileLabel(choice.profile_id)} no longer exists. Node ${node.name ?? node.id} falls back to the next tier. Pick a profile or set Custom.`,
      }];
    });
    // #669/ADR-0062: a node selecting a skill the bank no longer has. A warning,
    // never a refusal — the node runs without it. Only once the bank has loaded,
    // or every skill would flash as missing on the first render.
    const missingSkills = !skillBankLoaded
      ? []
      : tab.pipeline.nodes.flatMap((node) =>
          (node.skills ?? [])
            .filter((skill) => !skillIds.has(skill.id))
            .map((skill) => ({
              id: `skill:${node.id}:${skill.id}`,
              kind: "lint" as const,
              message: `Skill ${skill.name || skill.id} no longer exists in the bank. Node ${node.name ?? node.id} runs without it; the pipeline still launches.`,
            })),
        );
    return [...lint, ...missingProfiles, ...missingSkills, ...nudges];
  }, [tab, agentProfileIds, skillIds, skillBankLoaded]);
  // Filter BEFORE the render gate so dismissing the last nudge (with no lint)
  // collapses the whole overlay. MUST depend on `dismissed` or it won't update.
  const visibleItems = useMemo(
    () => allItems.filter((it) => it.kind === "lint" || !dismissed.has(it.id)),
    [allItems, dismissed],
  );

  return { visibleItems, dismiss };
}
