/**
 * The Projet's member repositories, a list with its own maximum height — nested, since
 * #940, in the modal's scrolling body. A repo owned by another Projet is shown disabled.
 */
export default function ProjectMembersList({
  candidates,
  checked,
  ownerElsewhere,
  onToggle,
}: {
  candidates: string[];
  checked: Set<string>;
  ownerElsewhere: Map<string, string>;
  onToggle: (path: string) => void;
}) {
  return (
    <div
      className="mb-3 max-h-48 overflow-y-auto rounded border border-line-strong bg-bg-3"
      data-testid="project-members-list"
    >
      {candidates.length === 0 && (
        <div className="px-2 py-2 text-fg-4" style={{ fontSize: "11px" }}>
          No repositories to attach.
        </div>
      )}
      {candidates.map((path) => {
        const owner = ownerElsewhere.get(path);
        const disabled = owner != null;
        return (
          <label
            key={path}
            className={`flex items-center gap-2 border-b border-line-soft px-2 py-1.5 last:border-b-0 ${
              disabled ? "opacity-50" : "cursor-pointer hover:bg-bg-4"
            }`}
            style={{ fontSize: "11px" }}
            title={owner ? `Already in project "${owner}"` : path}
            data-testid="project-member-row"
            data-path={path}
            data-disabled={disabled ? "true" : "false"}
          >
            <input
              type="checkbox"
              checked={checked.has(path)}
              disabled={disabled}
              onChange={() => onToggle(path)}
              data-testid="project-member-checkbox"
            />
            <span className="truncate text-fg">{path}</span>
            {owner && (
              <span
                className="ml-auto shrink-0 text-fg-4"
                data-testid="project-member-owner"
              >
                in {owner}
              </span>
            )}
          </label>
        );
      })}
    </div>
  );
}
