/**
 * Hand-written shapes of the instance-settings payloads that have no generated type yet:
 * `GET /settings` is the union of each context's section (`<context>::adapters::
 * instance_defaults`, assembled by the settings context), serialized as a JSON object.
 */

import type { AgentChoice } from "../../shared/generated/AgentChoice";
import type { OtlpHeaderView } from "../../shared/generated/OtlpHeaderView";
import type { SkillRef } from "../../shared/generated/SkillRef";
import type { HarnessDescriptorsView } from "../../harness/model/harness";
import type { SandboxProfileRef } from "../../sandbox/model/sandbox";

/** Which tier won for an instance-config knob (#129, ADR-0015). */
export type SettingSource = "stored" | "env" | "default";

/**
 * One instance-config knob as `GET /settings` discloses it (#129, ADR-0015):
 * the `effective` value the daemon uses, the winning `source` tier, and each
 * tier's raw value so the UI can *reveal* a shadowed env var. Values are in the
 * knob's canonical unit (count for the cap, seconds for the TTL and guard
 * timeout) — except `guard_timeout_secs.env`, which is the raw
 * `PDO_GUARD_TIMEOUT_MS` value in milliseconds.
 */
export interface SettingField {
  effective: number;
  source: SettingSource;
  stored: number | null;
  env: number | null;
  default: number;
}

/**
 * String sibling of {@link SettingField} for the instance `default_model`
 * (#347). Every tier is a string-or-null: there is no baked-in default model,
 * so `default` is always `null` (the account default = no `--model`).
 */
export interface StringSettingField {
  effective: string | null;
  source: SettingSource;
  stored: string | null;
  env: string | null;
  default: string | null;
}

/**
 * String sibling of {@link StringSettingField} carrying an advisory `reason` (#432).
 *
 * Only `default_sandbox` needs it: it is the one enum knob whose value space is OPEN (a
 * staging-profile name), so it is the one that can point at nothing. `PUT /settings` gates
 * the *stored* tier, but the **env** tier (`PDO_DEFAULT_SANDBOX`) passes through no
 * validator at all — so the settings view is the only honest place to surface a dangling
 * reference before the user launches something that will 400.
 *
 * `reason` is `null` when the winning tier resolves.
 */
export interface EnumSettingFieldWithReason extends StringSettingField {
  reason: string | null;
}

/**
 * Boolean sibling of {@link SettingField} for a checkbox knob (#469
 * `autocomplete_turn_end`). Every tier is a bool-or-null, and unlike
 * {@link StringSettingField} the `default` tier is a real value (`false`).
 */
export interface BoolSettingField {
  effective: boolean;
  source: SettingSource;
  stored: boolean | null;
  env: boolean | null;
  default: boolean;
}

/**
 * The full `GET /settings` view (#129, ADR-0015; default_model #347).
 *
 * `default_sandbox` is the ONLY sandbox knob here since #471 — one axis per screen: this screen
 * answers *which profile a Run takes by default*, and a staging profile answers *what the sandbox
 * is* (its image, its home content, its env).
 */
