/**
 * Split a unified `git diff` into per-file sections (#376). Pure, deterministic,
 * no React and no DOM: the daemon returns the raw patch text; this turns it into
 * a list of {@link FileDiff} so `DiffSection` can render one collapsible group
 * per file instead of one monolithic `<pre>`.
 *
 * Robust by contract: it never throws on malformed input — a chunk it can't
 * fully understand still yields a best-effort `FileDiff` (verbatim `body`, empty
 * paths) rather than aborting the whole parse. See CONTEXT.md § "Diff de Run".
 */

export type FileDiffStatus =
  | "added"
  | "deleted"
  | "modified"
  | "renamed"
  | "copied";

export interface FileDiff {
  /** Source path, or `null` for a pure addition (`--- /dev/null`). */
  oldPath: string | null;
  /** Destination path, or `null` for a pure deletion (`+++ /dev/null`). */
  newPath: string | null;
  /** `"old → new"` for a rename/copy, else `newPath ?? oldPath ?? ""`. */
  displayPath: string;
  status: FileDiffStatus;
  isBinary: boolean;
  additions: number;
  deletions: number;
  /** Verbatim section text (header + hunks) for a `<pre>`. */
  body: string;
}

/**
 * Parse a full `git diff` into its per-file sections, in patch order. Returns
 * `[]` for empty/whitespace-only input. Never throws.
 */
export function parseUnifiedDiff(raw: string): FileDiff[] {
  if (!raw || raw.trim().length === 0) return [];

  // Split on each `diff --git` boundary, keeping the boundary line at the head
  // of every chunk. The first chunk (any preamble before the first `diff --git`)
  // is dropped by the `startsWith` filter below.
  const chunks = raw
    .split(/(?=^diff --git )/m)
    .filter((c) => c.startsWith("diff --git "));

  return chunks.map(parseChunk);
}

/** Parse one `diff --git …` chunk. Best-effort: never throws. */
function parseChunk(body: string): FileDiff {
  try {
    return parseChunkStrict(body);
  } catch {
    // Malformed chunk: keep the text, give up on metadata.
    return {
      oldPath: null,
      newPath: null,
      displayPath: "",
      status: "modified",
      isBinary: false,
      additions: 0,
      deletions: 0,
      body,
    };
  }
}

function parseChunkStrict(body: string): FileDiff {
  const lines = body.split("\n");

  let renameFrom: string | null = null;
  let renameTo: string | null = null;
  let copyFrom: string | null = null;
  let copyTo: string | null = null;
  let minusPath: string | null | undefined; // undefined = no `---` line seen
  let plusPath: string | null | undefined; // undefined = no `+++` line seen
  let binaryOld: string | null = null;
  let binaryNew: string | null = null;

  let isBinary = false;
  let hasNewFileMode = false;
  let hasDeletedFileMode = false;
  let additions = 0;
  let deletions = 0;

  for (const line of lines) {
    if (line.startsWith("rename from ")) {
      renameFrom = decodePath(line.slice("rename from ".length));
    } else if (line.startsWith("rename to ")) {
      renameTo = decodePath(line.slice("rename to ".length));
    } else if (line.startsWith("copy from ")) {
      copyFrom = decodePath(line.slice("copy from ".length));
    } else if (line.startsWith("copy to ")) {
      copyTo = decodePath(line.slice("copy to ".length));
    } else if (line.startsWith("new file mode ")) {
      hasNewFileMode = true;
    } else if (line.startsWith("deleted file mode ")) {
      hasDeletedFileMode = true;
    } else if (line.startsWith("--- ")) {
      minusPath = markerPath(line, "a/");
    } else if (line.startsWith("+++ ")) {
      plusPath = markerPath(line, "b/");
    } else if (line === "GIT binary patch" || /^Binary files .* differ$/.test(line)) {
      isBinary = true;
      const m = /^Binary files (.*) and (.*) differ$/.exec(line);
      if (m) {
        binaryOld = stripPrefixOrNull(m[1], "a/");
        binaryNew = stripPrefixOrNull(m[2], "b/");
      }
    } else if (line.startsWith("+") && !line.startsWith("+++")) {
      additions++;
    } else if (line.startsWith("-") && !line.startsWith("---")) {
      deletions++;
    }
    // `\ No newline at end of file`, `@@`, `index`, `diff`, mode lines: ignored.
  }

  // Path resolution ladder: rename/copy headers → `---`/`+++` → binary line.
  let oldPath: string | null;
  let newPath: string | null;
  if (renameFrom !== null || renameTo !== null) {
    oldPath = renameFrom;
    newPath = renameTo;
  } else if (copyFrom !== null || copyTo !== null) {
    oldPath = copyFrom;
    newPath = copyTo;
  } else if (minusPath !== undefined || plusPath !== undefined) {
    oldPath = minusPath ?? null;
    newPath = plusPath ?? null;
  } else {
    oldPath = binaryOld;
    newPath = binaryNew;
  }

  const isRename = renameFrom !== null || renameTo !== null;
  const isCopy = !isRename && (copyFrom !== null || copyTo !== null);

  let status: FileDiffStatus;
  if (isRename) status = "renamed";
  else if (isCopy) status = "copied";
  else if (hasNewFileMode || oldPath === null) status = "added";
  else if (hasDeletedFileMode || newPath === null) status = "deleted";
  else status = "modified";

  if (isBinary) {
    additions = 0;
    deletions = 0;
  }

  const displayPath =
    isRename || isCopy
      ? `${oldPath ?? ""} → ${newPath ?? ""}`
      : (newPath ?? oldPath ?? "");

  return {
    oldPath,
    newPath,
    displayPath,
    status,
    isBinary,
    additions,
    deletions,
    body,
  };
}

