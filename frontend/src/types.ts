export type RunStatus = "running" | "awaiting_user" | "completed" | "failed" | "skipped" | "halted" | "paused" | "archived";
export type NodeStatus = "pending" | "running" | "awaiting_user" | "completed" | "skipped" | "failed" | "stopped" | "stale" | "interrupted";

export function isLiveRun(status: RunStatus): boolean {
  return status === "running" || status === "awaiting_user" || status === "paused";
}

/**
 * A run with an actively-running (or user-awaiting) node — the states in which
 * the App auto-snaps the selection to that node so its terminal is shown at
 * once. NARROWER than `isLiveRun`: a `paused` run does not auto-snap, so its
 * Run-info panel is already reachable by deselecting. This is exactly the set
 * where the panel is otherwise unreachable, so the canvas exposes an explicit
 * toggle for it (#465 slice 2, F1). Keep the auto-snap guard and that toggle's
 * visibility on this single predicate so they can never drift apart.
 */
export function isNodeActiveRun(status: RunStatus): boolean {
  return status === "running" || status === "awaiting_user";
}

/**
 * Mirror of Rust `RunStatus::is_terminal()` (the total complement of `is_live`):
 * `{completed, failed, skipped, halted, archived}`. NOTE this INCLUDES
 * `archived` — callers that gate on "terminal AND not archived" (e.g. the
 * "Open session" shell action, #316) must exclude `archived` explicitly.
 */
export function isTerminalRun(status: RunStatus): boolean {
  return !isLiveRun(status);
}

/**
 * How the daemon process was launched + whether it is installed as a
 * persistent service (#156 / ADR-0019). Folded into `GET /sessions` (not a
 * new route) and computed once at daemon boot.
 */
export interface ServiceHealth {
  /** Best-effort env-marker hint: how THIS process was launched. */
  supervisor: "systemd" | "launchd" | "none";
  /**
   * Will a daemon come back after reboot? `true` when an enabled unit is
   * present, `false` when reachable-but-ephemeral (drives the status-bar
   * `ephemeral` pill), `null` when unknown/unsupported (non-Linux, no systemd,
   * detection failure). Never an error — the UI silences on `true`/`null`.
   */
  persistent: boolean | null;
}

/**
 * Live NodeRun-session count, the configured global cap, the daemon version,
 * and the persistent-service health, for the bottom status bar (#159 /
 * ADR-0012, #139, #156). Manager sessions are excluded. `version` and
 * `service` are absent until the daemon has responded.
 */
export interface DaemonStatus {
  live: number;
  cap: number;
  version?: string;
  service?: ServiceHealth;
}

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

/** One staging profile, as `GET /settings` lists it (#432): the NAME only. */
export interface SandboxProfileRef {
  name: string;
  /** `full` / `minimal` — resolves with no DB row until edited (ADR-0031 §2). */
  virtual: boolean;
}

/** What a profile entry points at (#432). `glob` is authored by the built-in default only. */
export type SandboxEntryKind = "dir" | "file" | "glob";

/** One resolved entry of a staging profile, as the editor shows it (#432). */
export interface SandboxProfileEntry {
  /** `$HOME`-relative path, or the default's one-level glob pattern. */
  path: string;
  kind: SandboxEntryKind;
  /** From the built-in default (checkable) vs a user extra (removable). */
  from_default: boolean;
  enabled: boolean;
  /**
   * Class (b): unchecking does NOT make the file absent — the staging floor
   * re-synthesises the keys it needs. Exactly two entries. The UI must say so, or
   * unchecking reads as more destructive than it is.
   */
  resynthesised: boolean;
  /** Server-owned advisory (disk cost, behaviour when unchecked). Never derived client-side. */
  note: string | null;
  /** Under `.ssh` / `.aws` / `.gnupg` — allowed with a warning (ADR-0031 §3). */
  sensitive: boolean;
  /** Present on the host right now; `null` when unknowable (a glob, or no `$HOME`). */
  exists: boolean | null;
}

/**
 * One class-(c) floor guarantee (#432): satisfied by the WHOLE file, so it is neither
 * checkable nor addable. Rendered read-only — without this block a `minimal` profile's
 * screen looks broken and the user wrongly concludes the container starts with no
 * credentials.
 */
export interface SandboxFloorGuarantee {
  id: string;
  label: string;
  path: string | null;
}

/** The full editor view of one staging profile (`GET|PUT /settings/sandbox-profiles/{name}`). */
export interface SandboxProfile {
  name: string;
  virtual: boolean;
  /** A DB row exists (the profile has been edited at least once). */
  materialised: boolean;
  /** The stored DIFF — the user's intention, never the effective list. */
  disabled: string[];
  extras: string[];
  /** What a Run created NOW would freeze into `RunStarted`. */
  resolved: string[];
  entries: SandboxProfileEntry[];
  /** Signalled no-ops, never errors (ADR-0031 §2). */
  redundant_extras: string[];
  inactive_disabled: string[];
  floor: SandboxFloorGuarantee[];
  sensitive_prefixes: string[];
  /**
   * Environment variables posed at `docker create` for every Run on this profile (#468,
   * ADR-0031 §8). **Not a diff** — unlike `disabled`/`extras` there is no built-in default
   * to fold against, so this map IS the effective env.
   *
   * Values are served in clear, on purpose: they already sit in clear in SQLite, in the
   * Run's frozen `run_started` payload and in `docker inspect`. The sandbox is not a
   * security boundary and the editor says so — masking them here would suggest PDO is
   * protecting something it is not.
   */
  env: Record<string, string>;
  /**
   * The keys PDO poses itself and therefore refuses with a 400 naming the key (`HOME`,
   * `PDO_DAEMON_URL`, `PDO_RUN_ID`). Server-owned so the editor never hard-codes a parallel
   * list that would drift the day a fourth run-constant appears (#373).
   */
  reserved_env_keys: string[];
  /**
   * Where this profile's container image comes from (#467, ADR-0031 §9), or `null` when it poses
   * nothing and PDO's built-in default decides (#471: registry-pulled, hash-derived from the
   * seeded Dockerfile). Like `env` it is a FULL replacement on write — `null` is how you go back
   * to the default image.
   */
  image: SandboxProfileImage | null;
  updated_at: string | null;
}

