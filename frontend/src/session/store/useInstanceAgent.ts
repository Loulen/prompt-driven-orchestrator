import { useEffect, useMemo, useState } from "react";
import { fetchSettings } from "../../settings/api/settings";
import type { AgentCombination } from "../../settings/model/settings";
import { harnessCatalog, type HarnessCatalog } from "../../harness/model/catalog";

/** The harness an agent runs on when its configuration names none. */
const HARNESS_FLOOR = "claude";

/**
 * The instance's harness catalogue and default harness (CORE-125): what an
 * agent Session hébergée inherits when its configuration names no harness.
 */
export function useInstanceAgent(): { catalog: HarnessCatalog; inherited: AgentCombination } {
  const [settings, setSettings] = useState<Awaited<ReturnType<typeof fetchSettings>> | null>(null);
  useEffect(() => {
    let alive = true;
    fetchSettings().then(
      (read) => alive && setSettings(read),
      () => undefined,
    );
    return () => {
      alive = false;
    };
  }, []);
  return useMemo(
    () => ({
      catalog: harnessCatalog(settings?.harness_descriptors ?? null),
      inherited: { harness: settings?.default_harness.effective || HARNESS_FLOOR, model: null, effort: null },
    }),
    [settings],
  );
}
