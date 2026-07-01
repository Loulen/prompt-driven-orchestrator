import { useState } from "react";
import { X } from "lucide-react";
import { useSettings } from "../hooks/useSettings";
import type { InstanceSettings, SettingField, UpdateSettingsRequest } from "../types";
import SessionCounter from "./SessionCounter";

interface Props {
  open: boolean;
  onClose: () => void;
  /**
   * Live NodeRun-session count, so the cap field can preview how the pending cap
   * relates to the sessions running right now (reuses `SessionCounter`).
   */
  liveSessions?: number;
  /**
   * Called after a successful save so the caller can refresh derived UI — e.g.
   * `refreshSessions()` to update the status-bar cap live.
   */
  onSaved?: () => void;
}

/**
 * Instance-wide settings page (#129, ADR-0015): a gear-opened modal exposing the
 * three daemon-wide knobs — session cap, tmux reaper TTL, Trigger guard timeout.
 *
 * Precedence is `stored → env → default`: a stored value wins, so this page is
 * authoritative. It discloses a shadowed env var (D6) rather than ignoring it,
 * and validates fail-fast (D7) client-side, with the daemon's `400` surfaced too.
 *
 * The outer component owns open/close and the fetch; the inner [`SettingsForm`]
 * mounts only once settings load and seeds its edit-state synchronously from
 * props — so the inputs show the effective values on first render (no async
 * seeding race).
 */
export default function SettingsModal({ open, onClose, liveSessions = 0, onSaved }: Props) {
  const { settings, save } = useSettings(open);

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      onClick={onClose}
    >
      <div
        className="w-[460px] max-h-[85vh] flex flex-col rounded-lg border border-line bg-bg-4 shadow-xl"
        onClick={(e) => e.stopPropagation()}
        data-testid="settings-modal"
      >
        <div className="flex items-center justify-between border-b border-line px-4 py-3">
          <h2 className="font-semibold text-fg" style={{ fontSize: "13.5px" }}>
            Instance settings
          </h2>
          <button
            onClick={onClose}
            aria-label="Close settings"
            className="grid h-6 w-6 place-items-center rounded text-fg-3 transition-colors hover:bg-bg-5 hover:text-fg"
          >
            <X size={14} />
          </button>
        </div>

        {settings ? (
          <SettingsForm
            // Re-seed if the loaded config changes (refetch / restart).
            key={settings.updated_at}
            settings={settings}
            liveSessions={liveSessions}
            save={save}
            onClose={onClose}
            onSaved={onSaved}
          />
        ) : (
          <div
            className="px-4 py-6 text-fg-4"
            style={{ fontSize: "12px" }}
            data-testid="settings-loading"
          >
            Loading…
          </div>
        )}
      </div>
    </div>
  );
}

/** Advisory ceiling: caps above this enter the tmux-server-collapse zone
 *  (#77/#78). Not a hard limit (Sharp tool — ADR-0001), just an amber warning. */
const CAP_ADVISORY = 20;

interface FormProps {
  settings: InstanceSettings;
  liveSessions: number;
  save: (patch: UpdateSettingsRequest) => Promise<InstanceSettings>;
  onClose: () => void;
  onSaved?: () => void;
}

