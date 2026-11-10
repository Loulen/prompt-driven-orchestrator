/**
 * Path math of the staging-profile editor: turning what the filesystem explorer
 * picks (ABSOLUTE paths) into what a profile stores.
 */

/** Open-at directory for the Dockerfile picker (#431/#467): the PARENT of the current
 *  absolute path, so the explorer lands where the file lives rather than at `$HOME`.
 *  Anything non-absolute (or absent) → undefined, i.e. the daemon's default chain. The
 *  daemon also clamps a file path to its parent, but resolving it here keeps the very
 *  first request honest about what it is asking for. */
export function dockerfileStartDir(path: string | null): string | undefined {
  const trimmed = (path ?? "").trim();
  if (!trimmed.startsWith("/")) return undefined;
  const lastSlash = trimmed.lastIndexOf("/");
  return lastSlash <= 0 ? "/" : trimmed.slice(0, lastSlash);
}

/**
 * Turn an absolute path from the filesystem explorer into a `$HOME`-relative profile
 * entry. `null` when it does not live under `$HOME` (or `$HOME` is unknown), so the caller
 * can say so inline instead of firing a `PUT` the daemon will 400.
 *
 * The reason `GET /settings` grew a `home` field: `onPick` yields an ABSOLUTE path, an
 * entry is RELATIVE, and no endpoint exposed `$HOME` before this. Do NOT derive it from
 * any other path the payload happens to carry — `home` lives behind the
 * `sandbox_home_override` seam, and the daemon revalidates at the edge regardless.
 */
export function relativiseToHome(abs: string, home: string | null): string | null {
  if (!home) return null;
  const h = home.replace(/\/+$/, "");
  if (abs === h) return null; // `$HOME` itself is not an entry
  const prefix = `${h}/`;
  if (!abs.startsWith(prefix)) return null;
  const rel = abs.slice(prefix.length).replace(/\/+$/, "");
  return rel.length > 0 ? rel : null;
}
