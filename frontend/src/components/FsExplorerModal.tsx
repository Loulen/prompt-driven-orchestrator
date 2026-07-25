import { useCallback, useEffect, useState } from "react";
// `File` is aliased: the bare name would shadow the DOM `File` global in this module.
import { ArrowUp, File as FileIcon, Folder, FolderGit2, Link2 } from "lucide-react";
import { browseFs } from "../api";
import type { BrowseEntry, BrowseOptions } from "../api";

interface Props {
  /**
   * What the user picks. `"dir"` (the default) is today's behaviour exactly:
   * directories-only listing, and the pick is the folder you are standing in.
   * `"file"` lists regular files alongside the navigable directories and the pick is
   * a selected file. A TS union, not an enum (`erasableSyntaxOnly` is on).
   *
   * Note the singular: the value names what is *chosen*, not what is *listed* — in
   * file mode directories stay listed, since you have to traverse them.
   */
  mode?: "dir" | "file";
  /** Show dot-entries. A FIXED consumer decision, not a user toggle. */
  showHidden?: boolean;
  /** Directory to open at; omitted → the daemon's default chain. */
  startPath?: string;
  /** Optional title. Omitted → no title row, which keeps RepoCombobox pixel-identical. */
  title?: string;
  /** Confirm-button label. Omitted → "Select this folder" / "Select this file". */
  confirmLabel?: string;
  /**
   * Called with the chosen absolute path. Invoked SYNCHRONOUSLY from the click
   * handler (`RepoCombobox.test.tsx` asserts `onChange` with no `waitFor`), then
   * `onClose`.
   */
  onPick: (path: string) => void;
  onClose: () => void;
  /**
   * Testid namespace: `${testIdPrefix}-{backdrop,up,path,error,entry,git-dot,symlink,select}`.
   * New consumers leave the default.
   */
  testIdPrefix?: string;
  /**
   * Escape hatch for the CONTAINER testid alone. It exists only because #131 shipped
   * an irregular `repo-browser-modal` (`browser`, not `browse`) that is pinned by both
   * `RepoCombobox.test.tsx` and `e2e/repo-explorer-pick.spec.ts`.
   */
  modalTestId?: string;
}

/**
 * The generic filesystem explorer (#131, extracted from `RepoCombobox` in #431).
 *
 * A self-contained nested modal over `GET /fs/browse`: own full-screen backdrop at
 * `z-[60]` (above the `z-50` of every parent modal), own Escape handler that closes
 * this layer alone. Two consumers today — the New-Run repo picker (`mode="dir"`,
 * unchanged to the pixel) and the settings Dockerfile picker (`mode="file"`,
 * `showHidden`).
 *
 * `role="dialog"` / a focus trap are deliberately out of scope: they are absent since
 * #131, and adding them here would change observable behaviour under an extraction
 * that is meant to change none.
 */
