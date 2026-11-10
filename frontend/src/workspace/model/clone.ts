/**
 * A repository given by its URL (CORE-97): telling a URL from a path as the user
 * types, and the progress of its clone as a line.
 */

import type { CloneJob } from "../../shared/generated/CloneJob";

/**
 * Whether `text` names a remote repository rather than a path: a `scheme://` URL
 * git clones (`https`, `http`, `ssh`, `git`, `file`, `git+ssh`), or an scp-like
 * `user@host:path`. The daemon's own rule, mirrored so the field can switch at once.
 */
export function isRemoteUrl(text: string): boolean {
  const value = text.trim();
  const scheme = /^(https?|ssh|git|file|git\+ssh|ssh\+git):\/\/(.+)$/i.exec(value);
  if (scheme) return scheme[2].replace(/^\/+|\/+$/g, "") !== "";
  if (/\s/.test(value)) return false;
  const cut = value.indexOf(":");
  if (cut < 0) return false;
  const authority = value.slice(0, cut);
  const path = value.slice(cut + 1);
  return (
    authority.includes("@") &&
    !authority.includes("/") &&
    !authority.startsWith("@") &&
    !authority.endsWith("@") &&
    path.replace(/^\/+|\/+$/g, "") !== ""
  );
}

/** Where a clone is, as one line: `Receiving objects · 64 % · 38.20 MiB`. */
export function progressLine(job: CloneJob): string {
  const progress = job.progress;
  if (!progress) return "Starting the clone…";
  const parts = [progress.phase];
  if (progress.percent != null) parts.push(`${progress.percent} %`);
  if (progress.size) parts.push(progress.size);
  return parts.join(" · ");
}

/** How far the bar goes, 0 to 100. Resolving deltas follows receiving: the bar keeps going. */
export function progressPercent(job: CloneJob): number {
  const progress = job.progress;
  if (!progress || progress.percent == null) return 0;
  return progress.phase.toLowerCase().startsWith("resolving")
    ? 90 + progress.percent / 10
    : progress.phase.toLowerCase().startsWith("receiving")
      ? (progress.percent * 9) / 10
      : 0;
}
