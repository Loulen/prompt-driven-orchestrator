import { useRef, useState } from "react";
import { Briefcase, Check, ChevronDown } from "lucide-react";
import type { Project } from "../../../workspace/model/project";
import { AnchoredPanel } from "../../../shared/ui/kit/layer";
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
 * The Projet a Run is launched in (ADR-0087), first row of New Run › Where:
 * the kit's dropdown, the repository count at its end; in the list, each
 * Projet with its folders as the row's meta, « No project » last when the
 * Droits allow it.
 */
export default function ProjectCombobox({ value, onChange, projects, allowNone, note }: Props) {
  const [open, setOpen] = useState(false);
  const [hi, setHi] = useState(-1);
  const btn = useRef<HTMLButtonElement>(null);
  const selected = value ? (projects.find((p) => p.id === value) ?? null) : null;
  // The rows the keyboard walks: the Projets, then « No project ».
  const rows: (string | null)[] = [...projects.map((p) => p.id), ...(allowNone ? [null] : [])];

  const openList = () => {
    setHi(Math.max(0, rows.indexOf(value)));
    setOpen(true);
  };
  const choose = (projectId: string | null) => {
    onChange(projectId);
    setOpen(false);
    btn.current?.focus({ preventScroll: true });
  };

  return (
    <>
      <button
        ref={btn}
        type="button"
        id="launch-project"
        className="d-sel ln-grow"
        title={selected ? `${selected.name} · ${memberNames(selected)}` : "No project"}
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => (open ? setOpen(false) : openList())}
        onKeyDown={(e) => {
          if (!open) {
            if (["ArrowDown", "ArrowUp", "Enter", " "].includes(e.key)) {
              e.preventDefault();
              openList();
            }
            return;
          }
          if (e.key === "ArrowDown" || e.key === "ArrowUp") {
            e.preventDefault();
            const dir = e.key === "ArrowDown" ? 1 : -1;
            setHi((h) => (h + dir + rows.length) % rows.length);
          } else if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            if (hi >= 0 && hi < rows.length) choose(rows[hi]);
          } else if (e.key === "Tab") setOpen(false);
        }}
        data-testid="project-combobox"
      >
        <Briefcase className="d-sel__lead" size={13} strokeWidth={1.5} aria-hidden />
        <span className={selected ? "d-sel__val" : "d-sel__val d-sel__val--ph"}>{selected ? selected.name : "No project"}</span>
        {selected && <span className="ln-sel__count">{repoCount(selected)}</span>}
        <ChevronDown className="d-sel__chev" size={12} strokeWidth={1.5} aria-hidden />
      </button>
      <AnchoredPanel anchor={btn} open={open} onClose={() => setOpen(false)}>
        <div role="listbox" aria-label="Projects" className="d-pop__list" data-testid="project-combobox-menu">
          {projects.length > 0 && <div className="d-caps d-pop__group">Your projects</div>}
          {projects.map((project, i) => (
            <div
              key={project.id}
              role="option"
              aria-selected={project.id === value}
              className="d-pop__row"
              data-hi={i === hi || undefined}
              title={`${project.name} · ${project.members.length > 0 ? memberNames(project) : "no repository"}`}
              onPointerMove={() => setHi(i)}
              onMouseDown={(e) => {
                e.preventDefault();
                choose(project.id);
              }}
              data-testid="project-option"
              data-project-id={project.id}
            >
              <span className="d-pop__mark">{project.id === value && <Check size={12} strokeWidth={2} aria-hidden />}</span>
              <Briefcase size={13} strokeWidth={1.5} className="d-pop__ic" aria-hidden />
              <span className="d-pop__label">{project.name}</span>
              <span className="d-pop__hint ln-proj__hint">
                {project.members.length > 0 ? memberNames(project) : "no repository"}
              </span>
            </div>
          ))}
          {allowNone && (
            <>
              {projects.length > 0 && <div className="d-pop__sep" />}
              <div
                role="option"
                aria-selected={value === null}
                className="d-pop__row"
                data-hi={hi === rows.length - 1 || undefined}
                onPointerMove={() => setHi(rows.length - 1)}
                onMouseDown={(e) => {
                  e.preventDefault();
                  choose(null);
                }}
                data-testid="project-option-none"
              >
                <span className="d-pop__mark">{value === null && <Check size={12} strokeWidth={2} aria-hidden />}</span>
                <span className="d-pop__label ln-faint">No project</span>
              </div>
            </>
          )}
          {note && (
            <div className="d-pop__empty ln-repo__note" data-testid="project-combobox-note">
              {note}
            </div>
          )}
        </div>
      </AnchoredPanel>
    </>
  );
}