function SettingsForm({ settings, liveSessions, save, onClose, onSaved }: FormProps) {
  // Seed synchronously from the loaded effective values — correct on first render.
  const [capStr, setCapStr] = useState(() => String(settings.session_cap.effective));
  const [ttlStr, setTtlStr] = useState(() => String(settings.reaper_ttl_secs.effective));
  const [guardStr, setGuardStr] = useState(() => String(settings.guard_timeout_secs.effective));
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSave = async () => {
    if (submitting) return;
    setError(null);

    const patch: UpdateSettingsRequest = {};

    const capT = capStr.trim();
    if (capT !== "") {
      const cap = Number(capT);
      if (!Number.isInteger(cap) || cap < 1) {
        setError("Session cap must be a whole number ≥ 1.");
        return;
      }
      if (cap !== settings.session_cap.effective) patch.session_cap = cap;
    }

    const ttlT = ttlStr.trim();
    if (ttlT !== "") {
      const ttl = Number(ttlT);
      if (!Number.isInteger(ttl) || ttl < 1) {
        setError("Reaper TTL must be a whole number ≥ 1 second.");
        return;
      }
      if (ttl !== settings.reaper_ttl_secs.effective) patch.reaper_ttl_secs = ttl;
    }

    const guardT = guardStr.trim();
    if (guardT !== "") {
      const guard = Number(guardT);
      if (!Number.isInteger(guard) || guard < 1 || guard > 600) {
        setError("Guard timeout must be a whole number between 1 and 600 seconds.");
        return;
      }
      if (guard !== settings.guard_timeout_secs.effective) patch.guard_timeout_secs = guard;
    }

    // Nothing changed → close without a round-trip.
    if (Object.keys(patch).length === 0) {
      onClose();
      return;
    }

    setSubmitting(true);
    try {
      await save(patch);
      onSaved?.();
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSubmitting(false);
    }
  };

  const capPreview = Number(capStr.trim());
  const capForPreview =
    Number.isInteger(capPreview) && capPreview >= 1 ? capPreview : settings.session_cap.effective;

  return (
    <>
      <div className="flex flex-col gap-4 overflow-y-auto px-4 py-4">
        {/* Session cap */}
        <SettingRow
          id="session-cap"
          label="Concurrent session cap"
          help="Max live NodeRun sessions daemon-wide. Kept below the tmux-collapse zone (#77/#78)."
          value={capStr}
          onChange={setCapStr}
          field={settings.session_cap}
          envVar="PDO_SESSION_CAP"
          unit=""
        >
          <div className="flex items-center gap-2 pt-1">
            <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
              Preview:
            </span>
            <SessionCounter live={liveSessions} cap={capForPreview} />
          </div>
          {Number.isInteger(capPreview) && capPreview > CAP_ADVISORY && (
            <div
              className="text-st-await"
              style={{ fontSize: "10.5px" }}
              data-testid="settings-cap-advisory"
            >
              Caps above {CAP_ADVISORY} risk collapsing the tmux server (#77/#78).
            </div>
          )}
        </SettingRow>

        {/* Reaper TTL */}
        <SettingRow
          id="reaper-ttl"
          label="Reaper TTL (seconds)"
          help="Seconds after a node completes before its idle tmux session is reaped. Sweep runs every 60 s, so values below ~60 s add little."
          value={ttlStr}
          onChange={setTtlStr}
          field={settings.reaper_ttl_secs}
          envVar="PDO_REAPER_TTL_SECS"
          unit=" s"
        />

        {/* Guard timeout */}
        <SettingRow
          id="guard-timeout"
          label="Trigger guard timeout (seconds)"
          help="Hard timeout for a Trigger guard command. 1–600 s."
          value={guardStr}
          onChange={setGuardStr}
          field={settings.guard_timeout_secs}
          envVar="PDO_GUARD_TIMEOUT_MS"
          unit=" ms"
          envIsMs
        />

        {error && (
          <div
            className="rounded-md border border-st-failed/30 bg-st-failed-bg px-3 py-2 text-st-failed"
            style={{ fontSize: "11.5px" }}
            data-testid="settings-error"
          >
            {error}
          </div>
        )}
      </div>

      <div className="flex items-center justify-end gap-2 border-t border-line px-4 py-3">
        <button
          onClick={onClose}
          className="rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 transition-colors hover:bg-bg-4"
          style={{ fontSize: "11.5px" }}
        >
          Cancel
        </button>
        <button
          onClick={handleSave}
          disabled={submitting}
          className="rounded-md bg-acc px-3 py-1.5 font-medium text-[#04140d] transition-colors hover:bg-acc-dim disabled:opacity-40"
          style={{ fontSize: "11.5px" }}
          data-testid="settings-save"
        >
          {submitting ? "Saving…" : "Save"}
        </button>
      </div>
    </>
  );
}

interface SettingRowProps {
  id: string;
  label: string;
  help: string;
  value: string;
  onChange: (v: string) => void;
  field: SettingField;
  envVar: string;
  /** Unit suffix appended to the env value in the disclosure (e.g. " s", " ms"). */
  unit: string;
  /** The env value is in milliseconds while the knob is in seconds (guard). */
  envIsMs?: boolean;
  children?: React.ReactNode;
}

function SettingRow({
  id,
  label,
  help,
  value,
  onChange,
  field,
  envVar,
  unit,
  envIsMs,
  children,
}: SettingRowProps) {
  return (
    <div className="flex flex-col gap-1.5">
      <label htmlFor={`setting-${id}`} className="font-medium text-fg-2" style={{ fontSize: "11.5px" }}>
        {label}
      </label>
      <input
        id={`setting-${id}`}
        data-testid={`setting-${id}`}
        type="number"
        min={1}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="w-full rounded-md border border-line-strong bg-bg-3 px-2.5 py-1.5 font-mono text-fg placeholder:text-fg-4 focus:border-acc focus:outline-none"
        style={{ fontSize: "12px" }}
      />
      <div className="text-fg-4" style={{ fontSize: "10.5px" }}>
        {help}
      </div>
      <div
        className="text-fg-3"
        style={{ fontSize: "10.5px" }}
        data-testid={`setting-source-${id}`}
      >
        {sourceNote(field, envVar, unit, envIsMs)}
      </div>
      {children}
    </div>
  );
}

/** Human-readable disclosure of which tier a knob's value comes from (D6). */
function sourceNote(
  field: SettingField,
  envVar: string,
  unit: string,
  envIsMs?: boolean,
): string {
  const envDisplay = field.env != null ? `${envVar}=${field.env}${unit}` : null;
  if (field.source === "stored") {
    // Stored wins — but if an env var is also set, disclose that it is shadowed.
    return envDisplay
      ? `Source: stored value (wins). Env ${envDisplay} is set but overridden.`
      : `Source: stored value (overrides env and default).`;
  }
  if (field.source === "env") {
    const note = envDisplay ? `Source: env ${envDisplay}.` : `Source: env ${envVar}.`;
    return envIsMs ? `${note} (Saving stores it in seconds.)` : note;
  }
  return `Source: built-in default (${field.default}${unit === " ms" ? " s" : unit}).`;
}
