import { FilePlus, FolderPlus, ShieldAlert, X } from "lucide-react";
import { Button, Checkbox, IconButton } from "../../shared/ui/kit";
import type { SandboxProfile, SandboxProfileEntry } from "../model/sandbox";

/**
 * What the selected profile stages in the Run's home: the read-only floor, the
 * checkable entries of the built-in default, and the `$HOME` extras — each copied into
 * the Run's staging, never bind-mounted, so the host file is never written.
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
  const fromDefault = profile.entries.filter((e) => e.from_default);
  const extras = profile.entries.filter((e) => !e.from_default);
  return (
    <>
      {/* The FLOOR, read-only. Without it a `minimal` profile's screen looks broken and
          the user wrongly concludes the container starts with no credentials. */}
      <div className="set-group" data-testid="staging-profile-floor">
        <span className="d-caps set-group__head" title="Always applied, not editable">
          Floor
        </span>
        {profile.floor.map((g) => (
          <div key={g.id} className="set-rrow">
            <span className="set-ellipsis">{g.label}</span>
            {g.path && <span className="set-mono set-faint set-ellipsis">{g.path}</span>}
          </div>
        ))}
      </div>

      {/* From the default: checkable. `minimal` legitimately has none — say so, or the
          empty area reads as a loading failure. */}
      <div className="set-group">
        <span className="d-caps set-group__head">From the default</span>
        {fromDefault.length === 0 ? (
          <span className="set-faint text-sm" data-testid="staging-profile-no-default-entries">
            None: <span className="set-mono">{profile.name}</span> is the floor
          </span>
        ) : (
          fromDefault.map((e) => (
            <div key={e.path} className="set-rrow" data-testid={`staging-entry-${e.path}`} title={e.note ?? undefined}>
              <Checkbox checked={e.enabled} disabled={busy} onChange={() => onToggleDefault(e)} label={e.path}>
                <span className="set-mono set-ellipsis">{e.path}</span>
              </Checkbox>
              <span className="set-faint text-xs">{e.kind}</span>
              {e.exists === false && <span className="d-chip">not on this host</span>}
            </div>
          ))
        )}
      </div>

      {/* Extras: `$HOME` exceptions, added through the generic explorer. */}
      <div className="set-group">
        <span className="d-caps set-group__head" title="Copied into the Run's staging, never bind-mounted">
          Extras
          <span className="d-grow" />
          {/* `FsExplorerModal` is consumed UNCHANGED (#431): two buttons, two mounts,
              `mode="file"` and `mode="dir"`, both `showHidden`. */}
          <Button size="sm" tone="ghost" icon={FilePlus} onClick={() => onPick("file")} data-testid="staging-extra-add-file">
            File
          </Button>
          <Button size="sm" tone="ghost" icon={FolderPlus} onClick={() => onPick("dir")} data-testid="staging-extra-add-folder">
            Folder
          </Button>
        </span>
        {extras.map((e) => (
          <div key={e.path} className="set-rrow" data-testid={`staging-extra-${e.path}`}>
            <span className="set-mono set-ellipsis set-rrow__path">~/{e.path}</span>
            <span className="set-faint text-xs">{e.kind}</span>
            {e.sensitive && (
              <span className="d-chip text-st-await" data-testid={`staging-extra-sensitive-${e.path}`}>
                secrets
              </span>
            )}
            {e.exists === false && <span className="d-chip">missing</span>}
            <IconButton
              icon={X}
              label={`Remove ${e.path}`}
              size="sm"
              onClick={() => onRemoveExtra(e.path)}
              disabled={busy}
              data-testid={`staging-extra-remove-${e.path}`}
            />
          </div>
        ))}
        {extras.some((e) => e.sensitive) && (
          <div
            className="set-warn"
            title={`Copied, not shared, but the container can read them (${profile.sensitive_prefixes.join(", ")})`}
            data-testid="staging-profile-sensitive-warning"
          >
            <ShieldAlert size={13} strokeWidth={1.5} aria-hidden />
            <span>Stages secrets</span>
            <span className="set-faint set-mono set-ellipsis">{profile.sensitive_prefixes.join(", ")}</span>
          </div>
        )}
      </div>
    </>
  );
}
