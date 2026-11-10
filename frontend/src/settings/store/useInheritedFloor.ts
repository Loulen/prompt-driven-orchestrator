import { useEffect, useMemo, useState } from "react";
import { fetchSettings } from "../api/settings";
import { inheritedFloor, instanceFloor, type NodeHarnessEntries } from "../model/agentProfiles";
import type { AgentCombination, InstanceSettings } from "../model/settings";
import type { AgentProfile } from "../../shared/generated/AgentProfile";

/** What « Inherit » falls to on a surface that does not already hold
 *  `GET /settings` (node inspector, Projet): the instance's choice, its legacy
 *  default harness, then the Default profile — read once on mount. A node's
 *  own legacy entries (`node`) apply as the daemon reads them. */
export function useInheritedFloor(profiles: AgentProfile[], node?: NodeHarnessEntries): AgentCombination {
  const [settings, setSettings] = useState<InstanceSettings | null>(null);
  useEffect(() => {
    let live = true;
    fetchSettings()
      .then((read) => {
        if (live) setSettings(read);
      })
      .catch(() => {
        /* the Default profile alone, until a read succeeds */
      });
    return () => {
      live = false;
    };
  }, []);
  return useMemo(
    () => inheritedFloor(profiles, instanceFloor(settings), node),
    [profiles, settings, node],
  );
}
