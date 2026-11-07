import { ChevronDown, FolderPlus, Globe, HardDrive, History, Plus } from "lucide-react";
import { tildify } from "../../../lib/homePath";
import { Button, Menu, type MenuEntry } from "../../../shared/ui/kit";

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
  browseRefused,
  createRepositoryRefused,
  onChoose,
}: {
  recent: string[];
  home: string | null;
  /** CORE-205: why the caller may not browse the disk (« Local folder… » greyed), else absent. */
  browseRefused?: string;
  /** CORE-205: why the caller may not create a repository (clone and new greyed), else absent. */
  createRepositoryRefused?: string;
  onChoose: (choice: AddRepositoryChoice) => void;
}) {
  const entries: MenuEntry[] = [
    ...(recent.length > 0
      ? [
          { kind: "label" as const, label: "Recent" },
          ...recent.map(
            (path): MenuEntry => ({
              label: tildify(path, home),
              icon: History,
              onSelect: () => onChoose({ kind: "recent", path }),
            }),
          ),
          { kind: "separator" as const },
        ]
      : []),
    // CORE-205: a refused entry stays, greyed; the menu says why at its foot.
    { label: "Local folder…", icon: HardDrive, disabledReason: browseRefused, onSelect: () => onChoose({ kind: "local" }) },
    {
      label: "Clone from a URL…",
      icon: Globe,
      disabledReason: createRepositoryRefused,
      onSelect: () => onChoose({ kind: "clone" }),
    },
    {
      label: "New empty repository…",
      icon: FolderPlus,
      hint: "git init",
      disabledReason: createRepositoryRefused,
      onSelect: () => onChoose({ kind: "new" }),
    },
  ];
  return (
    <span className="self-start">
      <Menu
        label="Add repository"
        entries={entries}
        trigger={({ open, toggle }) => (
          <Button size="sm" icon={Plus} aria-expanded={open} onClick={toggle} data-testid="project-add-repo">
            Add repository
            <ChevronDown size={11} strokeWidth={1.5} aria-hidden />
          </Button>
        )}
      />
    </span>
  );
}
