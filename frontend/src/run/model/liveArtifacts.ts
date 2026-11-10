/**
 * The live artifacts of an iteration (ADR-0089), as the Run view words them.
 */
import type { LiveArtifactIO } from "../../shared/generated/LiveArtifactIO";

/** `worker/iter-1/live/progress.md` -> `live/progress.md`: the iteration's
 *  folder is the panel's own context, the `live/` part is what the agent wrote. */
export function liveDisplayPath(path: string): string {
  const at = path.lastIndexOf("/live/");
  return at >= 0 ? path.slice(at + 1) : path;
}

/** The latest `updated_at` of the artifacts, or null when none is produced. */
export function newestUpdate(live: LiveArtifactIO[]): string | null {
  let newest: string | null = null;
  let newestMs = -Infinity;
  for (const artifact of live) {
    if (!artifact.updated_at) continue;
    const ms = Date.parse(artifact.updated_at);
    if (!Number.isNaN(ms) && ms > newestMs) {
      newestMs = ms;
      newest = artifact.updated_at;
    }
  }
  return newest;
}

/** `20 s ago`, `3 min ago`, `2 h ago`, `4 days ago`: precise while the node
 *  works, which is when the section is watched. */
export function liveUpdatedAgo(iso: string, now: number = Date.now()): string {
  const then = Date.parse(iso);
  if (Number.isNaN(then)) return iso;
  const seconds = Math.max(0, Math.round((now - then) / 1000));
  if (seconds < 60) return `${seconds} s ago`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  const days = Math.floor(hours / 24);
  return `${days} day${days === 1 ? "" : "s"} ago`;
}
