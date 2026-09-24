/**
 * A path under the daemon's home, as a person reads it: `~/code/shop-app`
 * rather than `/home/me/code/shop-app`. Any other path, or no known home, is
 * returned as is. Display only: the full path stays the value (and the title).
 */
export function tildify(path: string, home: string | null | undefined): string {
  if (!home) return path;
  const root = home.replace(/\/+$/, "");
  if (path === root) return "~";
  if (root && path.startsWith(`${root}/`)) return `~${path.slice(root.length)}`;
  return path;
}
