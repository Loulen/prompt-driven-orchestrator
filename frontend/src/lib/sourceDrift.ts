import type { BranchFetchError, SourceDrift } from "../types";
import { shortAge } from "./branchSelect";

/**
 * How the **dérive de la source** reads on a Run (#803, ADR-0070 §4; CONTEXT.md
 * § « Dérive de la source »).
 *
 * The pure half of the Run view's chip: everything here is a function of the
 * daemon's answer plus the outcome of the last explicit fetch. It lives in the lib
 * (not in the panels) because the SAME chip renders in two places — the Info tab's
 * Source block and the Repositories tab's primary row — and two copies of "what does
 * `1↑ 3↓` mean here" would drift the way #571 documented for branch logic.
 *
 * The measurement itself never fetches: the daemon reads local refs on every call,
 * and the Run view's fetch button is the only thing that refreshes the remote side.
 */

/**
 * What the chip shows, as one of four shapes.
 *
 * `unknown` is the state the daemon cannot report, because it is not a property of
 * the repo: the LAST FETCH FAILED, so the `m` axis is dated from a success nobody
 * can put a date on any more. The `n` axis survives — it is the Run's own commits,
 * counted on a local branch that no remote has an opinion about — which is why this
 * is a per-axis unknown (`1↑ ?`) and not a blanket "unavailable".
 */
export type DriftChip =
  | { kind: "unavailable"; reason: string }
  | { kind: "unknown"; ahead: number }
  | { kind: "drift"; ahead: number; behind: number };

export function driftChip(
  drift: SourceDrift | null,
  fetchError: BranchFetchError | null,
): DriftChip | null {
  if (!drift) return null;
  if (drift.state === "unavailable") {
    return { kind: "unavailable", reason: drift.reason };
  }
  if (fetchError) return { kind: "unknown", ahead: drift.ahead };
  return { kind: "drift", ahead: drift.ahead, behind: drift.behind };
}

/** `1↑` · `3↓` · `?` — one span per axis, so the two can be painted apart. */
export function driftAxes(chip: DriftChip): { ahead: string; behind: string } | null {
  if (chip.kind === "unavailable") return null;
  return {
    ahead: `${chip.ahead}↑`,
    behind: chip.kind === "unknown" ? "?" : `${chip.behind}↓`,
  };
}

/**
 * The tooltip's lines (design B): what each number counts, how the `m` splits
 * between the local source and its tracking branch, the fork, and — the point of
 * the whole thing — what it means for the merge back.
 *
 * Returned as lines rather than a string so the component can style the numbers;
 * an empty array means there is nothing worth hovering for.
 */
export function driftTooltip(
  drift: SourceDrift | null,
  runId: string,
  fetchError: BranchFetchError | null,
  now: number = Date.now(),
): string[] {
  if (!drift) return [];
  if (drift.state === "unavailable") return [drift.reason];

  const lines: string[] = [];
  const short = runId.length > 8 ? `pdo/run-…${runId.slice(-3)}` : `pdo/run-${runId}`;
  lines.push(
    `${drift.ahead}↑ ${plural(drift.ahead, "commit")} on ${short} since the fork`,
  );

  if (fetchError) {
    // Honest about WHICH half is stale: the local side was still counted, the
    // remote side is dated from a fetch that never landed.
    lines.push(`↓ unknown — the last fetch failed: ${fetchError.message}`);
    if (drift.local_behind != null) {
      lines.push(`Local ${drift.source_branch} has ${drift.local_behind} since the fork.`);
    }
  } else {
    lines.push(`${drift.behind}↓ arrived on the source: ${sideDetail(drift)}`);
  }

  const fetched = shortAge(drift.last_fetch_at, now);
  lines.push(
    `Fork ${drift.fork} · local refs, remote as of ${
      fetched ? `fetch ${fetched} ago` : "no fetch on record"
    }`,
  );
  if (!fetchError && drift.behind > 0) {
    // The consequence, not the measurement — the reason anyone reads the chip.
    lines.push("Merging back will need a merge or rebase.");
  }
  return lines;
}

/** `main +1 · origin/main +3`, or just one side when there is only one. */
function sideDetail(
  drift: Extract<SourceDrift, { state: "available" }>,
): string {
  const parts: string[] = [];
  if (drift.local_behind != null) parts.push(`${drift.source_branch} +${drift.local_behind}`);
  if (drift.upstream && drift.upstream_behind != null) {
    parts.push(`${drift.upstream} +${drift.upstream_behind}`);
  }
  // A tracking-ref source has neither side to split: the branch itself is the count.
  if (parts.length === 0) parts.push(`${drift.source_branch} +${drift.behind}`);
  return parts.join(" · ");
}

function plural(n: number, word: string): string {
  return n === 1 ? word : `${word}s`;
}
