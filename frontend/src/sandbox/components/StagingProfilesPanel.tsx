import { useState } from "react";
import { Loader2, Plus, Trash2 } from "lucide-react";
import Section, { SectionExtra } from "../../settings/components/settingsSurface/Section";
import type { SettingsSection } from "../../settings/model/settingsCategories";
import { KeyValueRow } from "../../shared/ui/key-value-row";
import { Button, EmptyState, IconButton, TextInput } from "../../shared/ui/kit";
import FsExplorerModal from "../../workspace/components/FsExplorerModal";
import { relativiseToHome } from "../model/paths";
import type { SandboxProfileEntry } from "../model/sandbox";
import { useStagingProfiles } from "../store/useStagingProfiles";
import DeleteProfileDialog from "./DeleteProfileDialog";
import ProfileEntries from "./ProfileEntries";
import ProfileEnvEditor from "./ProfileEnvEditor";
import ProfileImageEditor from "./ProfileImageEditor";

interface PanelProps {
  /** Settings › Sandbox & worktrees › Staging profiles. */
  section: SettingsSection;
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
 * - it never batches. Each toggle / add / remove is its own `PUT` — profiles are their own
 *   REST resource, not part of the grouped `PUT /settings`.
 */
export default function StagingProfilesPanel({ section, home, onChanged }: PanelProps) {
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
    <Section
      section={section}
      tools={
        <IconButton
          icon={Plus}
          label="New staging profile"
          size="sm"
          onClick={() => setCreating(true)}
          data-testid="staging-profile-new"
        />
      }
    >
      <SectionExtra testId="staging-profiles-panel">
        {/* #691: inline in Settings › Sandbox & worktrees › Staging profiles. Natural
            height, no inner scroll: the category page is the one scroll container. */}
        {profiles == null ? (
          <EmptyState icon={Loader2} spin layout="row">
            <span data-testid="staging-profiles-loading">Loading</span>
          </EmptyState>
        ) : (
          <div role="listbox" aria-label="Staging profiles">
            {profiles.map((p) => (
              <div key={p.name} className="set-rrow" aria-selected={p.name === selected} role="option">
                <button
                  type="button"
                  onClick={() => setSelected(p.name)}
                  data-testid={`staging-profile-row-${p.name}`}
                  className={`set-line min-w-0 flex-1 text-left ${p.name === selected ? "text-text" : "text-text-2"}`}
                >
                  <span className={`set-mono set-ellipsis ${p.name === selected ? "font-medium" : ""}`}>{p.name}</span>
                  {(p.materialised || p.virtual) && (
                    <span className="d-chip">{p.materialised ? "edited" : "built-in"}</span>
                  )}
                </button>
                {/* Only a MATERIALISED row can be deleted; an unedited built-in default has
                    no row to delete (the daemon 404s), so the button is not offered. */}
                {p.materialised && (
                  <IconButton
                    icon={Trash2}
                    label={`Delete ${p.name}`}
                    size="sm"
                    className="set-hover"
                    onClick={() => void askDelete(p.name)}
                    data-testid={`staging-profile-delete-${p.name}`}
                  />
                )}
              </div>
            ))}
          </div>
        )}

        {creating && (
          <div className="set-line pt-1">
            <TextInput
              autoFocus
              mono
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void create();
                if (e.key === "Escape") {
                  e.preventDefault();
                  setCreating(false);
                }
              }}
              placeholder="full-no-mcp"
              title="Lowercase letters, digits and -; starts as a copy of the built-in default"
              aria-label="New staging profile name"
              data-testid="staging-profile-new-name"
            />
            <Button
              size="sm"
              tone="primary"
              onClick={() => void create()}
              disabled={busy || newName.trim().length === 0}
              data-testid="staging-profile-create"
            >
              Create
            </Button>
          </div>
        )}

        {error && (
          <div className="set-row__note set-row__note--failed" role="alert" data-testid="staging-profiles-error">
            <span className="set-ellipsis" title={error}>
              {error}
            </span>
          </div>
        )}

        {/* ── the selected profile's entries ── */}
        {current && (
          <div className="mt-3 flex flex-col gap-3 border-t border-line pt-3">
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
              <KeyValueRow
                label="Inactive"
                title="Remembered, but not in this version's default"
                testId="staging-profile-inactive-disabled"
              >
                <span className="set-mono set-ellipsis">{current.inactive_disabled.join(", ")}</span>
              </KeyValueRow>
            )}
            {current.redundant_extras.length > 0 && (
              <KeyValueRow
                label="Already in the default"
                title="Kept in case the default drops them"
                testId="staging-profile-redundant-extras"
              >
                <span className="set-mono set-ellipsis">{current.redundant_extras.join(", ")}</span>
              </KeyValueRow>
            )}
          </div>
        )}
      </SectionExtra>

      {/* No footer: every edit above already went to the daemon (the section is saved at
          once), and the form's Save never batches anything from here. */}

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
    </Section>
  );
}