/**
 * A profile's image source (#467). The two shapes are **interchangeable in the form and
 * radically different downstream**, which is the one thing the editor has to convey:
 *
 * - `dockerfile` is the hash-derived path — the tag is the SHA-256 of the file's bytes, so a pull
 *   from the registry and a local build are interchangeable and a failed pull falls back to a
 *   build;
 * - `registry` is an explicit ref, pulled as-is. It has no Dockerfile, therefore no content hash,
 *   therefore **no build to fall back to**: a failed pull fails the Run (ADR-0030 pt 7 as amended
 *   by #467), and PDO cannot verify the image even contains `claude`.
 */
export type SandboxProfileImage =
  | { kind: "dockerfile"; path: string }
  | { kind: "registry"; ref: string };

/**
 * Who still points at a profile (`GET …/{name}/referents`, #432). Server-side because the
 * frontend cannot derive the third class: {@link RunListEntry} carries no `sandbox`.
 *
 * The distinction is the whole point of the delete dialog: `instance_default` and
 * `triggers` are NOT repointed and their next Run **fails**, while `runs` already froze
 * their entry list at start and are **unaffected**.
 */
export interface SandboxProfileReferents {
  name: string;
  instance_default: boolean;
  triggers: { id: string; name: string; enabled: boolean }[];
  runs: { run_id: string; pipeline_name: string | null; name: string | null }[];
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
   * Optional in the type (the SettingsModal guards on it) so a UI built against a
   * daemon that predates #553 still typechecks — same defensive posture the modal
   * takes for `price_table`. In production the SPA is embedded, so they agree.
   */
  harness_descriptors?: HarnessDescriptorsView;
  updated_at: string;
}

/** `GET /settings` → `harness_descriptors` (#553). */
export interface HarnessDescriptorsView {
  /** `~/.pdo/harnesses/descriptors.yaml`. `null` only when `HOME` is unset. */
  path: string | null;
  /** The harnesses the registry resolves (floor ∪ disk), in resolution order. */
  names: string[];
  /**
   * Each resolved harness enriched for the picker (#586): its provenance and
   * whether its binary resolves on the daemon's `$PATH`. Drives the two-section,
   * grey-if-uninstalled picker. Optional so a daemon predating #586 still
   * typechecks (the picker then falls back to the embedded floor); in production
   * the SPA is embedded, so it is always present.
   */
  harnesses?: HarnessListItem[];
  /** Descriptors the disk tier refused — each inert, its key on the floor. */
  rejected: { name: string; why: string }[];
  /** Advisory: an inert file or refused descriptor, named. `null` when all is well. */
  reason: string | null;
}

/** One resolved harness, as `GET /settings → harness_descriptors.harnesses`
 *  discloses it (#586, #616). */
