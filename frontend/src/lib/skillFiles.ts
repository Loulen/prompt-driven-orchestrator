/**
 * The reference-files model shared by the paste popup and the skill detail
 * (#671). Pure: no React, no fetch. The popup stages files locally (nothing
 * touches disk before Create); the detail writes immediately. Both render the
 * same row grammar (icon · mono name · badge · size · action) and both refuse
 * folders in place, so the rules live here once.
 */

/** Per-file ceiling, mirrored from the daemon (`skill_bank::MAX_FILE_BYTES`). */
export const MAX_FILE_BYTES = 10 * 1024 * 1024;

export const SKILL_MD = "SKILL.md";

/** A dropped/picked `SKILL.md` is the skill's text, never a reference file. */
export function isSkillMd(path: string): boolean {
  return (path.split("/").pop() ?? path) === SKILL_MD;
}

/** Mirror of `skill_bank::normalise_file_path`: relative, `/`-separated, no `..`. */
export function normaliseRelativePath(raw: string): string | null {
  const trimmed = raw.trim().replace(/\\/g, "/");
  if (trimmed === "" || trimmed.startsWith("/")) return null;
  const parts = trimmed.split("/");
  if (parts.some((part) => part === "" || part === "." || part === "..")) return null;
  return parts.join("/");
}

/**
 * One file the user attached, before or after it reaches the daemon.
 * - `browser`: a `File` from a drop or `<input type=file>`, uploaded as multipart.
 * - `host`: an absolute path picked in the explorer; the daemon copies it.
 */
export type StagedSource =
  | { kind: "browser"; file: File }
  | { kind: "host"; fromPath: string };

export type StagedStatus =
  | { state: "staged" }
  | { state: "uploading" }
  | { state: "uploaded"; size: number }
  | { state: "failed"; message: string }
  | { state: "skipped" };

export interface StagedFile {
  /** Destination relative to the skill folder; the row's name. */
  path: string;
  /** `null` for a host pick: the explorer lists no sizes. */
  size: number | null;
  source: StagedSource;
  status: StagedStatus;
  /** Set when this row took the place of an earlier row with the same path. */
  replaces?: boolean;
}

export interface RefusedItem {
  name: string;
  reason: string;
}

/** What a drop or a pick yields once sorted: files to stage, a `SKILL.md`, refusals. */
export interface SortedDrop {
  files: StagedFile[];
  /** The last `SKILL.md` among the dropped files, if any (it replaces the text). */
  skillMd: File | null;
  refused: RefusedItem[];
}

/** The relative destination of a browser `File`: keeps a folder drop's sub-path. */
export function relativePathOf(file: File): string {
  const rel = (file as File & { webkitRelativePath?: string }).webkitRelativePath;
  return rel && rel !== "" ? rel : file.name;
}

/**
 * Sort a `DataTransfer` (or a picker's `FileList`) into stageable files, a
 * `SKILL.md`, and in-place refusals: folders ("Drop files, not folders"),
 * oversize files, unusable paths. A folder shows up as a `File` with no type and
 * a directory entry; `webkitGetAsEntry` is the only reliable tell.
 */
export function sortDroppedFiles(
  files: ArrayLike<File>,
  items?: ArrayLike<DataTransferItem> | null,
): SortedDrop {
  const out: SortedDrop = { files: [], skillMd: null, refused: [] };
  const directoryNames = new Set<string>();
  if (items) {
    for (let i = 0; i < items.length; i++) {
      const item = items[i];
      const entry = (item as DataTransferItem & { webkitGetAsEntry?: () => { isDirectory: boolean; name: string } | null })
        .webkitGetAsEntry?.();
      if (entry?.isDirectory) directoryNames.add(entry.name);
    }
  }
  for (let i = 0; i < files.length; i++) {
    const file = files[i];
    if (directoryNames.has(file.name) || (file.size === 0 && file.type === "" && looksLikeFolder(file))) {
      out.refused.push({ name: `${file.name}/`, reason: "Drop files, not folders" });
      continue;
    }
    if (isSkillMd(file.name)) {
      out.skillMd = file;
      continue;
    }
    const path = normaliseRelativePath(relativePathOf(file));
    if (!path) {
      out.refused.push({ name: file.name, reason: "Unusable file path" });
      continue;
    }
    if (file.size > MAX_FILE_BYTES) {
      out.refused.push({ name: path, reason: `${formatBytes(file.size)} · larger than the 10 MB limit` });
      continue;
    }
    out.files.push({ path, size: file.size, source: { kind: "browser", file }, status: { state: "staged" } });
  }
  return out;
}

function looksLikeFolder(file: File): boolean {
  // Without `items`, a directory dropped from a file manager is an empty, typeless
  // `File` whose name carries no extension. A genuinely empty `README` would be
  // refused too — the cost of never writing a folder as a 0-byte file.
  return !file.name.includes(".");
}

/** Host picks from the explorer: `SKILL.md` is a replacement, the rest are copies. */
export function sortHostPicks(paths: string[]): { files: StagedFile[]; skillMdPath: string | null } {
  let skillMdPath: string | null = null;
  const files: StagedFile[] = [];
  for (const fromPath of paths) {
    const name = fromPath.split("/").pop() ?? fromPath;
    if (isSkillMd(name)) {
      skillMdPath = fromPath;
      continue;
    }
    files.push({ path: name, size: null, source: { kind: "host", fromPath }, status: { state: "staged" } });
  }
  return { files, skillMdPath };
}

/**
 * Merge new rows into the staged list: a same-path row replaces the earlier
 * one in place (badge "replaces"), order otherwise preserved.
 */
export function mergeStaged(current: StagedFile[], incoming: StagedFile[]): StagedFile[] {
  const next = [...current];
  for (const file of incoming) {
    const index = next.findIndex((row) => row.path === file.path);
    if (index === -1) next.push(file);
    else next[index] = { ...file, replaces: true };
  }
  return next;
}

export function totalStagedBytes(files: StagedFile[]): number {
  return files.reduce((sum, file) => sum + (file.size ?? 0), 0);
}

/** `4.1 KB`-style sizes, the design's row grammar (the detail header keeps `formatSize`). */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/** Count of files an OS drag carries, for the overlay's "Drop to attach N files". */
export function draggedFileCount(dataTransfer: DataTransfer | null): number {
  if (!dataTransfer) return 0;
  if (dataTransfer.items && dataTransfer.items.length > 0) {
    let n = 0;
    for (let i = 0; i < dataTransfer.items.length; i++) if (dataTransfer.items[i].kind === "file") n++;
    return n;
  }
  return dataTransfer.files?.length ?? 0;
}

/** Whether a drag comes from the OS (files), not from the tree's own row drags. */
export function isFileDrag(dataTransfer: DataTransfer | null): boolean {
  if (!dataTransfer) return false;
  return Array.from(dataTransfer.types ?? []).includes("Files");
}
