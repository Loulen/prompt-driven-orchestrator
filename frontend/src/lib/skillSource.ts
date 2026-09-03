/**
 * Client-side mirror of the daemon's `skill_import::parse_source` (#670): the
 * import popup parses the typed source on every keystroke to show the
 * `repo · ref · path` chips and the proposed folder name before any request.
 * The daemon's reading is the one stored; this only exists for feedback.
 */

export interface ParsedSource {
  kind: "git" | "local";
  /** Clone URL (git) or absolute folder (local). */
  url: string;
  ref: string | null;
  /** Sub-folder to scan, `/`-separated, `""` for the root. */
  path: string;
  /** `owner/repo`, or the folder name for a local source. */
  repo: string;
  suggestedFolder: string;
}

function stripGit(s: string): string {
  return s.endsWith(".git") ? s.slice(0, -4) : s;
}

function lastSegment(path: string): string | null {
  const parts = path.split("/").filter(Boolean);
  return parts.length ? parts[parts.length - 1] : null;
}

export function suggestedFolderName(repo: string, path: string): string {
  const last = lastSegment(path);
  return last ? `${repo} · ${last}` : repo;
}

/** `null` when the text is neither a repository URL nor an absolute local folder. */
export function parseSkillSource(input: string): ParsedSource | null {
  const text = input.trim();
  if (!text) return null;
  if (text.startsWith("git@") || text.startsWith("ssh://")) {
    const afterHost = text.startsWith("ssh://")
      ? text.slice("ssh://".length).split("/").slice(1).join("/")
      : text.split(":").slice(1).join(":");
    const repo = stripGit(afterHost.replace(/^\/+|\/+$/g, ""));
    if (!repo) return null;
    return { kind: "git", url: text, ref: null, path: "", repo, suggestedFolder: repo };
  }
  if (text.startsWith("file://")) {
    const repo = lastSegment(stripGit(text.slice("file://".length))) ?? "repo";
    return { kind: "git", url: text, ref: null, path: "", repo, suggestedFolder: repo };
  }
  const schemeIndex = text.indexOf("://");
  if (schemeIndex >= 0) {
    const scheme = text.slice(0, schemeIndex);
    if (scheme !== "http" && scheme !== "https") return null;
    const rest = text.slice(schemeIndex + 3).split(/[?#]/)[0];
    const [host, ...segments] = rest.split("/").filter(Boolean);
    if (!host || segments.length < 2) return null;
    const owner = segments[0];
    const repoName = stripGit(segments[1]);
    const repo = `${owner}/${repoName}`;
    let tail = segments.slice(2);
    if (tail[0] === "-") tail = tail.slice(1);
    let ref: string | null = null;
    let path = "";
    if ((tail[0] === "tree" || tail[0] === "blob") && tail.length >= 2) {
      ref = tail[1];
      const pathSegments = tail.slice(2);
      if (tail[0] === "blob") pathSegments.pop();
      path = pathSegments.join("/");
    }
    return {
      kind: "git",
      url: `${scheme}://${host}/${owner}/${repoName}`,
      ref,
      path,
      repo,
      suggestedFolder: suggestedFolderName(repo, path),
    };
  }
  if (text.startsWith("/") || text.startsWith("~")) {
    const normalised = text.replace(/\/+$/, "") || "/";
    const repo = lastSegment(normalised) ?? "folder";
    return { kind: "local", url: normalised, ref: null, path: "", repo, suggestedFolder: repo };
  }
  return null;
}

/** `3f9c2e1`-style short commit. */
export function shortCommit(commit: string | null | undefined): string {
  return commit ? commit.slice(0, 7) : "";
}

/** `github.com/anthropics/skills` for a URL, `~/x` untouched. */
export function displaySourceUrl(url: string): string {
  return url.replace(/^https?:\/\//, "").replace(/\.git$/, "");
}
