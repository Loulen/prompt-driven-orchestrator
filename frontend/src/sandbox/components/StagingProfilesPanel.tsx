import { useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import FsExplorerModal from "../../workspace/components/FsExplorerModal";
import { relativiseToHome } from "../model/paths";
import type { SandboxProfileEntry } from "../model/sandbox";
import { useStagingProfiles } from "../store/useStagingProfiles";
import DeleteProfileDialog from "./DeleteProfileDialog";
import ProfileEntries from "./ProfileEntries";
import ProfileEnvEditor from "./ProfileEnvEditor";
import ProfileImageEditor from "./ProfileImageEditor";

interface PanelProps {
  /** Host `$HOME`, from `GET /settings`. `null` disables the pickers' relativisation. */
  home: string | null;
  /** Called after every successful write so the parent can refetch `GET /settings`. */
  onChanged: () => void;
}

/**
 * The staging-profile editor: a list of profiles on top, the selected one's entries below.
 *
 * Two things it deliberately does NOT do:
 * - it never computes a warning or a size itself. The disk-cost note, the `sensitive` flag
 *   and the floor block all arrive from the daemon (#373 discipline: a client-side tag
 *   re-opens exactly the drift that cost us before). A real recursive walk of every
 *   `node_modules` under `plugins/` is seconds of IO anyway.
 * - it never batches. Each toggle / add / remove is its own `PUT`, which is why the footer
 *   says **Done** and not Save — profiles are their own REST resource, not part of the
 *   grouped `PUT /settings`.
 */
export default function StagingProfilesPanel({ home, onChanged }: PanelProps) {
  const {
    profiles,
    selected,
    setSelected,
    current,
    error,
    setError,
    busy,
    writeCurrent,
    create: createProfile,
    confirmDelete,
    askDelete,
    cancelDelete,
    doDelete,
  } = useStagingProfiles(onChanged);
  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState("");
  const [pickerMode, setPickerMode] = useState<"file" | "dir" | null>(null);

  /**
   * Toggle one entry. Only a BUILT-IN DEFAULT entry is toggleable: unchecking adds it to
   * `disabled`, re-checking removes it. An extra is removed by dropping it from `extras`,
   * never by "unchecking" it — conflating the two would make the stored diff ambiguous
   * (and the daemon rejects a `disabled` that is not a default entry).
   */
  const toggleDefault = (entry: SandboxProfileEntry) => {
    if (!current || !entry.from_default) return;
    writeCurrent({
      disabled: entry.enabled
        ? [...current.disabled, entry.path]
        : current.disabled.filter((d) => d !== entry.path),
    });
  };

  const removeExtra = (path: string) => {
    if (!current) return;
    writeCurrent({ extras: current.extras.filter((e) => e !== path) });
  };

  const addExtra = (abs: string) => {
    if (!current) return;
    const rel = relativiseToHome(abs, home);
    if (!rel) {
      setError(
        `\`${abs}\` must live under your home directory${home ? ` (${home})` : ""} — an entry is a path relative to \`$HOME\`.`,
      );
      return;
    }
    if (current.extras.includes(rel)) return;
    writeCurrent({ extras: [...current.extras, rel] });
  };

  const create = async () => {
    if (await createProfile(newName.trim())) {
      setNewName("");
      setCreating(false);
    }
  };

  return (
    <>
      <div
        // #691: mounted inline in Settings › Sandbox & worktrees › Staging profiles. Natural
        // height, no inner scroll: the category page is the one scroll container.
        className="flex flex-col gap-4 px-4 py-4"
        data-testid="staging-profiles-panel"
      >
        {/* ── the profile list ── */}
        <div className="flex flex-col gap-1">
          {profiles == null ? (
            <div className="text-text-3" style={{ fontSize: "12px" }} data-testid="staging-profiles-loading">
              Loading…
            </div>
          ) : (
            profiles.map((p) => (
              <div
                key={p.name}
                className={`flex items-center gap-2 rounded-md border px-2.5 py-1.5 transition-colors ${
                  p.name === selected
                    ? "border-primary bg-surface-2"
                    : "border-line-strong bg-surface-2 hover:border-line"
                }`}
              >
                <button
                  type="button"
                  onClick={() => setSelected(p.name)}
                  data-testid={`staging-profile-row-${p.name}`}
                  className="flex min-w-0 flex-1 items-center gap-2 text-left"
                >
                  <span className="truncate font-mono text-text" style={{ fontSize: "12px" }}>
                    {p.name}
                  </span>
                  <span className="shrink-0 text-text-3" style={{ fontSize: "10px" }}>
                    {p.materialised ? "edited" : p.virtual ? "built-in" : ""}
                  </span>
                </button>
                {/* Only a MATERIALISED row can be deleted; an unedited built-in default has
                    no row to delete (the daemon 404s), so the button is not offered. */}
                {p.materialised && (
                  <button
                    type="button"
                    onClick={() => void askDelete(p.name)}
                    aria-label={`Delete ${p.name}`}
                    data-testid={`staging-profile-delete-${p.name}`}
                    className="shrink-0 rounded p-1 text-text-3 transition-colors hover:bg-active hover:text-st-failed"
                  >
                    <Trash2 size={13} />
                  </button>
                )}
              </div>
            ))
          )}

          {creating ? (
            <div className="mt-1 flex items-center gap-2">
              <input
                autoFocus
                value={newName}
                onChange={(e) => setNewName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void create();
                  if (e.key === "Escape") setCreating(false);
                }}
                placeholder="full-no-mcp"
                data-testid="staging-profile-new-name"
                className="min-w-0 flex-1 rounded-md border border-line-strong bg-surface-2 px-2.5 py-1.5 font-mono text-text placeholder:text-text-3 focus:border-primary focus:outline-none"
                style={{ fontSize: "12px" }}
              />
              <button
                type="button"
                onClick={() => void create()}
                disabled={busy || newName.trim().length === 0}
                data-testid="staging-profile-create"
                className="shrink-0 rounded-md bg-primary px-2.5 py-1.5 font-medium text-primary-ink transition-colors hover:bg-primary-hi disabled:opacity-40"
                style={{ fontSize: "11px" }}
              >
                Create
              </button>
            </div>
          ) : (
            <button
              type="button"
              onClick={() => setCreating(true)}
              data-testid="staging-profile-new"
              className="mt-1 flex items-center gap-1.5 self-start rounded-md px-1 py-1 text-text-3 transition-colors hover:text-text"
              style={{ fontSize: "11px" }}
            >
              <Plus size={12} /> New profile
            </button>
          )}
          <div className="text-text-3" style={{ fontSize: "10.5px" }}>
            Lowercase letters, digits and <span className="font-mono">-</span>. A new profile
            starts as a copy of the built-in default; unchecking an entry stores the
            difference, so a later PDO release that adds an entry still shows up here.
          </div>
        </div>

        {error && (
          <div
            className="rounded-md border border-st-failed/30 bg-st-failed/14 px-3 py-2 text-st-failed"
            style={{ fontSize: "11.5px" }}
            data-testid="staging-profiles-error"
          >
            {error}
          </div>
        )}

        {/* ── the selected profile's entries ── */}
        {current && (
          <div className="flex flex-col gap-3 border-t border-line pt-4">
            <h3 className="font-medium text-text-2" style={{ fontSize: "12px" }}>
              <span className="font-mono">{current.name}</span> — entries
            </h3>

            <ProfileEntries
              profile={current}
              busy={busy}
              onToggleDefault={toggleDefault}
              onRemoveExtra={removeExtra}
              onPick={setPickerMode}
            />

            <ProfileEnvEditor
              profile={current}
              busy={busy}
              onWrite={(env) => writeCurrent({ env })}
              onRefuse={setError}
            />

            {/* Image (#467, ADR-0031 §9): WHICH container the Run gets, as opposed to what
                lands in its home. `key` on the profile name so switching profiles resets the
                draft — the reset is the mount, not an effect (the #385 lesson). */}
            <ProfileImageEditor
              key={current.name}
              profile={current}
              busy={busy}
              onWrite={(image) => writeCurrent({ image })}
            />

            {/* Signalled no-ops (ADR-0031 §2). Not errors: the default may LOSE an entry
                tomorrow, and unchecking one a future release will add must be remembered. */}
            {current.inactive_disabled.length > 0 && (
              <div
                className="text-text-3"
                style={{ fontSize: "10.5px" }}
                data-testid="staging-profile-inactive-disabled"
              >
                Remembered but inactive (not in this version's default):{" "}
                <span className="font-mono">{current.inactive_disabled.join(", ")}</span>
              </div>
            )}
            {current.redundant_extras.length > 0 && (
              <div
                className="text-text-3"
                style={{ fontSize: "10.5px" }}
                data-testid="staging-profile-redundant-extras"
              >
                Already in the default (kept in case it is removed later):{" "}
                <span className="font-mono">{current.redundant_extras.join(", ")}</span>
              </div>
            )}
          </div>
        )}
      </div>

      {/* No footer: every edit above already went to the daemon (the section header says
          `saves as you go`), and the form's Save below never batches anything from here. */}

      {pickerMode && (
        <FsExplorerModal
          mode={pickerMode}
          showHidden
          title={pickerMode === "file" ? "Add a file to stage" : "Add a folder to stage"}
          confirmLabel="Add to the profile"
          startPath={home ?? undefined}
          onPick={addExtra}
          onClose={() => setPickerMode(null)}
        />
      )}

      {confirmDelete && (
        <DeleteProfileDialog
          referents={confirmDelete}
          busy={busy}
          onCancel={cancelDelete}
          onConfirm={() => void doDelete(confirmDelete.name)}
        />
      )}
    </>
  );
}
