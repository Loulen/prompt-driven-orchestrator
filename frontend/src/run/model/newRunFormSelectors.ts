import type { InstanceSettings } from "../../settings/model/settings";
import type { SandboxProfileRef } from "../../sandbox/model/sandbox";
import { HARNESS_FLOOR, harnessCatalog, type HarnessCatalog } from "../../harness/model/catalog";

/**
 * The New Run / Trigger form's selector clusters (sandbox, harness), split out of
 * `newRunForm.ts`: what each selector derives from `settings` + the selected value.
 */

/** Everything the sandbox selector derives from `settings` + the selected value (ADR-0031). */
export interface SandboxState {
  dockerUnavailable: boolean;
  sandboxReason: string | undefined;
  sandboxProfiles: SandboxProfileRef[];
  missingProfile: boolean;
  instanceDefaultSandbox: string | null;
  effectiveSandbox: string | null;
  sandboxDoomed: boolean;
  inheritedDefaultReason: string | null;
}

/**
 * The sandbox cluster (#410/#432/#452, ADR-0031 §7). `sandbox` is the selector value in
 * BOTH modes: `""` = "the user did not choose", `off`, or a staging profile name.
 */
export function sandboxState({
  settings,
  sandbox,
  mode,
}: {
  settings: InstanceSettings | null;
  sandbox: string;
  mode: "run" | "trigger";
}): SandboxState {
  // #410: advisory Docker greying. Only gate the sandboxed options once we KNOW Docker is
  // unavailable (settings loaded && probe false); while settings load, stay
  // optimistic. `sandboxReason` explains the greying (title + help text).
  const dockerUnavailable = settings != null && !settings.sandbox_docker.available;
  const sandboxReason = settings?.sandbox_docker.reason ?? undefined;

  // #432: the options come from the daemon's profile list. Sorted server-side.
  const sandboxProfiles = settings?.sandbox_profiles ?? [];

  /**
   * THE PHANTOM-PROFILE RULE. A seeded value is **never** silently rewritten: a
   * non-empty, non-`off` value that is absent from the list gets a tombstone option and
   * blocks Save/Launch.
   *
   * Without the tombstone React sets `selectedIndex = -1`, the field renders blank, and
   * saving would PATCH `sandbox: null` — a **silent fallback to the instance default**,
   * exactly what ADR-0031 §7 forbids. Deliberately separate from the Docker clamp above:
   * clamping to `off` is legitimate for an unavailable Docker, and would be a silent
   * fallback for a missing profile.
   */
  const missingProfile = Boolean(
    settings &&
      sandbox &&
      sandbox !== "off" &&
      !sandboxProfiles.some((p) => p.name === sandbox),
  );

  // #452: the instance default, used to LABEL the inherit option — never to seed the value.
  // `null` while settings are unknown, which is the honest rendering: we do not know yet.
  const instanceDefaultSandbox = settings ? (settings.default_sandbox.effective ?? "off") : null;

  // #452: what will actually apply to the Run being created. `""` is not a value — it means
  // the key is omitted and the instance default decides — so the checks below have to
  // resolve it to say anything true about the Run.
  const effectiveSandbox = sandbox === "" ? instanceDefaultSandbox : sandbox;

  /**
   * #452, the Docker clamp, relocated. A Run whose effective sandbox is a profile while the
   * daemon reports Docker unavailable is born condemned, so #410 clamped the SELECTOR to
   * `off`. That protection was right and is kept; the way it was applied was not — it wrote
   * a business verdict into the field, indistinguishable from a user picking `off`, and
   * posted it explicitly.
   *
   * So refuse the launch and say why, instead of quietly substituting an answer. Same rule
   * as the phantom-profile tombstone above, for the same reason: the app never demotes a
   * sandbox behind the user's back (ADR-0031 §7).
   *
   * Run mode only. A Trigger resolves its sandbox when it FIRES, and today's Docker probe
   * says nothing about that moment.
   */
  const sandboxDoomed = Boolean(
    mode === "run" && dockerUnavailable && effectiveSandbox && effectiveSandbox !== "off",
  );

  // #452: `default_sandbox` carries a `reason` when the winning tier names a profile that
  // does not resolve. Inheriting is now the default path in run mode, so that dangling name
  // is worth showing here — the create chokepoint 400s on it rather than falling back.
  const inheritedDefaultReason = sandbox === "" ? (settings?.default_sandbox.reason ?? null) : null;

  return {
    dockerUnavailable,
    sandboxReason,
    sandboxProfiles,
    missingProfile,
    instanceDefaultSandbox,
    effectiveSandbox,
    sandboxDoomed,
    inheritedDefaultReason,
  };
}

/** Everything the harness selector derives from `settings` + the selected value (#551, ADR-0046). */
export interface HarnessState {
  /** The harnesses the selector offers, split into Built-in / From-descriptors
   *  sections with per-harness install state (#586, dynamic from `/settings`). */
  catalog: HarnessCatalog;
  /**
   * The instance default harness, used to **label** the inherit option — never to seed
   * the value (the #452 trap: a prefilled field freezes a stale value the user never
   * chose). `null` while `settings` are unknown (the honest "we don't know yet" render).
   * Resolves through the floor when the instance names none, so run mode always has a
   * concrete name to show.
   */
  instanceDefaultHarness: string | null;
  /**
   * What will actually apply to the Run being created. `""` is not a value — it means the
   * key is omitted and the instance default (then the floor) decides — so it resolves to
   * `instanceDefaultHarness`. A concrete selection is itself.
   */
  effectiveHarness: string | null;
}

/**
 * The harness cluster (#551, ADR-0046). `harness` is the selector value in BOTH modes:
 * `""` = "the user did not choose" (inherit), or a concrete harness name. Mirror of
 * {@link sandboxState}, minus the Docker/profile machinery — a harness name is free text
 * with no availability probe (ADR-0045: PDO does not validate it; an unknown one fails
 * fast at spawn), so there is nothing to grey and nothing to block.
 */
export function harnessState({
  settings,
  harness,
}: {
  settings: InstanceSettings | null;
  harness: string;
}): HarnessState {
  // #452: the instance default LABELS the inherit option, never seeds the value. `null`
  // while settings are unknown; resolves through the `claude` floor once known, so run
  // mode always names a concrete harness the Run would inherit.
  const instanceDefaultHarness = settings
    ? (settings.default_harness.effective || HARNESS_FLOOR)
    : null;
  const effectiveHarness = harness === "" ? instanceDefaultHarness : harness;
  return {
    // #586: the picker's options are now dynamic — the floor merged with the disk
    // descriptor tier, each tagged installed/not. `harnessCatalog` falls back to
    // the embedded floor while settings are unknown, so the control is never empty.
    catalog: harnessCatalog(settings?.harness_descriptors ?? null),
    instanceDefaultHarness,
    effectiveHarness,
  };
}