export interface HarnessListItem {
  name: string;
  /** `builtin` = the embedded floor (claude/opencode); `descriptor` = the disk tier. */
  source: "builtin" | "descriptor";
  /** Whether the harness's binary resolves on the daemon's `$PATH`. `false` greys
   *  the row and blocks selection — a spawn would fail fast (ADR-0037). */
  installed: boolean;
  /** #616/ADR-0053: the model ids the installed binary offers, deduced from it and
   *  served — the picker renders THESE instead of a hard-coded alias list. Empty ⇒
   *  the binary enumerates none, so the client falls back to free text (a declared
   *  absence). Optional so a daemon predating #616 still typechecks. */
  models?: string[];
  /** #616/ADR-0053: the effort levels the binary offers. Empty ⇒ no effort axis. */
  efforts?: string[];
  /** #616/ADR-0053: the served effort-axis fact — whether this harness has an
   *  effort axis at all. Drives the effort-picker greying, replacing the client's
   *  hard-coded map. */
  has_effort?: boolean;
  /** #616/ADR-0053: the probed binary version the catalogue was read at, for the
   *  picker's provenance line. `null` when the binary answered no `--version`. */
  version?: string | null;
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

/**
 * Response of `POST /settings/cost-prices/sync` (#427, ADR-0034).
 *
 * A noop is an honest 200 carrying `noop: true` + `reason` (ADR-0025 forbids a
 * blind `{ok:true}`); a network cut is a thrown 502 naming the source.
 */
export interface SyncCostPricesReport {
  ok: boolean;
  noop?: boolean;
  reason?: string | null;
  source: string;
  fetched_at: string | null;
  /** Rows retained in the fetched tier. */
  rows: number;
  /** Keys no tier priced before — the repair. */
  added: string[];
  /** Keys whose effective price changes. */
  updated: string[];
  unchanged: number;
  /** Source rows refused, with the motive. */
  rejected: string[];
  /** Fetched, but the manual tier still wins — said, never hidden. */
  shadowed_by_manual: string[];
}

/**
 * A partial `PUT /settings` edit; omitted fields are left unchanged.
 *
 * `default_model` uses `""` as the clear sentinel (the backend normalises it to
 * NULL): sending `null` would deserialise to `None` server-side and be a silent
 * no-op, so the modal sends `""` to reset to the account default.
 */
export interface UpdateSettingsRequest {
  session_cap?: number;
  reaper_ttl_secs?: number;
  guard_timeout_secs?: number;
  default_model?: string;
  /** #550: instance default harness; `""` clears it (same sentinel as
   *  `default_model`). */
  default_harness?: string;
  /** #550: per-harness default model map; replaces the stored map wholesale. */
  default_harness_model?: Record<string, string>;
  /** Atomic instance agent selection. `null` clears to the Default floor. */
  agent_choice?: AgentChoice | null;
  /** Default sandbox (#410/#432): `"off"` or a staging-profile name, or `""` to clear
   *  back to the built-in default (`off`). Same `""`-sentinel discipline as
   *  `default_model`. The daemon 400s a name that does not resolve.
   *
   *  #471 removed `image_source` and `dockerfile_path` from this shape, and the daemon now
   *  **400s naming the field** if either is sent — a stale client is told, not ignored. */
  default_sandbox?: string;
  /** Turn-end auto-completion (#469). A plain bool: `false` PERSISTS as a stored `0`
   *  (there is no `""` clear sentinel), so unticking the box overrides a
   *  `PDO_AUTOCOMPLETE_TURN_END=1` instead of falling back to it. */
  autocomplete_turn_end?: boolean;
  /** Default Run auto-naming (#338). Same plain-bool discipline as
   *  `autocomplete_turn_end`: `false` persists as a stored `0`, so unticking overrides a
   *  `PDO_DEFAULT_AUTO_NAME=1` rather than falling back to it. */
  default_auto_name?: boolean;
}
// `for-each` was removed (ADR-0011 / #151): a fan-out is now a `collection`
// loop region, not a node. The backend keeps the variant only to migrate old
// YAML into a region. `loop` was likewise removed in #171.
// `script` (#248 / ADR-0017) runs author-written bash deterministically instead
// of launching Claude; the FE union is not 1:1 with the backend enum.
// `agent` (#653 / ADR-0060) replaces `doc-only` and `code-mutating`: the type
// names the execution role, and where the NodeRun works is `isolated_worktree`.
export type NodeType = "agent" | "start" | "end" | "merge" | "script";

export interface RunListEntry {
  run_id: string;
  pipeline_name: string;
  status: RunStatus;
  /**
   * Display-only "no forward progress" overlay (#180): true when the run has no
   * running/waiting node and a stale node. The dot renders amber and steady
   * even though `status` stays `"running"`. Derived server-side per read.
   */
  stalled?: boolean;
  started_at: string | null;
  /**
   * Why the Run ended non-green (#503). Absent on a green or live Run. Present on
   * the list entry so the red dot has something to say — before #503 the whole
   * failure signal in this list was a coloured dot with no text behind it.
   */
  failure_reason?: string | null;
  /**
   * Why the Run is parked `awaiting_user` on an **incident** (ADR-0049), prose +
   * a machine slug (#601). Present on the list entry so the manager cockpit shows
   * *why* a run is parked without opening the detail. Absent on a green/live Run
   * or an interactive wait (which carries no incident reason).
   */
  awaiting_reason?: string | null;
  awaiting_reason_code?: string | null;
  name?: string | null;
  /** Provenance: the id of the Trigger that created this Run, if any (#160). */
  triggered_by?: string | null;
  /**
   * Resolved target repo for "group by project" (#258): the run's `target_repo`,
   * or the daemon's `repo_root` when unset. Always sent by the daemon; declared
   * optional so existing test fixtures that omit it still typecheck.
   */
  effective_repo?: string;
}

/**
 * A **Projet** (#552, ADR-0046): a named grouping of member repo paths, and the
 * middle tier of the harness precedence axis. Materialised on demand — a Projet
 * exists only once a human names a group header or attaches a setting; until then
 * the lists group by the derived path label (#258). Membership is compared
 * verbatim (ADR-0033).
 */
export interface Project {
  id: string;
  name: string;
  /** The harness this Projet carries, or absent/null when it carries none. */
  harness?: string | null;
  agent_choice?: AgentChoice | null;
  /** Member repository paths (the effective-repo keys the lists group by). */
  members: string[];
}

export type ProvisioningMode = "copy" | "hardlink" | "symlink";
export type ProvisioningScope = "instance" | "project" | "run" | "isolated_node";

export interface ProvisioningRules {
  copy: string[];
  hardlink: string[];
  symlink: string[];
}

export interface ScopedProvisioningRules {
  scope: ProvisioningScope;
  rules: ProvisioningRules;
}

export interface ProvisioningEntry {
  relative_path: string;
  mode: ProvisioningMode;
  origin_scope: ProvisioningScope;
  pattern: string;
  provided_by_git: boolean;
}

export interface ProvisioningRulePreview {
  scope: ProvisioningScope;
  mode: ProvisioningMode;
  pattern: string;
  paths: string[];
  excluded_paths: Array<{
    relative_path: string;
    excluded_by_scope: ProvisioningScope;
  }>;
  unmatched: boolean;
}

export interface ProvisioningPlan {
  entries: ProvisioningEntry[];
  rules: ProvisioningRulePreview[];
  conflicts: Array<{
    scope: ProvisioningScope;
    relative_path: string;
    modes: ProvisioningMode[];
  }>;
}

/**
 * A persisted Trigger (#160 / ADR-0012): a cron schedule bound to a run
 * template. Cron-only in this slice — `guard_command` is reserved for #161.
 */
export interface Trigger {
  id: string;
  name: string;
  pipeline_id: string;
  pipeline_name: string;
  target_repo?: string | null;
  /**
   * Read-only secondary repos as raw JSON TEXT (#465, ADR-0042): a
   * `[{repo, base_branch?}]` array with `[0]` = primary. Null/absent → mono-repo.
   * Forwarded and re-frozen at fire time.
   */
  target_repos?: string | null;
  /**
   * Resolved target repo for "group by project" (#258): the raw `target_repo`,
   * or the daemon's `repo_root` when unset. Sent only by the list endpoint
   * (`GET /triggers`); the row badge / detail still read raw `target_repo`.
   */
  effective_repo?: string | null;
  source_branch?: string | null;
  input_template: string;
  variables: Record<string, unknown>;
  cron: string;
  guard_command?: string | null;
  overlap_policy: string;
  /** Bounded-`allow` ceiling (#239): max simultaneous live Runs; null = unbounded. */
  max_concurrent?: number | null;
  /** Per-Trigger sandbox (#410/#432): `"off"` or a staging-profile name, or null/absent
   *  to inherit the instance default. Read at fire time. */
  sandbox?: string | null;
  /** Per-Trigger harness (#551, ADR-0046): a harness name, or null/absent to inherit the
   *  instance default. Read at fire time and folded into the fired Run's harness (no
   *  separate Trigger tier — a cron tick and a "Run now" produce the same one). */
  harness?: string | null;
  agent_choice?: AgentChoice | null;
  /** Whether Runs this Trigger fires are auto-named (#338). Frozen at creation from the
   *  instance default; `true` is the pre-#338 behaviour. A flat bool (no inherit state). */
  auto_name: boolean;
  enabled: boolean;
  next_fire_at?: string | null;
  last_fired_at?: string | null;
  last_outcome?: string | null;
}

/** One audit row in a Trigger's fire history (`trigger_fires`). */
export interface TriggerFire {
  id: number;
  trigger_id: string;
  ts: string;
  outcome: string;
  reason?: string | null;
  run_id?: string | null;
  /**
   * Guard diagnostics on a `guard-exit-nonzero` row (#244): what the guard
   * printed and its exit status. Absent/null on every other outcome and on
   * legacy rows; each stream is tail-capped to 16 KB by the daemon.
   */
  guard_stdout?: string | null;
  guard_stderr?: string | null;
  guard_exit_code?: number | null;
  /**
   * Fire origin (#341): "manual" for a Run-now click, "cron" for a scheduler
   * tick. Absent/null on legacy rows ≈ cron.
   */
  source?: "manual" | "cron" | null;
}

export interface IterationInfo {
  iter: number;
  status: NodeStatus;
  started_at: string | null;
  completed_at: string | null;
}

/**
 * One frontmatter field the validator rejected. Named since #490 because the shape
 * now travels on two paths: the projection (`NodeState`) *and* the refusal body of
 * `mark_node_done`.
 */
export interface FrontmatterViolation {
  port: string;
  field: string;
  reason: string;
}

export interface NodeCost {
  usd: number | null;
  form: "derived" | "reported" | null;
  partial: boolean;
  unpriced_models?: string[];
  unavailable_reasons?: string[];
  executions: number;
  readable_executions: number;
}

export interface NodeState {
  node_id: string;
  status: NodeStatus;
  iter: number;
  started_at: string | null;
  completed_at: string | null;
  failure_reason: string | null;
  /**
   * Why the node was **auto-skipped** as structurally unreachable (#620): its
   * producing branch was not taken, so nothing would ever spawn it. Present only
   * when `status === "skipped"`; distinct from `failure_reason` (a skip is not a
   * failure). Absent on every other status. The projection lifts it out of the
   * skip event's payload so the reason reads at node level, not only in the log.
   */
  skip_reason?: string | null;
  iterations: IterationInfo[];
  frontmatter_retries?: number;
  frontmatter_violations?: FrontmatterViolation[];
  /**
   * #490: declared output ports the validator found empty. Optional because the
   * daemon omits it when empty (`skip_serializing_if`) — and because a synthetic
   * `NodeState` literal in `App.tsx` would otherwise stop compiling.
   */
  missing_outputs?: string[];
  /**
   * #616/ADR-0046: the harness this node's session was FROZEN on at spawn, from the
   * `NodeStarted` payload. Shown per-node in the Run view (next to the id, on
   * select) so what actually ran is visible — distinct from the run-level default
   * (`RunState.harness`). Optional: absent for a node that never started (a pure
   * skip) or a pre-#616 daemon.
   */
  harness?: string;
  /**
   * #653/ADR-0060: where this NodeRun was FROZEN to work at spawn — `true` its
   * own sub-worktree, `false` the Run's. This, not the document, is what the run
   * inspector shows: editing the graph never moves a live iteration. Absent for a
   * node that never started, a structural node, or a pre-#653 daemon.
   */
  isolated_worktree?: boolean;
  /** Node provisioning recipe frozen into this iteration's NodeStarted event. */
  provisioning?: ProvisioningRules;
  /** Time this iteration's isolated worktree recipe was first materialized. */
  provisioning_frozen_at?: string;
  /** Exact resolved plan applied when the isolated worktree was first materialized. */
  provisioning_plan?: ProvisioningPlan;
  /**
   * #654/ADR-0060: what this NodeRun DELIVERED onto the run's branch — the two
   * tips its delivery moved the branch between. Present for any NodeRun that
   * delivered changes, isolated or not; absent for one that delivered nothing
   * (no commit was written) and on a pre-#654 daemon. Its presence, never the
   * node's type or isolation, is what says a per-node diff exists.
   */
  delivery?: NodeDelivery | null;
  cost?: NodeCost | null;
}

/** The two run-branch tips one delivery moved between (#654 / ADR-0060). */
export interface NodeDelivery {
  before: string;
  after: string;
}

export interface EdgeInfo {
  source_node: string;
  source_port: string;
  target_node: string;
  target_port: string;
  halt_message?: string | null;
  when_clause?: Record<string, unknown> | null;
}

/**
 * Runtime trigger status for a single conditional edge (ADR-0011, #147).
 * Shown ONLY in the edge detail panel — never rendered on the canvas. Derived
 * from the run state; absent until the edge's source node has been evaluated.
 */
export interface EdgeTriggerStatus {
  fired: boolean;
  /** The clause's evaluated value rendered for display, e.g. `verdict = FAIL`. */
  last_value: string | null;
  evaluated_at: string | null;
  iter: number | null;
}

export interface PortBrief {
  name: string;
  side: PortSide;
  description?: string | null;
}

export interface NodeDefInfo {
  id: string;
  name?: string | null;
  node_type: NodeType;
  /** #653: where the node works, as the Run's pipeline snapshot froze it. */
  isolated_worktree?: boolean | null;
  view_x: number | null;
  view_y: number | null;
  inputs: PortBrief[];
  outputs: PortBrief[];
}

export interface StartNodeInfo {
  input_path: string;
  started_at: string;
  target_node_ids: string[];
  // Filenames of images uploaded alongside the text prompt (stored in
  // `_input/`). Empty when the run was launched without images (issue #145).
  input_images: string[];
}

export interface EndPortStatus {
  port_name: string;
  status: string;
  reason: string | null;
  fired_at: string | null;
}

export interface EndNodeInfo {
  id: string;
  ports: EndPortStatus[];
}

export interface MergeResolverInfo {
  status: NodeStatus;
  conflicting_node_id: string;
  iter: number;
  session_name: string | null;
  started_at: string | null;
  completed_at: string | null;
  failure_reason: string | null;
}

export interface LoopStateInfo {
  loop_node_id: string;
  current_iter: number;
  max_iter: number;
  break_received: boolean;
  done: boolean;
}

export interface ForEachStateInfo {
  foreach_node_id: string;
  total_items: number;
  break_received: boolean;
  done: boolean;
}

/**
 * Barrier accounting for a `kind: collection` region (ADR-0011 / #269), keyed by
 * region id. `total_items` is the resolved size of the `over` list — the ONLY
 * truthful denominator for the canvas badge (#453): member `iter` counts the
 * last lap *reached*, which reads `1 items` on a region wedged at lap 1 of 2 and
 * on a healthy 1-item region alike.
 */
export interface CollectionStateInfo {
  region_id: string;
  total_items: number;
  done: boolean;
  entry?: string;
  members?: string[];
}

/**
 * One secondary repo pinned to a Run (#465, ADR-0042/0047). Mirror of the server
 * `RepoPin`: `repo` is the absolute host path, `alias` the disambiguated snapshot
 * folder name (the handle a `remove` names), `sha` the frozen commit, `base_branch`
 * the ref it was resolved from (provenance only; the SHA is authoritative), and
 * `read_only` the per-repo opt-in (ADR-0047). Absent `base_branch` means the pin
 * defaulted to `HEAD`; absent `read_only` means writable (the default).
 */
export interface RepoPin {
  repo: string;
  alias: string;
  sha: string;
  base_branch?: string | null;
  read_only?: boolean;
}

export interface RunState {
  run_id: string;
  status: RunStatus;
  pipeline_name: string;
  name?: string | null;
  input: string | null;
  started_at: string | null;
  completed_at: string | null;
  /**
   * Why the Run ended non-green — the `reason` of its `run_failed` / `run_skipped`
   * / `run_halted` (#503). Absent on a green or live Run, and cleared by a resume.
   *
   * Every one of those events had always carried a reason and nothing read it, so
   * the whole failure signal a user got was a red dot in the Runs list.
   */
  failure_reason?: string | null;
  /**
   * Why the Run is parked `awaiting_user` on an INCIDENT (ADR-0049) — a session
   * death, boot recovery, spawn abort, run-level stall, output-validation miss,
   * merge conflict or `unrouted` convergence. Distinct from the interactive
   * `awaiting_user` wait of a node asking its user a question, which carries no
   * `awaiting_reason`. Cleared by a resume/reopen. Present only while the Run is
   * `awaiting_user` on an incident.
   */
  awaiting_reason?: string | null;
  /**
   * The machine slug companion of {@link awaiting_reason} (#601): a stable
   * snake_case code (`session_died`, `run_stalled`, `unrouted`,
   * `region_exhausted`, `spawn_aborted`, `merge_conflict`, …) to branch on —
   * next to the human prose, the same slug+prose contract as a refusal body
   * (ADR-0035). Absent for an interactive wait.
   */
  awaiting_reason_code?: string | null;
  nodes: Record<string, NodeState>;
  edges: EdgeInfo[];
  node_defs: NodeDefInfo[];
  /** Instance + Project + Run provisioning rules frozen at Run creation. */
  provisioning_rules?: ScopedProvisioningRules[];
  start_node: StartNodeInfo | null;
  end_node: EndNodeInfo | null;
  merge_resolver: MergeResolverInfo | null;
  loop_states?: Record<string, LoopStateInfo>;
  foreach_states?: Record<string, ForEachStateInfo>;
  collection_states?: Record<string, CollectionStateInfo>;
  target_repo?: string | null;
  /**
   * The Run's read-only secondary repos (#465, ADR-0042). Absent/empty on a
   * mono-repo Run. **Editable mid-run** (slice 2) via `editRunRepos`: adding/removing
   * a secondary rewrites this list, and the change is visible to nodes launched
   * AFTER the edit (spawn-time visibility) — already-live nodes keep their frozen
   * context. Never contains the primary (that stays in `target_repo`).
   */
  target_repos?: RepoPin[];
  source_branch?: string | null;
  /**
   * Isolation for this Run (#403 / #407 / #410 / #432): `"off"`, or the name of the
   * **staging profile** it launched with. Absent on host/historical runs (projected as
   * `off` server-side and skipped from the payload when off). Immutable once the Run
   * started. Widened from the closed `off|full|minimal` union in #432 — a profile name is
   * an open value space.
   */
  sandbox?: string;
  /**
   * The profile's resolved entry list, **frozen at creation** (#432, ADR-0031 §6). Absent
   * on `off` and on pre-#432 runs. `[]` is a legitimate value — that IS `minimal`.
   *
   * This is what `prepare` consumes, which is why editing (or deleting) a profile cannot
   * retroactively change what a Run in flight staged.
   */
  sandbox_entries?: string[];
  /**
   * One-time image-prep visibility for a sandboxed Run (#410). `"pending"` while
   * the image is pulled/built at first use; `"ready"` once the container is about
   * to run; absent for host/off runs. Additive — `status` stays `"running"`
   * throughout, so this drives a banner only.
   */
  sandbox_prep?: "pending" | "ready";
  /**
   * The agentic harness this Run was created on (#551, ADR-0046) — the `run` tier of
   * the precedence chain, **frozen** at creation like {@link RunState.sandbox}. Absent
   * when the Run named no harness (every historical Run, and any Run that inherited the
   * instance default): each free node then resolved through the instance default and
   * the `claude` floor. Shown in the Run panel; a pinned node ignored it.
   */
  harness?: string;
  /**
   * Cumulative count of NodeRun sessions this run spawned — raw `NodeStarted`
   * count, not distinct `(node, iter)`; manager excluded (#100). Defaults to 0
   * on older payloads.
   */
  sessions_spawned?: number;
  /**
   * Lines changed for the run (`git diff --numstat` of the run branch, `.pdo/`
   * excluded), or null/absent once the branch is gone (archived/cleaned) — the
   * UI renders "—" in that case, never "0" (#100).
   */
  loc?: { insertions: number; deletions: number; files_changed: number } | null;
  /**
   * Estimated USD cost (#272) — local Claude Code token usage × public list
   * prices, an estimate, not an invoice. Null/absent when no transcripts are
   * found (UI "—"). `partial` = an unpriced model was seen → the number is a
   * lower bound; `unpriced_models` names which family keys were excluded (#425),
   * so the UI can say *which* model rather than an anonymous "an unpriced model".
   * Invariant: `partial ⟺ unpriced_models.length > 0`.
   *
   * `uncosted_harnesses` (#553): the harnesses a node ran on that have **no cost
   * source** (e.g. `opencode`). Non-empty ⇒ the Run's cost is not honestly
   * summable, so the UI shows "—" with a reason naming them, never a `$0` and
   * never a mute lower-bound — a categorically different state from `partial`
   * (which still shows a figure). Empty on every all-`claude` Run.
   *
   * `by_harness` (#615, ADR-0052): the total ventilated by harness — "X via
   * `copilot`, Y via `claude`". `usd` is their sum; each slice carries its `form`
   * (a `derived` claude estimate vs a `reported` copilot figure), so the UI frames
   * *only* a derived slice as a Claude-Code estimate and never labels a reported
   * one as one. Absent/empty on a pre-#615 Run or one with no costable session.
   */
  cost?: {
    usd: number;
    partial: boolean;
    unpriced_models: string[];
    uncosted_harnesses?: string[];
    by_harness?: HarnessCost[];
  } | null;
}

/** One harness's slice of a Run's cost (#615, ADR-0052 §3). Additive in dollars,
 *  tagged with its `form` so a reported figure is never mislabelled an estimate. */
export interface HarnessCost {
  harness: string;
  usd: number;
  form: "derived" | "reported";
  partial: boolean;
  unpriced_models: string[];
}

export interface DaemonEvent {
  id: number | null;
  run_id: string;
  ts: string;
  kind: string;
  node_id: string | null;
  iter: number | null;
  payload: Record<string, unknown> | null;
}

export interface WsMessage {
  type:
    | "ready"
    | "heartbeat"
    | "event"
    | "pipeline_changed"
    | "trigger_created"
    | "trigger_fired"
    | "trigger_updated"
    | "trigger_deleted"
    | "triggers_paused"
    | "project_changed";
  event?: DaemonEvent;
  pipeline_id?: string;
  path?: string;
  ts?: string;
  /** Set on trigger_* messages (#160). */
  trigger_id?: string;
  outcome?: string;
  run_id?: string | null;
  /** Set on `triggers_paused` messages (#348): the new global pause state. */
  paused?: boolean;
  /** Set on `project_changed` messages (#552): the mutated Projet's id. */
  project_id?: string;
}

export type EditScope = null | "run";

export interface PipelineVariableInfo {
  var_type: string;
  default: unknown;
}

/**
 * A branch the daemon offers as a Run source (#571). `name` is posted back as
 * `source_branch` **verbatim** — a remote-tracking ref keeps its `origin/`
 * prefix, so what the field shows is exactly what launches (#452/#454). `kind`
 * is authoritative and comes from the full refname on the daemon: never
 * re-derive locality by string surgery client-side, a *local* branch may
 * legitimately be named `origin/x`.
 */
export interface BranchRef {
  name: string;
  kind: "local" | "remote";
}

export type PipelineScope = "instance" | "repo" | "user" | "library";

export interface PipelineListEntry {
  id: string;
  name: string;
  scope: PipelineScope;
  path: string;
  node_count: number;
  modified: string | null;
  variables: Record<string, PipelineVariableInfo>;
  /**
   * Whether a manual Run must supply a non-empty prompt (#158). Defaults to
   * `true` when absent; the New Run modal makes the prompt field optional when
   * this is `false`.
   */
  prompt_required?: boolean;
  drifted?: boolean | null;
}

export type PortSide = "left" | "right" | "top" | "bottom";
export type PortType = "markdown" | "image" | "image_list" | "html";

export interface PortDef {
  name: string;
  repeated: boolean;
  side?: PortSide;
  port_type?: PortType;
  frontmatter?: Record<string, FrontmatterFieldDecl> | null;
  when?: Record<string, unknown> | null;
  description?: string | null;
  instructions?: string | null;
}

export interface FrontmatterFieldDecl {
  type: string;
  allowed?: string[] | null;
}

export interface VariableDef {
  type: string;
  default: unknown;
}

export interface NodeDef {
  id: string;
  name?: string | null;
  type: NodeType;
  inputs: PortDef[];
  outputs: PortDef[];
  interactive: boolean;
  view?: { x: number; y: number } | null;
  max_iter?: number | string | null;
  over?: string | null;
  /** Optional per-node model override (#296): free-text pass-through to
   *  `claude --model <x>`. Absent/null ⇒ account default (no flag). */
  model?: string | null;
  /** Optional per-node reasoning-effort override (#424): free-text pass-through
   *  to `claude --effort <level>`. Absent/null ⇒ no flag (account default).
   *  Orthogonal to `model`, and semantic — it enters the pipeline diff and the
   *  node-library content hash.
   *
   *  Since #550 this is the **resolved harness's** effort: folded out of
   *  `harnesses[resolved].effort` on load, folded back on save. */
  effort?: string | null;
  /** #550/ADR-0046: the pinned harness (`claude`, `opencode`), or null/absent to
   *  follow the tier above (instance default, else the `claude` floor). A pin both
   *  selects the harness and shields it from every coarser tier. */
  pin_harness?: string | null;
  /** #550/ADR-0046: per-harness `{model, effort}` settings. `model`/`effort` above
   *  are the RESOLVED harness's view (folded from this map on load, back into it on
   *  save); the map preserves entries for the non-resolved harnesses. */
  harnesses?: Record<string, HarnessSettings>;
  /** Atomic agent selection for this node. Missing/`inherit` continues precedence. */
  agent_choice?: AgentChoice | null;
  /** Resources added only when this isolated node worktree is first created. */
  provisioning?: ProvisioningRules;
  /** #653/ADR-0060: where this node's NodeRun works — `true` a sub-worktree of
   *  its own, `false` the Run's shared worktree. Carried by `agent` and `script`
   *  only, and ALWAYS serialized for them (including at the default), so a
   *  document never leaves the reader to guess. Absent on `merge` (isolated by
   *  construction) and on structural nodes. */
  isolated_worktree?: boolean | null;
}

export interface AgentCombination {
  harness: string;
  model?: string | null;
  effort?: string | null;
}

export type AgentChoice =
  | { mode: "inherit" }
  | { mode: "profile"; profile_id: string }
  | ({ mode: "custom" } & AgentCombination);

export interface AgentProfile extends AgentCombination {
  id: string;
  name: string;
  created_at: string;
  updated_at: string;
}

export interface AgentProfileReferents {
  profile_id: string;
  instance: boolean;
  pipelines: { id: string; name: string; node_id?: string }[];
  runs: { run_id: string; name?: string | null }[];
  projects: { id: string; name: string }[];
  triggers: { id: string; name: string; pipeline_id: string }[];
}

/** #550/ADR-0046: a node's `{model, effort}` for one harness. Free-text
 *  pass-through — a slug means nothing outside the harness that accepts it. */
export interface HarnessSettings {
  model?: string | null;
  effort?: string | null;
}

export interface EdgeEndpoint {
  node: string;
  port: string;
}

/** A pinned waypoint on a manually-routed edge — absolute canvas coordinates. */
export interface EdgeWaypoint {
  x: number;
  y: number;
}

/**
 * Edge routing mode (issue #154). `auto` edges store no waypoints — their
 * right-angle path is recomputed deterministically and re-routes on node move.
 * `manual` edges pin the route to persisted `waypoints`. Both `mode` and
 * `waypoints` are LAYOUT, not semantics: they persist in the pipeline file (so
 * routing travels when a workflow is shared) but are excluded from the semantic
 * pipeline-diff (see `comparablePipelineObject`).
 */
export type EdgeRouteMode = "auto" | "manual";

export interface EdgeDef {
  source: EdgeEndpoint;
  target: EdgeEndpoint;
  reason?: string | null;
  /** Optional `when:` clause (ADR-0011): conditional routing on the edge. */
  when?: Record<string, unknown> | null;
  /** `else: true` marks a fallback edge (fires iff no sibling matched). */
  else?: boolean;
  /**
   * `repeated: true` marks an edge whose source artifact accumulates across
   * iterations (glob `iter-*`). Loop accumulation ("read all laps") lives on
   * the edge, not on a declared input port (ADR-0011 / #149).
   */
  repeated?: boolean;
  /** Routing mode (#154). Absent ⇒ `auto`. */
  mode?: EdgeRouteMode | null;
  /** Pinned absolute waypoints (#154). Only meaningful when `mode === "manual"`. */
  waypoints?: EdgeWaypoint[] | null;
  /**
   * The target card side the incoming arrow anchors on (#168). When an edge is
   * dropped on an emergent node body (ADR-0011 / #149), the arrow anchors on the
   * side nearest the drop point rather than always the left. Like `mode`/
   * `waypoints` this is LAYOUT, not semantics: it persists in the file (so a
   * shared workflow keeps its arrow arrival sides) but is excluded from the
   * semantic pipeline-diff. Absent ⇒ left (legacy anchoring).
   */
  target_side?: PortSide | null;
}

/**
 * The kind of a named loop region (ADR-0011 / #148, #151). `bounded` regions
 * carry an iteration counter and a `max_iter`; they are born by auto-detection
 * of a cycle so no cycle is ever accidentally unbounded. `collection` regions
 * (ex-ForEach) carry an `over: <field>` driver and fan the member(s) out in
 * parallel, one lap per item, barriering on completion.
 */
export type LoopKind = "bounded" | "collection";

/**
 * A named loop region (ADR-0011 / #148, #151). Replaces the `loop` and `ForEach`
 * nodes: the loop is identified by `id`, its body is the explicit `members` list
 * (>= 1 node). A `bounded` region has a region-wide iteration counter keyed by
 * `id` and renders with a `↻ X/Y` header. A `collection` region fans `over` a
 * list and renders with a `⇉ N items` badge. The canvas draws either as a
 * translucent box (>= 2 members) or a compact badge (1 member).
 */
export interface LoopRegion {
  id: string;
  kind: LoopKind;
  members: string[];
  max_iter?: number | string | null;
  /** The frontmatter field a `collection` region fans out over (#151). */
  over?: string | null;
}

/**
 * An inert canvas note (#307 / ADR-0018): a documentation post-it laid on the
 * canvas. It has no title, no port, no edge; it is never spawned and lives
 * outside the DAG and the runtime. Clicking it opens the detail panel to edit
 * its `content`. Like `view`/`waypoints`/`target_side` it is LAYOUT, not
 * semantics: it travels in the pipeline file but is excluded from the semantic
 * pipeline-diff (`comparablePipelineObject`), so the synced/diverged star does
 * not move when a note is created/moved/edited/deleted. Note the `note` xyflow
 * node `type` is a canvas concern only — it is NOT a PDO `NodeType`.
 */
export interface NoteDef {
  id: string;
  content: string;
  view?: { x: number; y: number } | null;
}

export interface PipelineDef {
  name: string;
  version?: string | null;
  variables: Record<string, VariableDef>;
  nodes: NodeDef[];
  edges: EdgeDef[];
  /** Named bounded loop regions (ADR-0011 / #148). Absent when there are none. */
  loops?: LoopRegion[];
  /** Inert canvas notes (#307 / ADR-0018). Absent when there are none. */
  notes?: NoteDef[];
  /**
   * Whether a manual Run must supply a non-empty prompt (#158). Defaults to
   * `true` (prompt mandatory) and is omitted from YAML in that case. When
   * `false`, a Run may start with empty input and a provided prompt is treated
   * as additional info.
   */
  prompt_required?: boolean;
}

export interface PipelineDetail {
  id: string;
  scope: PipelineScope;
  path: string;
  yaml: string;
  pipeline: PipelineDef;
  prompts: Record<string, string>;
  diagnostics: string[];
}

// Mirrors the daemon's `GET /stats/overview` (cheap indexed SQL) and
// `GET /stats/cost` (memoized per-run cost, app-folded) payloads.

export interface StatsBucketCount {
  /** Period label (e.g. `2026-07-15`), as produced by SQLite `strftime`. */
  bucket: string;
  count: number;
}

export interface StatsPipelineFireCount {
  /** Trigger's `pipeline_id`, or `"(deleted trigger)"` for an orphan fire. */
  pipeline_id: string;
  count: number;
}

export interface StatsTriggersCreatedRuns {
  /** Fires whose outcome was `fired` (⟺ a run was created) in the window. */
  fired: number;
  /** Distinct triggers that fired at least once in the window. */
  distinct_triggers: number;
  /** Triggers currently enabled (point-in-time, not windowed). */
  enabled_triggers: number;
}

export interface StatsOverview {
  /** Sorted union of period labels across runs/errors/sessions (the x-axis). */
  buckets: string[];
  runs: StatsBucketCount[];
  /** `run_failed` only — `run_skipped` is NOT an error. */
  errors: StatsBucketCount[];
  /** `node_started` starts (re-spawns and loop laps included, manager excluded). */
  sessions: StatsBucketCount[];
  /** Harness columns active in the selected Run cohort. */
  session_harnesses: string[];
  /** Session starts split by harness for the stacked chart. */
  sessions_by_period: StatsSessionPeriod[];
  /** Pipeline → Node session hierarchy. */
  sessions_by_pipeline: StatsSessionEntity[];
  fires_by_pipeline: StatsPipelineFireCount[];
  triggers_created_runs: StatsTriggersCreatedRuns;
}

export interface StatsSessionHarness {
  harness: string;
  executions: number;
}

export interface StatsSessionPeriod {
  bucket: string;
  harnesses: StatsSessionHarness[];
}

export interface StatsSessionEntity {
  id: string;
  name: string;
  executions: number;
  harnesses: StatsSessionHarness[];
  by_period: StatsSessionPeriod[];
  nodes: StatsSessionEntity[];
}

/** One harness's cost and denominator coverage within an aggregate. */
export interface StatsHarnessCost {
  harness: string;
  /** Null means no readable contribution, never zero. */
  usd: number | null;
  estimated: boolean;
  partial: boolean;
  executions: number;
  readable: number;
  unknown: number;
  average_usd: number | null;
  unpriced_models: string[];
  missing_reasons: string[];
}

/** Cost shared by Total, periods, Projects, Pipelines and Nodes. */
export interface StatsCostAggregate {
  usd: number | null;
  average_usd: number | null;
  estimated: boolean;
  partial: boolean;
  executions: number;
  readable: number;
  unknown: number;
  unpriced_models: string[];
  missing_reasons: string[];
  harnesses: StatsHarnessCost[];
}

export interface StatsCostPeriod extends StatsCostAggregate {
  bucket: string;
}

export interface StatsCostEntity extends StatsCostAggregate {
  id: string;
  name: string;
  by_period: StatsCostPeriod[];
  nodes: StatsCostEntity[];
}

export interface StatsProjectCostEntity extends StatsCostEntity {
  pipelines: StatsCostEntity[];
}

/** One resolved price row (#528): a family, the tier that decides it, the price in
 *  force. Rides on `/stats/cost` beside the "Sync costs" action — the same table
 *  the cost fold bills with, so what the Cost tab shows can never drift (#373). */
export interface PriceRow {
  /** FAMILY key, un-dated (`claude-opus-4-8`), not a dated `message.model`. */
  key: string;
  tier: "manual" | "fetched" | "embedded";
  /** $/MTok in — the price ACTUALLY applied (the winning tier). */
  input: number;
  /** $/MTok out — the price ACTUALLY applied (the winning tier). */
  output: number;
}

export interface StatsCost {
  harnesses: string[];
  total: StatsCostAggregate;
  by_period: StatsCostPeriod[];
  by_pipeline: StatsCostEntity[];
  by_project: StatsProjectCostEntity[];
  /** The resolved price table, one row per family in alphabetical order (#528).
   *  Window-independent — a property of the price table, not the fold. Refreshed
   *  by the "Sync costs" refetch on the Cost tab. */
  resolved: PriceRow[];
}

/** One R-7 distribution and its measurement coverage (#585). */
export interface StatsDistribution {
  stats: {
    min: number;
    q1: number;
    median: number;
    mean: number;
    q3: number;
    max: number;
  } | null;
  measured: number;
  expected: number;
  missing_reasons: string[];
}

export interface StatsHarnessPerformance {
  harness: string;
  context: StatsDistribution;
  duration: StatsDistribution;
}

export interface StatsPerformanceAggregate {
  harnesses: StatsHarnessPerformance[];
}

export interface StatsPerformanceEntity extends StatsPerformanceAggregate {
  id: string;
  name: string;
  nodes: StatsPerformanceEntity[];
  subagents: StatsPerformanceEntity[];
}

/** Derived `GET /stats/performance` payload. */
export interface StatsPerformance {
  harnesses: string[];
  total: StatsPerformanceAggregate;
  infrastructure_total: StatsPerformanceAggregate;
  by_pipeline: StatsPerformanceEntity[];
  infrastructure: StatsPerformanceEntity[];
}
