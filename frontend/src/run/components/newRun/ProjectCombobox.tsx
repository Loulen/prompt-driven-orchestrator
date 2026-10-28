import { useEffect, useRef, useState } from "react";
import { Briefcase, Check, ChevronDown } from "lucide-react";
import type { Project } from "../../../workspace/model/project";
import { memberNames, repoCount } from "../../model/launchProject";

interface Props {
  /** The Projet chosen; `null` = no Projet. */
  value: string | null;
  onChange: (projectId: string | null) => void;
  /** The Projets offered, in the order shown. */
  projects: Project[];
  /** Whether « No project » is offered (the Droits allow a Run in no Projet). */
  allowNone: boolean;
  /** One line closing the menu (« 1 Project(s) hidden… »). */
  note?: string | null;
}

/**
 * The Projet a Run is launched in (ADR-0087), first field of New Run › Where:
 * the look of the repository field, each Projet with its repositories on a
 * second line, « No project » last when the Droits allow it.
 */
export default function ProjectCombobox({ value, onChange, projects, allowNone, note }: Props) {
  const [open, setOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const selected = value ? (projects.find((p) => p.id === value) ?? null) : null;

  useEffect(() => {
    function handleClickOutside(e: MouseEvent) {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) setOpen(false);
    }
    function handleKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape") setOpen(false);
    }
    document.addEventListener("mousedown", handleClickOutside);
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, []);

  const choose = (projectId: string | null) => {
    onChange(projectId);
    setOpen(false);
  };

  return (
    <div ref={containerRef} className="relative">
      <button
        type="button"
        id="launch-project"
        onClick={() => setOpen((o) => !o)}
        aria-haspopup="listbox"
        aria-expanded={open}
        className={`flex w-full items-center gap-2 rounded-md border bg-surface-2 px-2.5 py-1.5 text-left transition-colors focus:outline-none ${
          open ? "border-primary" : "border-line-strong hover:border-line-strong focus:border-primary"
        }`}
        style={{ fontSize: "12px" }}
        data-testid="project-combobox"
      >
        <span className={`min-w-0 flex-1 truncate ${selected ? "text-text" : "text-text-3"}`}>
          {selected ? selected.name : "No project"}
        </span>
        {selected && (
          <span className="shrink-0 font-mono text-text-3" style={{ fontSize: "10.5px" }}>
            {repoCount(selected)}
          </span>
        )}
        <ChevronDown size={13} className="shrink-0 text-text-3" />
      </button>
      {open && (
        <div
          role="listbox"
          aria-label="Projects"
          className="absolute left-0 right-0 z-50 mt-1 max-h-64 overflow-y-auto rounded-md border border-line bg-surface-2 py-1 shadow-lg"
          data-testid="project-combobox-menu"
        >
          {projects.length > 0 && (
            <div
              className="px-2.5 pb-0.5 pt-1 font-medium uppercase tracking-wider text-text-3"
              style={{ fontSize: "9.5px" }}
            >
              Your projects
            </div>
          )}
          {projects.map((project) => {
            const active = project.id === value;
            return (
              <button
                key={project.id}
                type="button"
                role="option"
                aria-selected={active}
                onMouseDown={(e) => {
                  e.preventDefault();
                  choose(project.id);
                }}
                className={`flex w-full items-center gap-2.5 px-2.5 py-1.5 text-left transition-colors hover:bg-active ${
                  active ? "bg-surface-2" : ""
                }`}
                data-testid="project-option"
                data-project-id={project.id}
              >
                <Briefcase size={12} className="shrink-0 text-text-3" />
                <span className="flex min-w-0 flex-1 flex-col">
                  <span className="truncate font-semibold text-text" style={{ fontSize: "12px" }}>
                    {project.name}
                  </span>
                  <span className="truncate font-mono text-text-3" style={{ fontSize: "10.5px" }}>
                    {project.members.length > 0 ? memberNames(project) : "no repository yet"}
                  </span>
                </span>
                {active && <Check size={14} className="shrink-0 text-primary" />}
              </button>
            );
          })}
          {allowNone && (
            <button
              type="button"
              role="option"
              aria-selected={value === null}
              onMouseDown={(e) => {
                e.preventDefault();
                choose(null);
              }}
              className={`flex w-full items-center gap-2.5 px-2.5 py-1.5 text-left text-text-3 transition-colors hover:bg-active ${
                projects.length > 0 ? "mt-1 border-t border-line" : ""
              } ${value === null ? "bg-surface-2" : ""}`}
              style={{ fontSize: "12px" }}
              data-testid="project-option-none"
            >
              <span className="w-3 shrink-0" />
              <span className="flex-1">No project</span>
              {value === null && <Check size={14} className="shrink-0 text-primary" />}
            </button>
          )}
          {note && (
            <div
              className="mt-1 border-t border-line px-2.5 pb-0.5 pt-1.5 text-text-3"
              style={{ fontSize: "10.5px" }}
              data-testid="project-combobox-note"
            >
              {note}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