export interface InstanceSettings {
  session_cap: SettingField;
  reaper_ttl_secs: SettingField;
  guard_timeout_secs: SettingField;
  /** #779: images + files budget per Run, in MB (`stored → env → default 50`). Bounds the
   *  multipart body of `POST /runs` and drives the « New run » modal's client-side gate. */
  max_attachments_mb: SettingField;
  /** The address humans open PDO at (`stored → env PDO_PUBLIC_URL → none`). Agents receive
   *  it and `pdo page mount` prints its links under it; `null` keeps them on localhost. */
  public_url: StringSettingField;
  /** CORE-97: where a repository given by its URL is cloned (`stored → ~/pdo/repos`),
   *  as typed: `~` stands for the daemon's home. Absent from an older daemon. */
  clones_folder?: StringSettingField;
  default_model: StringSettingField;
  /** #550/ADR-0046: instance-wide default harness (`stored → env → floor claude`).
   *  `effective: null` ⇒ the `claude` floor applies at resolve. */
  default_harness: StringSettingField;
  /** #550/ADR-0046: instance-wide default model **per harness**. `effective` is
   *  the resolved map (the stored map plus the legacy `PDO_DEFAULT_MODEL` folded
   *  under `claude`); `stored` is the raw stored map. */
  default_harness_model: {
    effective: Record<string, string>;
    stored: Record<string, string>;
  };
  agent_choice?: AgentChoice | null;
  /** #669/ADR-0062: the Instance tier of the skills selection (ids + labels). */
  skills?: SkillRef[];
  /**
   * Instance-wide default sandbox (#410/#432): `"off"` (host, default) or the name of a
   * **staging profile**. No longer a closed enum — its value space is the user's profile
   * namespace, which is why it carries a `reason` when the winning tier names a profile
   * that does not exist. The create-run chokepoint resolves precedence
   * run → trigger → this, and 400s on a dangling name (never a silent fallback).
   */
  default_sandbox: EnumSettingFieldWithReason;
  /**
   * Advisory Docker availability probe (#410), folded into `GET /settings` so the
   * NewRunModal learns the default AND whether Docker can run a sandbox in one fetch.
   * `available: false` grays out `full`/`minimal` (`reason` explains why); the
   * run-advance fail-fast stays the authoritative gate.
   */
  sandbox_docker: {
    available: boolean;
    reason: string | null;
    checked_at: string;
  };
  /**
   * Every staging profile the instance can serve (#432) — the two virtual defaults ∪ the
   * materialised rows, sorted. **Names only**, by contract: this payload is on the launch
   * dialog's hot path (it fetches settings on every open), so entry lists live behind
   * `GET /settings/sandbox-profiles` instead. Drives the sandbox `<select>`.
   */
  sandbox_profiles: SandboxProfileRef[];
  /**
   * The host `$HOME` the daemon stages from (#432), honouring `sandbox_home_override`. An
   * observed FACT, not a settings tier. Needed because the filesystem explorer's `onPick`
   * yields an ABSOLUTE path while a profile entry is RELATIVE to `$HOME` — and nothing
   * exposed `$HOME` before this. `null` when `HOME` is unset.
   */
  home: string | null;
  /**
   * Turn-end auto-completion (#469): may the runtime complete a node whose agent has
   * visibly finished its turn — end of turn constated in the transcript, no tool call
   * pending — and whose outputs validate? Off by default (ADR-0012: a terminal action
   * the runtime initiates is earned). Never keyed on a duration: that is exactly what
   * #469 removed.
   */
  autocomplete_turn_end: BoolSettingField;
  /**
   * Instance default for auto-naming a Run (#338). When a Run is created with no name and
   * no explicit choice, this decides whether the Pipeline Manager names it (from the input,
   * or a placeholder renamed best-effort) or leaves a stable `Untitled run …` placeholder
   * untouched. On by default (`true`) — the pre-#338 behaviour. The New Run modal seeds its
   * "Auto-generated by manager" box from `effective`, and a new Trigger freezes it.
   */
  default_auto_name: BoolSettingField;
  /**
   * May the daemon ask the release source which version is the latest (#697)? On by
   * default; off, no request ever leaves the daemon and `GET /update` answers `null`.
   * Saved at the change from the Version & update section (own persistence).
   */
  update_check: BoolSettingField;
  /**
   * #751 (ADR-0067 §4): may an agent's `pdo review reply --resolved` resolve the
   * review comment directly? **Off by default**: the reply is a *proposal* and the
   * human clicks Resolve / Reopen. On, the comment resolves on the spot and the
   * human keeps Reopen. Instance-wide, `stored → env → default` like auto-name.
   */
  review_agent_can_resolve: BoolSettingField;
  /**
   * Manager on demand: does every new Run automatically spawn its Pipeline
   * Manager session? **Off by default** — a Run starts managerless and the user
   * starts one from the Manager tab when they want it. Gates ONLY the automatic
   * spawn: a manual start from the Manager tab is always available. Stored
   * `0`/`1` discipline like the other plain-bool knobs.
   */
  manager_enabled: BoolSettingField;
  /**
   * Manager on demand: the agent-profile NAME the manager is pinned to, or
   * `null` (« Follow the Run » — the manager mirrors the Run's harness · model
   * · effort). A named profile overrides the Run tier. A stored name whose
   * profile was later deleted dangles: the Settings surface renders the #432
   * tombstone, and the spawn falls back to « Follow the Run ».
   */
  manager_profile: StringSettingField;
  /**
   * Which price tiers are in force (#427, ADR-0034) — an observed STATE, not a
   * settings knob, hence no `{effective, source, stored, env, default}` shape.
   *
   * Both paths are always reported, **even when neither file exists**: nothing is
   * ever seeded, so naming them is the whole discoverability story. `reason` is the
   * same string the daemon logs, and is non-null exactly when a file or a row went
   * inert — a hand-edited file passes through no validator, so this is the only
   * honest place to surface it (the #432 argument).
   */
  price_table: PriceTableView;
  /**
   * The harness descriptor disk tier (#553, ADR-0045) — an observed STATE, like
   * `price_table`. `names` lists the harnesses that actually resolve (embedded
   * floor merged with the user's disk file), so a declared harness "appears";
   * `path` is always reported (nothing is seeded) so the user knows where to
   * write; `reason` is the same string the daemon logs, non-null exactly when a
   * descriptor went inert or was refused — the only honest place to say so, since
   * a hand-edited descriptor passes through no validator (ADR-0001).
   *
   * Optional in the type (the Settings surface guards on it) so a UI built against a
   * daemon that predates #553 still typechecks — same defensive posture the modal
   * takes for `price_table`. In production the SPA is embedded, so they agree.
   */
  harness_descriptors?: HarnessDescriptorsView;
  /**
   * ADR-0090: the OTLP export of the instance's metrics and traces. Off by default; on,
   * every measure goes to `telemetry_otlp_endpoint` (the paths `/v1/metrics` and
   * `/v1/traces` are added). Optional like `harness_descriptors`, for a daemon that
   * predates it.
   */
  telemetry_otlp?: BoolSettingField;
  telemetry_otlp_endpoint?: StringSettingField;
  /** The export's headers, by name: their values are never sent back. */
  telemetry_otlp_headers?: OtlpHeaderView[];
  /** ADR-0090 §2: `/metrics` in the Prometheus format, by its own switch. Off by default. */
  telemetry_prometheus?: BoolSettingField;
  /**
   * ADR-0090 §4: who launched a Run, and who answered it, goes with its measures on both
   * channels. Off by default.
   */
  telemetry_include_actor?: BoolSettingField;
  updated_at: string;
}

/** `GET /settings` → `price_table` (#427). */
export interface PriceTableView {
  /** `~/.pdo/prices/models.yaml` — the human's file. PDO never writes it.
   *  `null` only when `HOME` is unset. */
  manual_path: string | null;
  /** `~/.pdo/prices/fetched.json` — the daemon's file. Rewritten whole. */
  fetched_path: string | null;
  /** URL of the last successful fetch, or `null` if none ever ran. */
  source: string | null;
  /** The fetched table's vintage — readable, not guessed. */
  fetched_at: string | null;
  fetched_rows: number;
  /** Family keys the manual tier actually decides — i.e. what shadows a sync. */
  manual_keys: string[];
  /** Advisory: an inert file or refused row, named. `null` when all is well. */
  reason: string | null;
}

export interface AgentCombination {
  harness: string;
  model?: string | null;
  effort?: string | null;
}

export interface AgentProfileReferents {
  profile_id: string;
  instance: boolean;
  pipelines: { id: string; name: string; node_id?: string }[];
  runs: { run_id: string; name?: string | null }[];
  projects: { id: string; name: string }[];
  triggers: { id: string; name: string; pipeline_id: string }[];
}
