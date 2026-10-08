/**
 * The node library's shapes: an entry, its ports, what saving, instantiating
 * or parsing a node answers.
 */

import type { FrontmatterFieldDecl } from "../../shared/generated/FrontmatterFieldDecl";
import type { PortDef } from "../../shared/generated/PortDef";
import type { PortSide } from "../../shared/generated/PortSide";
import type { PortType } from "../../shared/generated/PortType";

export interface LibraryPort {
  name: string;
  repeated: boolean;
  side?: string | null;
  port_type?: PortType;
  frontmatter?: Record<string, FrontmatterFieldDecl> | null;
  when?: Record<string, unknown> | null;
  instructions?: string | null;
  /** An Output intermédiaire (CORE-45). Absent ⇒ an ordinary port. */
  intermediate?: boolean | null;
}

export function libraryPortToPortDef(port: LibraryPort, defaultSide: PortSide): PortDef {
  return {
    name: port.name,
    repeated: port.repeated,
    side: (port.side as PortSide) ?? defaultSide,
    ...(port.port_type ? { port_type: port.port_type } : {}),
    ...(port.frontmatter ? { frontmatter: port.frontmatter } : {}),
    ...(port.when ? { when: port.when } : {}),
    ...(port.instructions?.trim() ? { instructions: port.instructions } : {}),
    ...(port.intermediate ? { intermediate: true } : {}),
  };
}

export interface LibraryEntry {
  name: string;
  type: string;
  inputs: LibraryPort[];
  outputs: LibraryPort[];
  interactive: boolean;
  /** Per-node model override (#296/#345) — the node library is model-aware.
   * Absent/null ⇒ account default. */
  model?: string | null;
  /** Per-node effort override (#424) — the node library is effort-aware too.
   * Absent/null ⇒ account default. */
  effort?: string | null;
  max_iter?: number | null;
  branches?: number | null;
  prompt: string;
  /** #655/ADR-0060: where an instance of this entry works. Always stated by the
   * daemon for `agent`/`script` (stamped at the type's default when the entry
   * predates #655); `null` for the types that carry no isolation. */
  isolated_worktree?: boolean | null;
  /** ADR-0086: the entry is an on-demand node. Absent ⇒ `false`. */
  on_demand?: boolean;
}

export interface LibrarySaveSpec {
  name: string;
  type: string;
  inputs: LibraryPort[];
  outputs: LibraryPort[];
  interactive: boolean;
  /** Per-node model override (#296/#345). Omit/undefined ⇒ account default. */
  model?: string | null;
  /** Per-node effort override (#424). Omit/undefined ⇒ account default. */
  effort?: string | null;
  prompt: string;
  /** #655/ADR-0060: where the starred node works. Omitted ⇒ the daemon stamps
   * the type's default; `null` for a type that carries no isolation. */
  isolated_worktree?: boolean | null;
  /** ADR-0086: star the « on demand » option with the node. */
  on_demand?: boolean;
}

export interface InstantiateResult {
  spec: {
    name: string;
    type: string;
    inputs: LibraryPort[];
    outputs: LibraryPort[];
    interactive: boolean;
    /** Per-node model override (#296/#345). Null ⇒ account default. */
    model?: string | null;
    /** Per-node effort override (#424). Null ⇒ account default. */
    effort?: string | null;
    /** #655/ADR-0060: the entry's workspace, restored verbatim onto the new
     * node. Dropping it would fall back to the type default and silently fork a
     * worktree for an Agent starred in the Run's. */
    isolated_worktree?: boolean | null;
    /** ADR-0086: an on-demand entry instantiates an on-demand node. */
    on_demand?: boolean;
  };
  prompt: string;
}

/**
 * Parsed form of a single node's YAML (#345): the `POST /nodes/parse` 200 body.
 * `spec` is `LibraryEntry`-shaped (same as {@link InstantiateResult}) plus the
 * legacy `max_iter`/`branches`; `warnings` carries soft losses (coerced/unknown
 * fields) with the node still created. A hard failure throws (400 `{error}`).
 */
export interface ParseNodeResult {
  spec: {
    name: string;
    type: string;
    inputs: LibraryPort[];
    outputs: LibraryPort[];
    interactive: boolean;
    model?: string | null;
    effort?: string | null;
    /** #616 (correctif 7): the pinned harness and per-harness settings map, so a
     *  node exported with a harness axis round-trips through reimport without its
     *  model/effort re-homing onto `claude`. */
    pin_harness?: string | null;
    harnesses?: Record<string, { model?: string; effort?: string }> | null;
    /** #653/ADR-0060: where the node works. `null` for the types that carry no
     *  isolation (`start`, `end`). */
    isolated_worktree?: boolean | null;
    /** ADR-0086: the pasted node waits for the user's ▶. */
    on_demand?: boolean;
    max_iter?: number | string | null;
    branches?: number | null;
  };
  prompt: string;
  warnings: string[];
}
