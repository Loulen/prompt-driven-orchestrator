/**
 * Hand-written shapes of the harness entries of `GET /settings`, built with
 * `json!` on the daemon side (no Rust DTO yet).
 */

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
  /** #705: the context window a source published beside a model id (`pi
   *  --list-models`), keyed by id, verbatim (`200K`, `1M`). A picker hint, never a
   *  guard. Absent/empty when no source names one. */
  model_contexts?: Record<string, string>;
  /** #616/ADR-0053: the effort levels the binary offers. Empty ⇒ no effort axis. */
  efforts?: string[];
  /** #798: the effort levels an exact model id supports, keyed by that id as it
   *  appears in `models`, verbatim. A key PRESENT for a model is authoritative
   *  for it — `[]` included, meaning the model supports no effort level. A key
   *  MISSING for a model falls back to the harness's global `efforts` (no
   *  client-side exceptions). Optional so a daemon predating #798 still
   *  typechecks — the consumer then treats every model as `efforts` says. */
  model_efforts?: Record<string, string[]>;
  /** #616/ADR-0053: the served effort-axis fact — whether this harness has an
   *  effort axis at all. Drives the effort-picker greying, replacing the client's
   *  hard-coded map. */
  has_effort?: boolean;
  /** #616/ADR-0053: the probed binary version the catalogue was read at, for the
   *  picker's provenance line. `null` when the binary answered no `--version`. */
  version?: string | null;
}
