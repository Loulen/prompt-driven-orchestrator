import { Star } from "lucide-react";
import MidPath from "../../shared/ui/MidPath";
import { tildify } from "../../lib/homePath";

/**
 * The Projet's member repositories, a list with its own maximum height in the
 * modal's scrolling body (#940): a box, the path cut in its middle, the other
 * Projets holding it as `+N` (ADR-0087: a repository may be in several). The
 * default primary repository (the one New Run preselects) is chosen among the
 * checked ones: its pill, or « Make default » on hover.
 */
export default function ProjectMembersList({
  candidates,
  checked,
  alsoIn,
  primary,
  onToggle,
  onPrimary,
  home,
}: {
  candidates: string[];
  checked: Set<string>;
  /** path → the names of the other Projets holding it. */
  alsoIn: Map<string, string[]>;
  /** The default primary repository, among the checked ones. */
  primary: string | null;
  onToggle: (path: string) => void;
  onPrimary: (path: string) => void;
  /** The daemon's home, to read a path as `~/code/…`. */
  home?: string | null;
}) {
  return (
    <div className="ln-mlist" data-testid="project-members-list">
      {candidates.length === 0 && <div className="d-empty--row ln-faint ln-mlist__none">No repository</div>}
      {candidates.map((path) => {
        const others = alsoIn.get(path) ?? [];
        const member = checked.has(path);
        const isPrimary = member && primary === path;
        return (
          <div
            key={path}
            className="ln-mlist__row"
            data-on={member || undefined}
            title={path}
            data-testid="project-member-row"
            data-path={path}
          >
            <label className="ln-mlist__label">
              <input
                type="checkbox"
                checked={member}
                onChange={() => onToggle(path)}
                aria-label={path}
                data-testid="project-member-checkbox"
              />
              <MidPath path={tildify(path, home)} title={false} className="ln-grow" />
            </label>
            {others.length > 0 && (
              <span className="ln-faint ln-also" title={`Also in ${others.join(", ")}`} aria-label={`also in ${others.join(", ")}`}>
                <span className="d-sr" data-testid="project-member-also-in">
                  also in {others.join(", ")}
                </span>
                <span aria-hidden>+{others.length}</span>
              </span>
            )}
            {member &&
              (isPrimary ? (
                <span className="d-chip ln-pill" title="Default repository: New Run preselects it" data-testid="project-member-primary">
                  default
                </span>
              ) : (
                <button
                  type="button"
                  className="d-btn d-btn--sm d-btn--ghost ln-mlist__make"
                  onClick={() => onPrimary(path)}
                  title="Make it the default repository"
                  data-testid="project-member-make-primary"
                >
                  <Star size={12} strokeWidth={1.5} aria-hidden />
                  Make default
                </button>
              ))}
          </div>
        );
      })}
    </div>
  );
}
