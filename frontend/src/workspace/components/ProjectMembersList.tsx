/**
 * The Projet's member repositories, a list with its own maximum height — nested, since
 * #940, in the modal's scrolling body. A repository may be a member of several Projets
 * (ADR-0087): the other Projets holding it are named, never in the way. The default
 * primary repository (the one New Run preselects) is chosen among the checked ones.
 */
export default function ProjectMembersList({
  candidates,
  checked,
  alsoIn,
  primary,
  onToggle,
  onPrimary,
}: {
  candidates: string[];
  checked: Set<string>;
  /** path → the names of the other Projets holding it. */
  alsoIn: Map<string, string[]>;
  /** The default primary repository, among the checked ones. */
  primary: string | null;
  onToggle: (path: string) => void;
  onPrimary: (path: string) => void;
}) {
  return (
    <div
      className="mb-3 max-h-48 overflow-y-auto rounded border border-line-strong bg-surface-2"
      data-testid="project-members-list"
    >
      {candidates.length === 0 && (
        <div className="px-2 py-2 text-text-3" style={{ fontSize: "11px" }}>
          No repositories to attach.
        </div>
      )}
      {candidates.map((path) => {
        const others = alsoIn.get(path) ?? [];
        const member = checked.has(path);
        const isPrimary = member && primary === path;
        return (
          <div
            key={path}
            className="flex items-center gap-2 border-b border-line-soft px-2 py-1.5 last:border-b-0 hover:bg-hover"
            style={{ fontSize: "11px" }}
            title={others.length > 0 ? `Also in ${others.join(", ")}` : path}
            data-testid="project-member-row"
            data-path={path}
          >
            <label className="flex min-w-0 flex-1 cursor-pointer items-center gap-2">
              <input
                type="checkbox"
                checked={member}
                onChange={() => onToggle(path)}
                data-testid="project-member-checkbox"
              />
              <span className="truncate text-text">{path}</span>
            </label>
            {others.length > 0 && (
              <span className="shrink-0 text-text-3" data-testid="project-member-also-in">
                also in {others.join(", ")}
              </span>
            )}
            {member &&
              (isPrimary ? (
                <span
                  className="shrink-0 rounded bg-primary/15 px-1.5 py-0.5 font-mono text-primary"
                  style={{ fontSize: "9.5px" }}
                  data-testid="project-member-primary"
                >
                  default
                </span>
              ) : (
                <button
                  type="button"
                  className="shrink-0 rounded px-1.5 py-0.5 text-text-3 transition-colors hover:bg-active hover:text-text-2"
                  style={{ fontSize: "10px" }}
                  onClick={() => onPrimary(path)}
                  data-testid="project-member-make-primary"
                >
                  Make default
                </button>
              ))}
          </div>
        );
      })}
    </div>
  );
}