export default function FsExplorerModal({
  mode = "dir",
  showHidden = false,
  startPath,
  title,
  confirmLabel,
  onPick,
  onClose,
  testIdPrefix = "fs-browse",
  modalTestId,
}: Props) {
  const [currentDir, setCurrentDir] = useState("");
  const [parent, setParent] = useState<string | null>(null);
  const [entries, setEntries] = useState<BrowseEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [truncated, setTruncated] = useState(false);
  /** File mode only: the selected file, `null` until one is clicked. */
  const [picked, setPicked] = useState<string | null>(null);

  // Navigate to `path` (omit → backend default root). Always lands on a 200 shape: an
  // in-body `error` (e.g. permission denied) is surfaced inline while the breadcrumb is
  // kept, so the user is never stranded on a blank pane.
  const navigateTo = useCallback(
    async (path?: string) => {
      setLoading(true);
      setError(null);
      try {
        // LOAD-BEARING ARITY: never `browseFs(path, {})` nor `browseFs(path, undefined)`
        // in default mode — either would be a recorded SECOND argument and the frozen
        // assertions in `RepoCombobox.test.tsx` (which pin one-argument calls) would break.
        const extra: BrowseOptions | undefined =
          mode === "file" || showHidden
            ? { files: mode === "file", hidden: showHidden }
            : undefined;
        const data = extra ? await browseFs(path, extra) : await browseFs(path);
        setCurrentDir(data.path);
        setParent(data.parent);
        setEntries(data.entries);
        setTruncated(data.truncated);
        setError(data.error);
        // Leaving a directory drops a stale selection from the one we left.
        setPicked(null);
      } catch (e) {
        setError(e instanceof Error ? e.message : "Failed to browse");
      } finally {
        setLoading(false);
      }
    },
    [mode, showHidden],
  );

  // Initial fetch on mount — the modal is mounted only when opened, so mounting IS
  // "the user opened the explorer". StrictMode double-invokes this in dev → two
  // identical `GET /fs/browse`. Harmless (an idempotent read); do NOT add a `useRef`
  // guard, it would break legitimate re-opens.
  //
  // `navigateTo` flips `loading` synchronously (that IS the "Loading…" pane), which is
  // exactly what `set-state-in-effect` warns about; same trade-off, same disable, as
  // `NewRunModal`/`TriggerDetailPanel`. `[]` deps on purpose: `startPath` is an open-at
  // value read once, and re-fetching when it changes mid-open would fight the user's
  // own navigation.
  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect -- see note above.
    void navigateTo(startPath);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Escape closes THIS layer alone. `document` (not `window`) + `stopPropagation`,
  // both load-bearing: sibling document listeners in a parent (e.g. RepoCombobox's
  // recents dropdown) fire in registration order.
  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    }
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [onClose]);

  const handleRow = (entry: BrowseEntry) => {
    // Dir mode: the listing is directories-only by contract, so a row is ALWAYS a
    // navigation and `is_dir` is never consulted — which keeps the frozen
    // `RepoCombobox` tests independent of a field their fixtures predate.
    if (mode === "dir" || entry.is_dir) void navigateTo(entry.path);
    else setPicked(entry.path);
  };

  // Dir mode picks the folder you are standing in (git-dotted or not — ADR-0001: any
  // folder is pickable, the authoritative check gates it downstream). File mode is
  // select-then-confirm: one path to `onPick`, and a misclick can never silently write
  // a persisted setting and close the box.
  const confirmTarget = mode === "file" ? picked : currentDir;
  const confirm = () => {
    if (!confirmTarget) return;
    onPick(confirmTarget);
    onClose();
  };

  const modalId = modalTestId ?? `${testIdPrefix}-modal`;
  const emptyLabel = mode === "file" ? "Nothing here" : "No subfolders here";

  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/50"
      data-testid={`${testIdPrefix}-backdrop`}
      onClick={(e) => {
        // Unconditional `stopPropagation`: keeps a backdrop click from bubbling up the
        // React tree to a parent modal's close handler.
        e.stopPropagation();
        onClose();
      }}
    >
      <div
        className="flex max-h-[70vh] w-[460px] flex-col rounded-lg border border-line bg-bg-4 shadow-xl"
        data-testid={modalId}
        onClick={(e) => e.stopPropagation()}
      >
        {title && (
          <div className="border-b border-line px-3 py-2">
            <span className="font-semibold text-fg" style={{ fontSize: "12.5px" }}>
              {title}
            </span>
          </div>
        )}

        {/* Header: up affordance + breadcrumb */}
        <div className="flex items-center gap-2 border-b border-line px-3 py-2">
          <button
            type="button"
            onClick={() => parent && void navigateTo(parent)}
            disabled={parent == null}
            className="flex shrink-0 items-center justify-center rounded p-1 text-fg-3 transition-colors hover:bg-bg-5 hover:text-fg disabled:opacity-30 disabled:hover:bg-transparent"
            title="Up one level"
            aria-label="Up one level"
            data-testid={`${testIdPrefix}-up`}
          >
            <ArrowUp size={14} />
          </button>
          <span
            className="min-w-0 flex-1 truncate font-mono text-fg-2"
            style={{ fontSize: "11.5px" }}
            title={currentDir}
            data-testid={`${testIdPrefix}-path`}
          >
            {currentDir || "…"}
          </span>
        </div>

        {/* Body: entry list */}
        <div className="min-h-[120px] flex-1 overflow-y-auto">
          {error && (
            <div
              className="px-3 py-2 font-mono text-st-failed"
              style={{ fontSize: "11px" }}
              data-testid={`${testIdPrefix}-error`}
            >
              {error}
            </div>
          )}
          {truncated && (
            <div className="px-3 py-1 text-fg-4" style={{ fontSize: "10.5px" }}>
              {/* FROZEN COPY: `RepoCombobox.test.tsx` asserts /Showing first 1000/. */}
              Showing first 1000 {mode === "file" ? "entries" : "folders"}
            </div>
          )}
          {loading && entries.length === 0 && (
            <div className="px-3 py-2 text-fg-4" style={{ fontSize: "11.5px" }}>
              Loading…
            </div>
          )}
          {!loading && !error && entries.length === 0 && (
            <div className="px-3 py-2 text-fg-4" style={{ fontSize: "11.5px" }}>
              {emptyLabel}
            </div>
          )}
          <ul>
            {entries.map((entry) => (
              <li key={entry.path}>
                <button
                  type="button"
                  onClick={() => handleRow(entry)}
                  className={`flex w-full items-center gap-2 px-3 py-1.5 text-left transition-colors hover:bg-bg-5 ${
                    picked === entry.path ? "bg-bg-5" : ""
                  }`}
                  data-testid={`${testIdPrefix}-entry`}
                >
                  {entry.is_git_repo ? (
                    <FolderGit2
                      size={14}
                      className="shrink-0 text-acc"
                      data-testid={`${testIdPrefix}-git-dot`}
                    />
                  ) : entry.is_dir || mode === "dir" ? (
                    <Folder size={14} className="shrink-0 text-fg-4" />
                  ) : (
                    <FileIcon size={14} className="shrink-0 text-fg-4" />
                  )}
                  <span
                    className="min-w-0 flex-1 truncate font-mono text-fg"
                    style={{ fontSize: "12px" }}
                  >
                    {entry.name}
                  </span>
                  {entry.is_symlink && (
                    <Link2
                      size={12}
                      className="shrink-0 text-fg-4"
                      aria-label="symlink"
                      data-testid={`${testIdPrefix}-symlink`}
                    />
                  )}
                </button>
              </li>
            ))}
          </ul>
        </div>

        {/* Footer: cancel + confirm */}
        <div className="flex items-center justify-end gap-2 border-t border-line px-3 py-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded-md px-3 py-1.5 text-fg-3 transition-colors hover:text-fg"
            style={{ fontSize: "11.5px" }}
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={confirm}
            disabled={!confirmTarget}
            className="rounded-md bg-acc px-3 py-1.5 font-medium text-bg-1 transition-opacity hover:opacity-90 disabled:opacity-40"
            style={{ fontSize: "11.5px" }}
            data-testid={`${testIdPrefix}-select`}
          >
            {confirmLabel ?? (mode === "file" ? "Select this file" : "Select this folder")}
          </button>
        </div>
      </div>
    </div>
  );
}
