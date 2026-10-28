import { useEffect, useRef, useState } from "react";
import { FolderPlus, Globe, HardDrive, History, Plus } from "lucide-react";
import { tildify } from "../../../lib/homePath";

/** What « Add repository » offers: a recent one, a local folder, a URL to clone, a new empty one. */
export type AddRepositoryChoice =
  | { kind: "recent"; path: string }
  | { kind: "local" }
  | { kind: "clone" }
  | { kind: "new" };

/**
 * « Add repository » of a Projet in Settings › Projects (CORE-96) and its
 * menu: the recent repositories it does not hold yet, then a local folder
 * (the explorer), a URL to clone (CORE-97) and a new empty repository
 * (`git init`).
 */
export default function AddRepositoryMenu({
  recent,
  home,
  canBrowse,
  canCreateRepository,
  onChoose,
}: {
  recent: string[];
  home: string | null;
  canBrowse: boolean;
  canCreateRepository: boolean;
  onChoose: (choice: AddRepositoryChoice) => void;
}) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const outside = (event: MouseEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    // Escape closes the menu alone, not Settings (the shell skips a handled Escape).
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      setOpen(false);
    };
    document.addEventListener("mousedown", outside);
    document.addEventListener("keydown", escape);
    return () => {
      document.removeEventListener("mousedown", outside);
      document.removeEventListener("keydown", escape);
    };
  }, [open]);

  const choose = (choice: AddRepositoryChoice) => {
    setOpen(false);
    onChoose(choice);
  };
  const item =
    "flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-text-2 transition-colors hover:bg-hover hover:text-text";

  return (
    <div ref={root} className="relative self-start">
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        data-testid="project-add-repo"
        className={`flex items-center gap-1.5 rounded-md border border-dashed px-2.5 py-1.5 transition-colors ${
          open ? "border-primary text-primary" : "border-line-strong text-text-3 hover:border-primary hover:text-primary"
        }`}
        style={{ fontSize: "11px" }}
      >
        <Plus size={11} />
        Add repository
      </button>
      {open && (
        <div
          role="menu"
          className="absolute left-0 top-full z-10 mt-1.5 w-[380px] rounded-md border border-line-strong bg-surface-1 p-1 shadow-xl"
          style={{ fontSize: "11px" }}
          data-testid="project-add-repo-menu"
        >
          {recent.length > 0 && (
            <>
              <div className="px-2 pb-1 pt-1.5 uppercase tracking-wider text-text-3" style={{ fontSize: "9.5px" }}>
                Recent
              </div>
              {recent.map((path) => (
                <button
                  key={path}
                  type="button"
                  role="menuitem"
                  className={item}
                  title={path}
                  onClick={() => choose({ kind: "recent", path })}
                  data-testid="project-add-repo-recent"
                >
                  <History size={12} className="shrink-0 text-text-3" />
                  <span className="truncate font-mono">{tildify(path, home)}</span>
                </button>
              ))}
              <div className="my-1 border-t border-line" />
            </>
          )}
          {canBrowse && (
            <button
              type="button"
              role="menuitem"
              className={item}
              onClick={() => choose({ kind: "local" })}
              data-testid="project-add-repo-local"
            >
              <HardDrive size={12} className="shrink-0 text-text-3" />
              <span className="flex-1">Local folder…</span>
              <span className="text-text-3" style={{ fontSize: "9.5px" }}>explorer</span>
            </button>
          )}
          {canCreateRepository && (
            <button
              type="button"
              role="menuitem"
              className={item}
              onClick={() => choose({ kind: "clone" })}
              data-testid="project-add-repo-clone"
            >
              <Globe size={12} className="shrink-0 text-st-running" />
              <span className="flex-1">Clone from a URL…</span>
            </button>
          )}
          {canCreateRepository && (
            <button
              type="button"
              role="menuitem"
              className={item}
              onClick={() => choose({ kind: "new" })}
              data-testid="project-add-repo-new"
            >
              <FolderPlus size={12} className="shrink-0 text-text-3" />
              <span className="flex-1">New empty repository…</span>
              <span className="text-text-3" style={{ fontSize: "9.5px" }}>git init</span>
            </button>
          )}
        </div>
      )}
    </div>
  );
}
