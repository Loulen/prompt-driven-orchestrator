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
    ...(canBrowse
      ? [{ label: "Local folder…", icon: HardDrive, onSelect: () => onChoose({ kind: "local" }) }]
      : []),
    ...(canCreateRepository
      ? [
          { label: "Clone from a URL…", icon: Globe, onSelect: () => onChoose({ kind: "clone" }) },
          {
            label: "New empty repository…",
            icon: FolderPlus,
            hint: "git init",
            onSelect: () => onChoose({ kind: "new" }),
          },
        ]
      : []),
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
