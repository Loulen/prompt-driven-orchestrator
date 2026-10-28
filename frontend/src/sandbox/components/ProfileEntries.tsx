import { Search, X } from "lucide-react";
import type { SandboxProfile, SandboxProfileEntry } from "../model/sandbox";

/**
 * What the selected profile stages in the Run's home: the read-only floor, the
 * checkable entries of the built-in default, and the `$HOME` extras.
 */
export default function ProfileEntries({
  profile,
  busy,
  onToggleDefault,
  onRemoveExtra,
  onPick,
}: {
  profile: SandboxProfile;
  busy: boolean;
  onToggleDefault: (entry: SandboxProfileEntry) => void;
  onRemoveExtra: (path: string) => void;
  /** Open the explorer to add an extra, a file or a folder. */
  onPick: (mode: "file" | "dir") => void;
}) {
  return (
    <>
    {/* The FLOOR, read-only. Without this block a `minimal` profile's screen looks
        broken and the user wrongly concludes the container starts with no
        credentials. These are guarantees, satisfied by a host copy OR by a
        fallback synthesis — never selectable, and refused as extras too. */}
    <div className="flex flex-col gap-1" data-testid="staging-profile-floor">
      <span className="text-text-3" style={{ fontSize: "10.5px" }}>
        Floor (always applied, not editable)
      </span>
      <ul className="flex flex-col gap-0.5">
        {profile.floor.map((g) => (
          <li key={g.id} className="flex items-baseline gap-2 text-text-3" style={{ fontSize: "10.5px" }}>
            <span>· {g.label}</span>
            {g.path && <span className="truncate font-mono">{g.path}</span>}
          </li>
        ))}
      </ul>
    </div>

    {/* From the default: checkable. `minimal` legitimately has none — say so
        explicitly, or the empty area reads as a loading failure. */}
    <div className="flex flex-col gap-1.5">
      <span className="text-text-3" style={{ fontSize: "10.5px" }}>
        From the default
      </span>
      {profile.entries.filter((e) => e.from_default).length === 0 ? (
        <span
          className="text-text-3"
          style={{ fontSize: "10.5px" }}
          data-testid="staging-profile-no-default-entries"
        >
          No entries — <span className="font-mono">{profile.name}</span> <em>is</em> the
          floor.
        </span>
      ) : (
        profile.entries
          .filter((e) => e.from_default)
          .map((e) => (
            <label
              key={e.path}
              className="flex cursor-pointer items-start gap-2"
              data-testid={`staging-entry-${e.path}`}
            >
              <input
                type="checkbox"
                checked={e.enabled}
                disabled={busy}
                onChange={() => onToggleDefault(e)}
                className="mt-0.5 shrink-0 accent-primary"
              />
              <span className="flex min-w-0 flex-col">
                <span className="flex items-baseline gap-2">
                  <span className="truncate font-mono text-text" style={{ fontSize: "11.5px" }}>
                    {e.path}
                  </span>
                  <span className="shrink-0 text-text-3" style={{ fontSize: "10px" }}>
                    {e.kind}
                  </span>
                  {e.exists === false && (
                    <span className="shrink-0 text-text-3" style={{ fontSize: "10px" }}>
                      not on this host
                    </span>
                  )}
                </span>
                {e.note && (
                  <span className="text-text-3" style={{ fontSize: "10px" }}>
                    {e.note}
                  </span>
                )}
              </span>
            </label>
          ))
      )}
    </div>

    {/* Extras: `$HOME` exceptions, added through the generic explorer. */}
    <div className="flex flex-col gap-1.5">
      <span className="text-text-3" style={{ fontSize: "10.5px" }}>
        Extras
      </span>
      {profile.entries
        .filter((e) => !e.from_default)
        .map((e) => (
          <div
            key={e.path}
            className="flex items-center gap-2"
            data-testid={`staging-extra-${e.path}`}
          >
            <span className="truncate font-mono text-text" style={{ fontSize: "11.5px" }}>
              ~/{e.path}
            </span>
            <span className="shrink-0 text-text-3" style={{ fontSize: "10px" }}>
              {e.kind}
            </span>
            {e.sensitive && (
              <span
                className="shrink-0 text-st-await"
                style={{ fontSize: "10px" }}
                data-testid={`staging-extra-sensitive-${e.path}`}
              >
                secrets
              </span>
            )}
            {e.exists === false && (
              <span className="shrink-0 text-text-3" style={{ fontSize: "10px" }}>
                missing
              </span>
            )}
            <button
              type="button"
              onClick={() => onRemoveExtra(e.path)}
              disabled={busy}
              aria-label={`Remove ${e.path}`}
              data-testid={`staging-extra-remove-${e.path}`}
              className="ml-auto shrink-0 rounded p-0.5 text-text-3 transition-colors hover:bg-active hover:text-text-2 disabled:opacity-40"
            >
              <X size={12} />
            </button>
          </div>
        ))}
      <div className="flex gap-2 pt-0.5">
        {/* `FsExplorerModal` is consumed UNCHANGED (#431): two buttons, two mounts,
            `mode="file"` and `mode="dir"`, both `showHidden` — the entries that
            matter here are dotfiles. A third `mode` value or a `pickDirs` flag
            would have to rewrite `handleRow`, the exact line whose frozen test
            keeps `RepoCombobox` and the repo-explorer e2e independent. */}
        <button
          type="button"
          onClick={() => onPick("file")}
          data-testid="staging-extra-add-file"
          className="rounded-md border border-line-strong bg-surface-2 px-2.5 py-1 text-text-2 transition-colors hover:border-primary"
          style={{ fontSize: "11px" }}
        >
          <span className="inline-flex items-center gap-1.5">
            <Search size={11} /> Add file…
          </span>
        </button>
        <button
          type="button"
          onClick={() => onPick("dir")}
          data-testid="staging-extra-add-folder"
          className="rounded-md border border-line-strong bg-surface-2 px-2.5 py-1 text-text-2 transition-colors hover:border-primary"
          style={{ fontSize: "11px" }}
        >
          <span className="inline-flex items-center gap-1.5">
            <Search size={11} /> Add folder…
          </span>
        </button>
      </div>
      <div className="text-text-3" style={{ fontSize: "10.5px" }}>
        An extra is <strong>copied</strong> into the run's staging and mounted
        read-write in the container. The host file is never mutated — and never
        bind-mounted, so nothing the container writes can reach it.
      </div>
      {profile.entries.some((e) => !e.from_default && e.sensitive) && (
        <div
          className="text-st-await"
          style={{ fontSize: "10.5px" }}
          data-testid="staging-profile-sensitive-warning"
        >
          This profile stages secrets ({profile.sensitive_prefixes.join(", ")}). They
          are copied, not shared — but the container can read them.
        </div>
      )}
    </div>
    </>
  );
}
