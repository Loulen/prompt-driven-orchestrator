import { Briefcase, ChevronDown, FolderPlus, Globe, HardDrive, History, Plus } from "lucide-react";
import { tildify } from "../../../lib/homePath";
import { Button, Menu, type MenuEntry } from "../../../shared/ui/kit";
import type { ProjectRepository } from "../../model/projectsSettings";

/** What « Add repository » offers: a repository of another Projet, a recent one, a local folder, a URL to clone, a new empty one. */
export type AddRepositoryChoice =
  | { kind: "project"; path: string }
  | { kind: "recent"; path: string }
  | { kind: "local" }
  | { kind: "clone" }
  | { kind: "new" };

/**
 * « Add repository » of a Projet in Settings › Projects (CORE-96) and its
 * menu. Adding a repository is judged in the Projet: a URL to clone (CORE-97),
 * a new empty repository (`git init`), a repository of another of the
 * caller's Projets (« From my Projects », the Projet's name as its hint).
 * A local folder (the explorer) and the recent repositories reach the disk:
 * who may read it keeps the menu it had, « From my Projects » first; for the
 * others, the Projet's gestures come first, and « Local folder… » stays,
 * greyed. A refused entry says why in its tooltip (CORE-205), never at the
 * menu's foot.
 */
export default function AddRepositoryMenu({
  fromProjects,
  recent,
  home,
  browseRefused,
  createRepositoryRefused,
  onChoose,
}: {
  /** The repositories of the caller's other Projets this one does not hold. */
  fromProjects: ProjectRepository[];
  recent: string[];
  home: string | null;
  /** CORE-205: why the caller may not read the disk (« Local folder… » greyed, no recent ones), else absent. */
  browseRefused?: string;
  /** CORE-205: why the caller may not create a repository for this Projet (clone and new greyed), else absent. */
  createRepositoryRefused?: string;
  onChoose: (choice: AddRepositoryChoice) => void;
}) {
  const fromMyProjects: MenuEntry[] =
    fromProjects.length > 0
      ? [
          { kind: "label", label: "From my Projects" },
          ...fromProjects.map(
            ({ path, projectName }): MenuEntry => ({
              label: tildify(path, home),
              icon: Briefcase,
              hint: projectName,
              onSelect: () => onChoose({ kind: "project", path }),
            }),
          ),
        ]
      : [];
  const clone: MenuEntry = {
    label: "Clone from a URL…",
    icon: Globe,
    disabledReason: createRepositoryRefused,
    onSelect: () => onChoose({ kind: "clone" }),
  };
  const empty: MenuEntry = {
    label: "New empty repository…",
    icon: FolderPlus,
    hint: "git init",
    disabledReason: createRepositoryRefused,
    onSelect: () => onChoose({ kind: "new" }),
  };
  const local: MenuEntry = {
    label: "Local folder…",
    icon: HardDrive,
    disabledReason: browseRefused,
    onSelect: () => onChoose({ kind: "local" }),
  };
  const separator: MenuEntry = { kind: "separator" };

  const entries: MenuEntry[] = browseRefused
    ? [
        clone,
        empty,
        ...(fromMyProjects.length > 0 ? [separator, ...fromMyProjects] : []),
        separator,
        local,
      ]
    : [
        ...fromMyProjects,
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
            ]
          : []),
        ...(fromMyProjects.length > 0 || recent.length > 0 ? [separator] : []),
        local,
        clone,
        empty,
      ];
  return (
    <span className="self-start">
      <Menu
        label="Add repository"
        reasons="tooltip"
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