/**
 * Resolve the path from a `--- `/`+++ ` marker line. Strips one trailing tab
 * (git's delimiter for paths with spaces), C-unquotes if quoted, maps
 * `/dev/null` → `null`, and strips the `a/`/`b/` prefix.
 */
function markerPath(line: string, prefix: "a/" | "b/"): string | null {
  let rest = line.slice(4); // after "--- " / "+++ "
  if (rest.endsWith("\t")) rest = rest.slice(0, -1);
  const path = decodePath(rest);
  if (path === "/dev/null" || path === null) return null;
  return path.startsWith(prefix) ? path.slice(prefix.length) : path;
}

/** Strip an `a/`/`b/` prefix, or return `null` for `/dev/null`. */
function stripPrefixOrNull(token: string, prefix: "a/" | "b/"): string | null {
  const path = decodePath(token);
  if (path === null || path === "/dev/null") return null;
  return path.startsWith(prefix) ? path.slice(prefix.length) : path;
}

/**
 * Decode a git path token: C-unquote if it is wrapped in double quotes
 * (`core.quotepath` output — octal escapes are raw UTF-8 bytes), else return it
 * verbatim. Returns `null` only for an empty token.
 */
function decodePath(token: string): string | null {
  const t = token;
  if (t.length === 0) return null;
  if (t.length >= 2 && t.startsWith('"') && t.endsWith('"')) {
    return cUnquote(t);
  }
  return t;
}

const ESCAPE_MAP: Record<string, number> = {
  a: 0x07,
  b: 0x08,
  f: 0x0c,
  n: 0x0a,
  r: 0x0d,
  t: 0x09,
  v: 0x0b,
  '"': 0x22,
  "\\": 0x5c,
};

/**
 * C-unquote a git-quoted path (surrounding double quotes included). Octal
 * escapes (`\NNN`) are individual UTF-8 bytes, so we build a byte array and
 * decode it once — a char-by-char replace would mojibake multi-byte codepoints.
 */
function cUnquote(quoted: string): string {
  const inner = quoted.slice(1, -1);
  const bytes: number[] = [];
  for (let i = 0; i < inner.length; i++) {
    const c = inner[i];
    if (c === "\\") {
      const n = inner[i + 1];
      if (n >= "0" && n <= "7") {
        let oct = "";
        let j = i + 1;
        while (j < inner.length && oct.length < 3 && inner[j] >= "0" && inner[j] <= "7") {
          oct += inner[j];
          j++;
        }
        bytes.push(parseInt(oct, 8) & 0xff);
        i = j - 1;
      } else if (n !== undefined && n in ESCAPE_MAP) {
        bytes.push(ESCAPE_MAP[n]);
        i++;
      } else if (n !== undefined) {
        pushUtf8(bytes, n);
        i++;
      }
      // trailing lone backslash: dropped.
    } else {
      pushUtf8(bytes, c);
    }
  }
  return new TextDecoder().decode(new Uint8Array(bytes));
}

/** Append the UTF-8 bytes of a single character to `bytes`. */
function pushUtf8(bytes: number[], ch: string): void {
  const code = ch.charCodeAt(0);
  if (code < 0x80) {
    bytes.push(code);
  } else {
    for (const b of new TextEncoder().encode(ch)) bytes.push(b);
  }
}
