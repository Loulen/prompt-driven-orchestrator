import { useCallback, useEffect, useState } from "react";
import { ChevronLeft, Plus, Search, Trash2, X } from "lucide-react";
import FsExplorerModal from "./FsExplorerModal";
import { useSettings } from "../hooks/useSettings";
import { useEditStore } from "../stores/editStore";
import {
  deleteSandboxProfile,
  fetchSandboxProfileReferents,
  fetchSandboxProfiles,
  saveSandboxProfile,
} from "../api";
import type {
  BoolSettingField,
  EnumSettingFieldWithReason,
  InstanceSettings,
  SandboxProfile,
  SandboxProfileEntry,
  SandboxProfileImage,
  SandboxProfileReferents,
  SettingField,
  StringSettingField,
  UpdateSettingsRequest,
} from "../types";
import ModelPicker from "./ModelPicker";
import SessionCounter from "./SessionCounter";
import HarnessSelect from "./HarnessSelect";
import { harnessCatalog, findHarnessOption } from "../lib/harness";

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
  const { settings, save, refresh } = useSettings(open);
  /**
   * Staging-profile editor (#432): a **drill-down panel** replacing body + footer inside
   * this same `z-50` shell, not a nested modal.
   *
   * Why not a real nested modal: `FsExplorerModal` hardcodes `z-[60]` with no prop, so a
   * modal-in-modal would force the picker to `z-[70]` (a new prop on a component #431 just
   * froze) and the delete confirmation to `z-[80]`. Staying in the shell keeps today's
   * layering exactly: shell `z-50` → picker `z-[60]` (already proven by the Dockerfile
   * picker below) → confirmation `z-[60]` rendered later in DOM order.
   */
  const [profilesOpen, setProfilesOpen] = useState(false);

  // The modal is unmounted-by-render (`if (!open) return null`), so its state SURVIVES a
  // close. Reset the drill-down here rather than in an effect on `open`: both the backdrop
  // and the ✕ funnel through this handler, and an effect would be a setState-in-effect
  // cascade for a transition we already own.
  const handleClose = () => {
    setProfilesOpen(false);
    onClose();
  };

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      onClick={handleClose}
    >
      <div
        className="w-[460px] max-h-[85vh] flex flex-col rounded-lg border border-line bg-bg-4 shadow-xl"
        onClick={(e) => e.stopPropagation()}
        data-testid="settings-modal"
      >
        <div className="flex items-center justify-between border-b border-line px-4 py-3">
          <div className="flex min-w-0 items-center gap-2">
            {profilesOpen && (
              <button
                onClick={() => setProfilesOpen(false)}
                aria-label="Back to settings"
                data-testid="staging-profiles-back"
                className="grid h-6 w-6 shrink-0 place-items-center rounded text-fg-3 transition-colors hover:bg-bg-5 hover:text-fg"
              >
                <ChevronLeft size={14} />
              </button>
            )}
            <h2 className="truncate font-semibold text-fg" style={{ fontSize: "13.5px" }}>
              {profilesOpen ? "Staging profiles" : "Instance settings"}
            </h2>
          </div>
          <button
            onClick={handleClose}
            aria-label="Close settings"
            className="grid h-6 w-6 shrink-0 place-items-center rounded text-fg-3 transition-colors hover:bg-bg-5 hover:text-fg"
          >
            <X size={14} />
          </button>
        </div>

        {/* Interface (#342): a per-client UI pref, NOT a daemon knob. It lives in
            the OUTER modal (always rendered when open), so it stays reachable
            even if `GET /settings` fails and the numeric form never mounts
            (Trap A). Hidden — not unmounted — behind the drill-down, same reason
            as `SettingsForm` below. */}
        <div className={profilesOpen ? "hidden" : undefined}>
          <InterfaceSection />
        </div>

        {settings ? (
          // HIDDEN, never unmounted, while the drill-down is open (#432): the form holds
          // UNSAVED edits (`capStr`, `model`, …) seeded on mount, and a conditional render
          // would throw them away in silence.
          <div
            className={
              profilesOpen ? "hidden" : "flex min-h-0 flex-1 flex-col"
            }
          >
            <SettingsForm
              // Re-seed if the loaded config changes (refetch / restart, or a profile write
              // that changed the name list).
              // NOT keyed on the profile list: a profile write must refresh the
              // `<select>` OPTIONS (a render-time prop) without re-seeding — and thus
              // discarding — the form's unsaved edits.
              key={settings.updated_at}
              settings={settings}
              liveSessions={liveSessions}
              save={save}
              onClose={handleClose}
              onSaved={onSaved}
              onManageProfiles={() => setProfilesOpen(true)}
            />
          </div>
        ) : (
          <div
            className="px-4 py-6 text-fg-4"
            style={{ fontSize: "12px" }}
            data-testid="settings-loading"
          >
            Loading…
          </div>
        )}

        {profilesOpen && (
          <StagingProfilesPanel
            home={settings?.home ?? null}
            onDone={() => setProfilesOpen(false)}
            // Refetch `GET /settings` so the Default-sandbox `<select>` sees the new
            // name list (and a freshly dangling `reason`) without a reopen.
            onChanged={() => {
              void refresh();
              onSaved?.();
            }}
          />
        )}
      </div>
    </div>
  );
}

/**
 * Per-client UI preferences (#342). Currently just the single-tab toggle. The
 * value persists to localStorage AT THE CHANGE via `setSingleTabMode` (Trap B) —
 * NOT batched behind the numeric form's Save button (which PUTs to the daemon).
 */
function InterfaceSection() {
  const singleTabMode = useEditStore((s) => s.singleTabMode);
  const setSingleTabMode = useEditStore((s) => s.setSingleTabMode);

  return (
    <div className="border-b border-line px-4 py-4">
      <h3 className="font-medium text-fg-2" style={{ fontSize: "12px" }}>
        Interface
      </h3>
      <button
        type="button"
        role="switch"
        aria-checked={singleTabMode}
        data-testid="setting-tabs-disabled"
        onClick={() => setSingleTabMode(!singleTabMode)}
        className="mt-3 flex w-full items-center justify-between gap-3 rounded-md border border-line-strong bg-bg-3 px-3 py-2 text-left transition-colors hover:border-acc"
      >
        <span className="flex flex-col gap-0.5">
          <span className="font-medium text-fg-2" style={{ fontSize: "11.5px" }}>
            Single-tab mode
          </span>
          <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
            Opening a pipeline or run replaces the current tab instead of stacking a
            new one. Enabling it closes the other open tabs.
          </span>
        </span>
        <span
          className={`relative h-3.5 w-6 shrink-0 rounded-full transition-colors ${
            singleTabMode ? "bg-acc" : "bg-fg-5"
          }`}
        >
          <span
            className={`absolute top-0.5 h-2.5 w-2.5 rounded-full bg-bg-1 transition-all ${
              singleTabMode ? "left-3" : "left-0.5"
            }`}
          />
        </span>
      </button>
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
  /** Open the staging-profile drill-down (#432). */
  onManageProfiles: () => void;
}

function SettingsForm({
  settings,
  liveSessions,
  save,
  onClose,
  onSaved,
  onManageProfiles,
}: FormProps) {
  // Seed synchronously from the loaded effective values — correct on first render.
  const [capStr, setCapStr] = useState(() => String(settings.session_cap.effective));
  const [ttlStr, setTtlStr] = useState(() => String(settings.reaper_ttl_secs.effective));
  const [guardStr, setGuardStr] = useState(() => String(settings.guard_timeout_secs.effective));
  // Model is `null` when unset (account default); ModelPicker speaks the same
  // `string | null` contract as the per-node inspector (#296/#324/#347).
  const [model, setModel] = useState<string | null>(() => settings.default_model.effective);
  // #550/ADR-0046: the harness axis. `defaultHarness` is `""` when unset (the
  // `claude` floor applies); the two per-harness default models are edited as
  // free text, empty = that harness's account default.
  const [defaultHarness, setDefaultHarness] = useState<string>(
    () => settings.default_harness.effective ?? "",
  );
  // #616 (correctif 1): the per-harness default model is a MAP keyed by harness
  // name, derived from the SERVED harness list — not two hard-coded `claude` /
  // `opencode` fields. Seeded from the stored map so every harness that already
  // carries a default keeps its value in the editor; a row is shown per served
  // harness. This is what fixes the correctif-1 bug: saving builds the payload from
  // the full stored map (below), so a harness with no field is never wiped.
  const [harnessModels, setHarnessModels] = useState<Record<string, string>>(
    () => ({ ...settings.default_harness_model.stored }),
  );
  // Default sandbox (#410/#432): `off` or a staging-profile name. `effective` is always
  // a present string (the `?? "off"` is belt-and-braces).
  const [defaultSandbox, setDefaultSandbox] = useState<string>(
    () => settings.default_sandbox.effective ?? "off",
  );
  // #432 phantom-profile rule, mirror of the launch dialog's: a seeded value that is not
  // in the list gets a tombstone option and blocks Save, instead of rendering blank and
  // clearing the knob on the next Save.
  const missingDefaultProfile =
    defaultSandbox !== "off" &&
    defaultSandbox !== "" &&
    !settings.sandbox_profiles.some((p) => p.name === defaultSandbox);
  // Turn-end auto-completion (#469). Seeded from `effective`, like the numeric
  // knobs: there is no "unset" affordance for a checkbox, and both directions
  // persist as a stored decision (see the note on the PUT below).
  const [autocompleteTurnEnd, setAutocompleteTurnEnd] = useState<boolean>(
    () => settings.autocomplete_turn_end.effective,
  );
  // Default Run auto-naming (#338). Seeded from `effective` like the other bool knobs;
  // both directions persist as a stored decision (see the PUT note below).
  const [defaultAutoName, setDefaultAutoName] = useState<boolean>(
    () => settings.default_auto_name.effective,
  );
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSave = async () => {
    if (submitting) return;
    setError(null);

    if (missingDefaultProfile) {
      setError(
        `No staging profile named \`${defaultSandbox}\` any more. Pick another one (or ` +
          "`off`) — a Run that falls back to a missing profile fails at launch.",
      );
      return;
    }

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

    // Model: `null` (Default) clears via the "" sentinel; a string sets it. Only
    // sent when it actually changed (avoids a needless clear/no-op PUT).
    if (model !== settings.default_model.effective) {
      patch.default_model = model ?? "";
    }
    // #550: the harness axis. `""` clears the default harness (same sentinel as
    // the model). The per-harness default model map is sent whole when either
    // known entry changed; a trimmed-empty value drops that harness's entry.
    if (defaultHarness !== (settings.default_harness.effective ?? "")) {
      patch.default_harness = defaultHarness;
    }
    // #616 (correctif 1): build the per-harness model map from the FULL edited map,
    // dropping only trimmed-empty entries — never a two-field block. The daemon
    // replaces the stored map wholesale, so sending the whole thing is what
    // PRESERVES a harness whose row wasn't touched (and one not even shown, e.g. a
    // declared-but-uninstalled harness whose stored default must survive). Sent only
    // when the effective map actually changed.
    const storedModels = settings.default_harness_model.stored;
    const nextModels: Record<string, string> = {};
    for (const [name, raw] of Object.entries(harnessModels)) {
      const v = raw.trim();
      if (v) nextModels[name] = v;
    }
    const sameMap =
      Object.keys(nextModels).length === Object.keys(storedModels).length &&
      Object.entries(nextModels).every(([k, v]) => storedModels[k] === v);
    if (!sameMap) {
      patch.default_harness_model = nextModels;
    }

    // Default sandbox mode (#410): a concrete enum variant, only sent when it changed. The
    // select never emits "" — the clear path is backend-only.
    if (defaultSandbox !== settings.default_sandbox.effective) {
      patch.default_sandbox = defaultSandbox;
    }

    // Turn-end auto-completion (#469): a plain bool, only sent when it changed.
    // `false` is a real value here, NOT a clear sentinel — the daemon persists a
    // stored `0` so unticking overrides a `PDO_AUTOCOMPLETE_TURN_END=1`.
    if (autocompleteTurnEnd !== settings.autocomplete_turn_end.effective) {
      patch.autocomplete_turn_end = autocompleteTurnEnd;
    }

    // Default Run auto-naming (#338): same discipline as the turn-end flag — a plain bool,
    // sent only when changed; `false` persists as a stored `0` so unticking overrides a
    // `PDO_DEFAULT_AUTO_NAME=1`.
    if (defaultAutoName !== settings.default_auto_name.effective) {
      patch.default_auto_name = defaultAutoName;
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

        {/* Turn-end auto-completion (#469). Labelled on what is MEASURED — an end
            of turn constated in the agent's transcript — and deliberately not on a
            duration: "no activity for N seconds" is the framing #469 removed,
            because a `docker build` is indistinguishable from a dead agent that
            way. Off by default (ADR-0012). */}
        <div className="flex flex-col gap-1.5">
          <label
            htmlFor="setting-autocomplete-turn-end"
            className="flex cursor-pointer items-start gap-2"
          >
            <input
              id="setting-autocomplete-turn-end"
              data-testid="setting-autocomplete-turn-end"
              type="checkbox"
              checked={autocompleteTurnEnd}
              onChange={(e) => setAutocompleteTurnEnd(e.target.checked)}
              className="mt-0.5 shrink-0 accent-acc"
            />
            <span className="font-medium text-fg-2" style={{ fontSize: "11.5px" }}>
              Try to auto-complete a node when its agent has clearly finished its turn
            </span>
          </label>
          <div className="text-fg-4" style={{ fontSize: "10.5px" }}>
            When an agent stops without running <span className="font-mono">pdo complete</span>,
            PDO can finish the node for it — but only when its transcript shows the turn
            actually ended (no tool call in flight) <strong>and</strong> its declared outputs
            validate. An agent still inside a long tool call, waiting on a reply, or stopped
            to ask a question is never touched. Leave this off and such a node waits for you.
          </div>
          <div
            className="text-fg-3"
            style={{ fontSize: "10.5px" }}
            data-testid="setting-source-autocomplete-turn-end"
          >
            {autocompleteSourceNote(settings.autocomplete_turn_end)}
          </div>
        </div>

        {/* Default Run auto-naming (#338): whether a Run created with no name is named by
            the Pipeline Manager. The New Run box and a new Trigger seed their choice from
            this. On by default — the pre-#338 behaviour. */}
        <div className="flex flex-col gap-1.5">
          <label
            htmlFor="setting-default-auto-name"
            className="flex cursor-pointer items-start gap-2"
          >
            <input
              id="setting-default-auto-name"
              data-testid="setting-default-auto-name"
              type="checkbox"
              checked={defaultAutoName}
              onChange={(e) => setDefaultAutoName(e.target.checked)}
              className="mt-0.5 shrink-0 accent-acc"
            />
            <span className="font-medium text-fg-2" style={{ fontSize: "11.5px" }}>
              Let the manager auto-name a Run created without a name
            </span>
          </label>
          <div className="text-fg-4" style={{ fontSize: "10.5px" }}>
            When a Run starts with no name — from the New Run dialog with "Auto-generated"
            left on, or from a Trigger — the Pipeline Manager gives it a short descriptive
            name (from the input, or a placeholder it renames once the run's purpose is
            clear). Turn this off and such a Run keeps a stable{" "}
            <span className="font-mono">Untitled run …</span> placeholder instead. This is
            only the <strong>default</strong>: the New Run box and each Trigger can override it.
          </div>
          <div
            className="text-fg-3"
            style={{ fontSize: "10.5px" }}
            data-testid="setting-source-default-auto-name"
          >
            {defaultAutoNameSourceNote(settings.default_auto_name)}
          </div>
        </div>

        {/* Default model (#347): the instance-wide model a work node uses when it
            has no `model:` override. Precedence: node → instance → account
            default. Reuses the per-node ModelPicker verbatim. */}
        <div className="flex flex-col gap-1.5">
          <label className="font-medium text-fg-2" style={{ fontSize: "11.5px" }}>
            Default model
          </label>
          <ModelPicker
            value={model}
            onChange={setModel}
            /* #616: the legacy instance-wide default folds under `claude` at
               resolve, so it offers claude's served catalogue (free text if none). */
            models={
              findHarnessOption(harnessCatalog(settings.harness_descriptors), "claude")
                ?.models ?? []
            }
            testid="default-model"
            /* One instance-wide default, not one per node — a constant subject. */
            subject="instance-default"
          />
          <div className="text-fg-4" style={{ fontSize: "10.5px" }}>
            The model every work node launches with unless it sets its own. "Default"
            leaves it to your Claude account (no <span className="font-mono">--model</span>).
          </div>
          <div
            className="text-fg-3"
            style={{ fontSize: "10.5px" }}
            data-testid="setting-source-default-model"
          >
            {modelSourceNote(settings.default_model)}
          </div>
        </div>

        {/* Default harness (#550/ADR-0046): the harness a new node runs on unless
            it pins its own or a coarser tier sets one. Precedence at spawn:
            node → Run → Projet → instance (this) → claude floor. */}
        <div className="flex flex-col gap-1.5" data-testid="setting-default-harness">
          <label className="font-medium text-fg-2" style={{ fontSize: "11.5px" }}>
            Default harness
          </label>
          <HarnessSelect
            value={defaultHarness}
            onChange={setDefaultHarness}
            catalog={harnessCatalog(settings.harness_descriptors)}
            inheritLabel="Default (claude floor)"
            data-testid="setting-default-harness-select"
            className="rounded border border-line-strong bg-bg-3 px-2 py-1 text-fg"
            style={{ fontSize: "11px" }}
          />
          <div className="text-fg-4" style={{ fontSize: "10.5px" }}>
            Every new node runs on this harness unless it pins its own. "Default"
            leaves it to the <span className="font-mono">claude</span> floor.
          </div>
          <div
            className="text-fg-3"
            style={{ fontSize: "10.5px" }}
            data-testid="setting-source-default-harness"
          >
            {modelSourceNote(settings.default_harness)}
          </div>
        </div>

        {/* Default model per harness (#550/ADR-0046, #616 correctif 1): a slug means
            nothing outside its harness, so the instance default model is per-harness.
            The rows are DERIVED from the served harness list — one per resolved
            harness — never two hard-coded fields. Saving preserves every harness's
            value (the map is sent whole), so setting one no longer wipes another. */}
        <div className="flex flex-col gap-1.5" data-testid="setting-default-harness-model">
          <label className="font-medium text-fg-2" style={{ fontSize: "11.5px" }}>
            Default model per harness
          </label>
          {(() => {
            const cat = harnessCatalog(settings.harness_descriptors);
            const rows = [...cat.builtin, ...cat.descriptors];
            return rows.map((h) => (
              <label
                key={h.name}
                className="flex items-center gap-2 text-fg-3"
                style={{ fontSize: "11px" }}
              >
                <span className="w-16 font-mono">{h.name}</span>
                <input
                  type="text"
                  value={harnessModels[h.name] ?? ""}
                  onChange={(e) =>
                    setHarnessModels((m) => ({ ...m, [h.name]: e.target.value }))
                  }
                  placeholder="account default"
                  data-testid={`setting-default-model-${h.name}`}
                  className="flex-1 rounded border border-line-strong bg-bg-3 px-2 py-1 text-fg"
                />
              </label>
            ));
          })()}
          <div className="text-fg-4" style={{ fontSize: "10.5px" }}>
            The model a node runs with on that harness when it sets none. Empty = the
            harness account default.
          </div>
        </div>

        {/* Default sandbox (#410/#432): what a Run uses when neither the launch dialog
            nor a firing Trigger picks one. Since #432 the options are DATA — `off` plus
            the instance's staging profiles — so this is no longer a closed enum, and a
            stored name can dangle (see the tombstone and the daemon-supplied `reason`). */}
        <div className="flex flex-col gap-1.5">
          <label
            htmlFor="setting-default-sandbox"
            className="font-medium text-fg-2"
            style={{ fontSize: "11.5px" }}
          >
            Default sandbox
          </label>
          <select
            id="setting-default-sandbox"
            data-testid="setting-default-sandbox"
            value={defaultSandbox}
            onChange={(e) => setDefaultSandbox(e.target.value)}
            className="w-full rounded-md border border-line-strong bg-bg-3 px-2.5 py-1.5 font-mono text-fg transition-colors focus:border-acc focus:outline-none"
            style={{ fontSize: "12px" }}
          >
            <option value="off">off (run on the host)</option>
            {settings.sandbox_profiles.map((p) => (
              <option key={p.name} value={p.name}>
                {p.name}
                {p.virtual ? " (built-in)" : ""}
              </option>
            ))}
            {/* Tombstone (same rule as the launch dialog): a seeded value is NEVER
                silently rewritten. Without it React would render the field blank and a
                Save would clear the knob — a silent fallback to `off`. */}
            {missingDefaultProfile && (
              <option value={defaultSandbox} data-testid="setting-default-sandbox-missing">
                {defaultSandbox} — missing
              </option>
            )}
          </select>
          <div className="text-fg-4" style={{ fontSize: "10.5px" }}>
            What a Run uses when neither the launch dialog nor a firing Trigger picks one.{" "}
            <span className="font-mono">off</span> runs on the host; a{" "}
            <span className="font-mono">staging profile</span> runs it inside a Docker
            sandbox with that profile's home content (requires Docker).
          </div>
          <button
            type="button"
            onClick={onManageProfiles}
            data-testid="setting-manage-staging-profiles"
            className="self-start rounded-md border border-line-strong bg-bg-3 px-2.5 py-1 text-fg-2 transition-colors hover:border-acc hover:bg-bg-4"
            style={{ fontSize: "11px" }}
          >
            Manage staging profiles…
          </button>
          {/* Server-supplied, because the `env` tier passes through no validator at all:
              a `PDO_DEFAULT_SANDBOX` naming a vanished profile is only visible here. */}
          {settings.default_sandbox.reason && (
            <div
              className="text-st-failed"
              style={{ fontSize: "10.5px" }}
              data-testid="setting-default-sandbox-reason"
            >
              {settings.default_sandbox.reason}
            </div>
          )}
          <div
            className="text-fg-3"
            style={{ fontSize: "10.5px" }}
            data-testid="setting-source-default-sandbox"
          >
            {defaultSandboxSourceNote(settings.default_sandbox)}
          </div>
        </div>

        {/* Price table (#427, ADR-0034) — READ ONLY. Not a knob: this is which of
            the three price tiers is in force, so there is nothing to PUT. The paths
            are shown even when the files are absent, because nothing is ever seeded
            and naming them is the only way a user learns where to write.

            The presence check is not dead code despite the non-optional type: in the
            `vite dev` workflow the SPA is served from source against a separately
            built daemon, which may predate this field. In production the SPA is
            embedded in the binary, so the two can never disagree. */}
        {settings.price_table && (
          <div className="flex flex-col gap-1.5" data-testid="setting-price-table">
            <label className="font-medium text-fg-2" style={{ fontSize: "11.5px" }}>
              Cost price table
            </label>
            <div className="text-fg-4" style={{ fontSize: "10.5px" }}>
              Costs resolve <span className="font-mono">manual → fetched → built-in</span>,
              per model family. Neither file exists until you create one (or press
              “Sync costs” in Stats); the built-in table is the floor.
            </div>
            <div
              className="text-fg-3"
              style={{ fontSize: "10.5px" }}
              data-testid="setting-price-table-manual-path"
            >
              Yours (wins):{" "}
              <span className="font-mono">
                {settings.price_table.manual_path ?? "— (HOME unset)"}
              </span>
              {settings.price_table.manual_keys.length > 0 &&
                ` — ${settings.price_table.manual_keys.length} model(s): ${settings.price_table.manual_keys.join(", ")}`}
            </div>
            <div
              className="text-fg-3"
              style={{ fontSize: "10.5px" }}
              data-testid="setting-price-table-fetched-path"
            >
              Synced (PDO writes this):{" "}
              <span className="font-mono">
                {settings.price_table.fetched_path ?? "— (HOME unset)"}
              </span>
              {` — ${settings.price_table.fetched_rows} model(s)`}
            </div>
            {/* The table's vintage, readable rather than guessed: a third-party
                source is now a correctness dependency of the numbers shown. */}
            <div
              className="text-fg-4"
              style={{ fontSize: "10.5px" }}
              data-testid="setting-price-table-fetched-at"
            >
              {settings.price_table.fetched_at
                ? `Last synced ${settings.price_table.fetched_at}${settings.price_table.source ? ` from ${settings.price_table.source}` : ""}`
                : "Never synced — only the built-in prices apply."}
            </div>
            {/* The resolved read view (winning tier + $/MTok per family, #528) is
                NOT here — it lives on the Stats → Cost tab, beside "Sync costs",
                fed by `/stats/cost`. Settings keeps the paths / source / vintage. */}
            {/* Server-supplied, because a hand-edited file passes through no
                validator at all: an inert file or refused row is only visible here
                (and in journalctl, which is this product's recurring blind spot). */}
            {settings.price_table.reason && (
              <div
                className="text-st-failed"
                style={{ fontSize: "10.5px" }}
                data-testid="setting-price-table-reason"
              >
                {settings.price_table.reason}
              </div>
            )}
          </div>
        )}

        {/* #553/ADR-0045: the harness descriptor disk tier — declare a harness PDO
            does not ship by writing this file; it merges over the built-in floor by
            name. Same posture as the price table above: the path is always shown
            (nothing is seeded), and a broken/refused descriptor is named here (the
            only place, since a hand-edited descriptor passes through no validator).
            Guarded so a daemon predating #553 renders nothing. */}
        {settings.harness_descriptors && (
          <div className="flex flex-col gap-1.5" data-testid="setting-harness-descriptors">
            <label className="font-medium text-fg-2" style={{ fontSize: "11.5px" }}>
              Harness descriptors
            </label>
            <div className="text-fg-4" style={{ fontSize: "10.5px" }}>
              Declare a harness PDO does not ship by writing this file; it merges over
              the built-in <span className="font-mono">claude</span> /{" "}
              <span className="font-mono">opencode</span> by name. Nothing is ever
              seeded — the built-in harnesses are the floor.
            </div>
            <div
              className="text-fg-3"
              style={{ fontSize: "10.5px" }}
              data-testid="setting-harness-descriptors-path"
            >
              File:{" "}
              <span className="font-mono">
                {settings.harness_descriptors.path ?? "— (HOME unset)"}
              </span>
            </div>
            <div
              className="text-fg-3"
              style={{ fontSize: "10.5px" }}
              data-testid="setting-harness-descriptors-names"
            >
              Harnesses: {settings.harness_descriptors.names.join(", ")}
            </div>
            {settings.harness_descriptors.reason && (
              <div
                className="text-st-failed"
                style={{ fontSize: "10.5px" }}
                data-testid="setting-harness-descriptors-reason"
              >
                {settings.harness_descriptors.reason}
              </div>
            )}
          </div>
        )}

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

/** Which tier the instance default_model comes from (#347). Unlike the numeric
 *  knobs there is no built-in default, so the "default" tier is the account
 *  default (no `--model`). Discloses a shadowed env var too. */
function modelSourceNote(field: StringSettingField): string {
  const envDisplay = field.env ? `PDO_DEFAULT_MODEL=${field.env}` : null;
  if (field.source === "stored") {
    return envDisplay
      ? `Source: stored value (wins). Env ${envDisplay} is set but overridden.`
      : `Source: stored value (overrides env and account default).`;
  }
  if (field.source === "env") {
    return `Source: env ${envDisplay ?? "PDO_DEFAULT_MODEL"}.`;
  }
  return `Source: your Claude account default (no --model).`;
}

/** Open-at directory for the Dockerfile picker (#431/#467): the PARENT of the current
 *  absolute path, so the explorer lands where the file lives rather than at `$HOME`.
 *  Anything non-absolute (or absent) → undefined, i.e. the daemon's default chain. The
 *  daemon also clamps a file path to its parent, but resolving it here keeps the very
 *  first request honest about what it is asking for. */
function dockerfileStartDir(path: string | null): string | undefined {
  const trimmed = (path ?? "").trim();
  if (!trimmed.startsWith("/")) return undefined;
  const lastSlash = trimmed.lastIndexOf("/");
  return lastSlash <= 0 ? "/" : trimmed.slice(0, lastSlash);
}

/** Which tier turn-end auto-completion comes from (#469). Like the enum knobs there IS
 *  a built-in default (`off`), and both directions of a save are a stored decision — so
 *  "stored (off)" is a meaningful state and must read differently from the default. */
function autocompleteSourceNote(field: BoolSettingField): string {
  const onOff = (v: boolean) => (v ? "on" : "off");
  const envDisplay =
    field.env != null ? `PDO_AUTOCOMPLETE_TURN_END=${onOff(field.env)}` : null;
  if (field.source === "stored") {
    const base = `Source: stored value (${onOff(field.effective)}).`;
    return envDisplay
      ? `${base} Env ${envDisplay} is set but overridden.`
      : `${base} Overrides env and default.`;
  }
  if (field.source === "env") {
    return `Source: env ${envDisplay ?? "PDO_AUTOCOMPLETE_TURN_END"}.`;
  }
  return `Source: built-in default (${onOff(field.default)}).`;
}

/** Which tier the instance default_auto_name comes from (#338). Same shape as
 *  {@link autocompleteSourceNote} — a real built-in default (`on`), and both directions
 *  of a save are a stored decision, so "stored (off)" reads differently from the default. */
function defaultAutoNameSourceNote(field: BoolSettingField): string {
  const onOff = (v: boolean) => (v ? "on" : "off");
  const envDisplay = field.env != null ? `PDO_DEFAULT_AUTO_NAME=${onOff(field.env)}` : null;
  if (field.source === "stored") {
    const base = `Source: stored value (${onOff(field.effective)}).`;
    return envDisplay
      ? `${base} Env ${envDisplay} is set but overridden.`
      : `${base} Overrides env and default.`;
  }
  if (field.source === "env") {
    return `Source: env ${envDisplay ?? "PDO_DEFAULT_AUTO_NAME"}.`;
  }
  return `Source: built-in default (${onOff(field.default)}).`;
}

/** Which tier the instance default_sandbox comes from (#410). Unlike `default_model` there
 *  IS a built-in default (`off`). Discloses a shadowed env var too. The dangling-profile
 *  `reason` is rendered separately — it is a health signal, not a precedence note. */
function defaultSandboxSourceNote(field: EnumSettingFieldWithReason): string {
  const envDisplay = field.env ? `PDO_DEFAULT_SANDBOX=${field.env}` : null;
  if (field.source === "stored") {
    return envDisplay
      ? `Source: stored value (wins). Env ${envDisplay} is set but overridden.`
      : `Source: stored value (overrides env and default).`;
  }
  if (field.source === "env") {
    return `Source: env ${envDisplay ?? "PDO_DEFAULT_SANDBOX"}.`;
  }
  return `Source: built-in default (${field.default ?? "off"}).`;
}

// --- Staging profiles (#432, ADR-0031 §2-§7) ---------------------------------

/**
 * Turn an absolute path from {@link FsExplorerModal} into a `$HOME`-relative profile
 * entry. `null` when it does not live under `$HOME` (or `$HOME` is unknown), so the caller
 * can say so inline instead of firing a `PUT` the daemon will 400.
 *
 * A pure helper next to {@link dockerfileStartDir}, and the reason `GET /settings` grew a
 * `home` field: `onPick` yields an ABSOLUTE path, an entry is RELATIVE, and no endpoint
 * exposed `$HOME` before this. Do NOT derive it from any other path the payload happens to
 * carry — `home` lives behind the `sandbox_home_override` seam, and the daemon revalidates
 * at the edge regardless.
 */
// Exported for its own unit test. Precedent: `RUN_INTENT` in `NewRunModal.tsx` — a pure,
// render-free value whose Fast-Refresh cost is nil, opted out one line at a time rather
// than moved to a module of its own.
// eslint-disable-next-line react-refresh/only-export-components
export function relativiseToHome(abs: string, home: string | null): string | null {
  if (!home) return null;
  const h = home.replace(/\/+$/, "");
  if (abs === h) return null; // `$HOME` itself is not an entry
  const prefix = `${h}/`;
  if (!abs.startsWith(prefix)) return null;
  const rel = abs.slice(prefix.length).replace(/\/+$/, "");
  return rel.length > 0 ? rel : null;
}

interface PanelProps {
  /** Host `$HOME`, from `GET /settings`. `null` disables the pickers' relativisation. */
  home: string | null;
  onDone: () => void;
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
function StagingProfilesPanel({ home, onDone, onChanged }: PanelProps) {
  const [profiles, setProfiles] = useState<SandboxProfile[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState("");
  const [pickerMode, setPickerMode] = useState<"file" | "dir" | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<SandboxProfileReferents | null>(null);
  // The pending env row. Two fields and no per-row edit mode: an existing variable is
  // changed by removing it and adding it again, which keeps every PUT a full replacement
  // (the daemon's contract) instead of a patch the client would have to compose.
  const [envKey, setEnvKey] = useState("");
  const [envValue, setEnvValue] = useState("");

  const load = useCallback(async (keep?: string | null) => {
    try {
      const { profiles: list } = await fetchSandboxProfiles();
      setProfiles(list);
      setSelected((cur) => {
        const want = keep ?? cur;
        if (want && list.some((p) => p.name === want)) return want;
        return list[0]?.name ?? null;
      });
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to load staging profiles");
    }
  }, []);

  // One fetch on mount — the panel is mounted only when the drill-down opens, so mounting
  // IS "the user opened the editor". `load` sets state after its `await`, which the rule
  // cannot see through; same trade-off and same disable as `FsExplorerModal`'s initial
  // navigate.
  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect -- async, see note above.
    void load();
  }, [load]);

  const current = profiles?.find((p) => p.name === selected) ?? null;

  /**
   * Write a profile's diff, then refresh both this panel and the parent's settings.
   *
   * Every `PUT` is a FULL replacement — `env` and `image` included — so each caller passes the
   * fields it is not changing verbatim. Threading that through one helper is what keeps "toggle
   * an entry" from quietly clearing the env or resetting the image source.
   */
  const write = useCallback(
    async (
      name: string,
      diff: {
        disabled: string[];
        extras: string[];
        env: Record<string, string>;
        image: SandboxProfileImage | null;
      },
    ) => {
      if (busy) return;
      setBusy(true);
      setError(null);
      try {
        await saveSandboxProfile(name, diff);
        await load(name);
        onChanged();
      } catch (e) {
        setError(e instanceof Error ? e.message : "Failed to save the profile");
      } finally {
        setBusy(false);
      }
    },
    [busy, load, onChanged],
  );

  /**
   * Toggle one entry. Only a BUILT-IN DEFAULT entry is toggleable: unchecking adds it to
   * `disabled`, re-checking removes it. An extra is removed by dropping it from `extras`,
   * never by "unchecking" it — conflating the two would make the stored diff ambiguous
   * (and the daemon rejects a `disabled` that is not a default entry).
   */
  const toggleDefault = (entry: SandboxProfileEntry) => {
    if (!current || !entry.from_default) return;
    const disabled = entry.enabled
      ? [...current.disabled, entry.path]
      : current.disabled.filter((d) => d !== entry.path);
    void write(current.name, {
      disabled,
      extras: current.extras,
      env: current.env,
      image: current.image,
    });
  };

  const removeExtra = (path: string) => {
    if (!current) return;
    void write(current.name, {
      disabled: current.disabled,
      extras: current.extras.filter((e) => e !== path),
      env: current.env,
      image: current.image,
    });
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
    void write(current.name, {
      disabled: current.disabled,
      extras: [...current.extras, rel],
      env: current.env,
      image: current.image,
    });
  };

  /**
   * Add (or replace) one environment variable (#468).
   *
   * The two refusals handled inline are the ones the user can see before a round-trip: a
   * blank name, and a name PDO poses itself. `reserved_env_keys` comes from the daemon —
   * hard-coding the three here would drift the day a fourth run-constant appears. Every
   * other rule (the `[A-Za-z_][A-Za-z0-9_]*` grammar, multi-line values) is left to the
   * daemon's 400, which this panel already surfaces verbatim: duplicating a grammar in two
   * languages is exactly the drift #373 cost us.
   */
  const addEnv = () => {
    if (!current) return;
    const key = envKey.trim();
    if (!key) return;
    if (current.reserved_env_keys.includes(key)) {
      setError(
        `\`${key}\` is set by PDO for every sandboxed Run and cannot be overridden — the container's home, its daemon URL and its Run id all depend on it.`,
      );
      return;
    }
    setEnvKey("");
    setEnvValue("");
    void write(current.name, {
      disabled: current.disabled,
      extras: current.extras,
      env: { ...current.env, [key]: envValue },
      image: current.image,
    });
  };

  const removeEnv = (key: string) => {
    if (!current) return;
    // A full replacement, so "remove" is literally "PUT the map without that key".
    const env = Object.fromEntries(
      Object.entries(current.env).filter(([k]) => k !== key),
    );
    void write(current.name, {
      disabled: current.disabled,
      extras: current.extras,
      env,
      image: current.image,
    });
  };

  /**
   * Write the profile's image source (#467). `null` clears it — a FULL replacement, so that is
   * literally how "go back to PDO's default image" is expressed (#471).
   *
   * Nothing is validated here beyond emptiness: the absolute-path rule, the existence check and
   * the ref grammar are all the daemon's 400s, which this panel surfaces verbatim. Re-deriving any
   * of them in TypeScript is exactly the drift #373 cost us — and the existence check is not even
   * derivable in a browser.
   */
  const writeImage = (image: SandboxProfileImage | null) => {
    if (!current) return;
    void write(current.name, {
      disabled: current.disabled,
      extras: current.extras,
      env: current.env,
      image,
    });
  };

  const create = async () => {
    const name = newName.trim();
    if (!name || busy) return;
    setBusy(true);
    setError(null);
    try {
      // A blank diff: "materialise this profile exactly as the current default", which is
      // the starting point for unchecking something.
      await saveSandboxProfile(name, { disabled: [], extras: [], env: {}, image: null });
      setNewName("");
      setCreating(false);
      await load(name);
      onChanged();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to create the profile");
    } finally {
      setBusy(false);
    }
  };

  /** Ask the daemon who points at the profile BEFORE opening the confirmation. */
  const askDelete = async (name: string) => {
    setError(null);
    try {
      setConfirmDelete(await fetchSandboxProfileReferents(name));
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to look up referents");
    }
  };

  const doDelete = async (name: string) => {
    setBusy(true);
    setError(null);
    try {
      await deleteSandboxProfile(name);
      setConfirmDelete(null);
      await load(null);
      onChanged();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to delete the profile");
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <div
        className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-4 py-4"
        data-testid="staging-profiles-panel"
      >
        {/* ── the profile list ── */}
        <div className="flex flex-col gap-1">
          {profiles == null ? (
            <div className="text-fg-4" style={{ fontSize: "12px" }} data-testid="staging-profiles-loading">
              Loading…
            </div>
          ) : (
            profiles.map((p) => (
              <div
                key={p.name}
                className={`flex items-center gap-2 rounded-md border px-2.5 py-1.5 transition-colors ${
                  p.name === selected
                    ? "border-acc bg-bg-3"
                    : "border-line-strong bg-bg-3 hover:border-line"
                }`}
              >
                <button
                  type="button"
                  onClick={() => setSelected(p.name)}
                  data-testid={`staging-profile-row-${p.name}`}
                  className="flex min-w-0 flex-1 items-center gap-2 text-left"
                >
                  <span className="truncate font-mono text-fg" style={{ fontSize: "12px" }}>
                    {p.name}
                  </span>
                  <span className="shrink-0 text-fg-4" style={{ fontSize: "10px" }}>
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
                    className="shrink-0 rounded p-1 text-fg-4 transition-colors hover:bg-bg-5 hover:text-st-failed"
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
                className="min-w-0 flex-1 rounded-md border border-line-strong bg-bg-3 px-2.5 py-1.5 font-mono text-fg placeholder:text-fg-4 focus:border-acc focus:outline-none"
                style={{ fontSize: "12px" }}
              />
              <button
                type="button"
                onClick={() => void create()}
                disabled={busy || newName.trim().length === 0}
                data-testid="staging-profile-create"
                className="shrink-0 rounded-md bg-acc px-2.5 py-1.5 font-medium text-[#04140d] transition-colors hover:bg-acc-dim disabled:opacity-40"
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
              className="mt-1 flex items-center gap-1.5 self-start rounded-md px-1 py-1 text-fg-3 transition-colors hover:text-fg"
              style={{ fontSize: "11px" }}
            >
              <Plus size={12} /> New profile
            </button>
          )}
          <div className="text-fg-4" style={{ fontSize: "10.5px" }}>
            Lowercase letters, digits and <span className="font-mono">-</span>. A new profile
            starts as a copy of the built-in default; unchecking an entry stores the
            difference, so a later PDO release that adds an entry still shows up here.
          </div>
        </div>

        {error && (
          <div
            className="rounded-md border border-st-failed/30 bg-st-failed-bg px-3 py-2 text-st-failed"
            style={{ fontSize: "11.5px" }}
            data-testid="staging-profiles-error"
          >
            {error}
          </div>
        )}

        {/* ── the selected profile's entries ── */}
        {current && (
          <div className="flex flex-col gap-3 border-t border-line pt-4">
            <h3 className="font-medium text-fg-2" style={{ fontSize: "12px" }}>
              <span className="font-mono">{current.name}</span> — entries
            </h3>

            {/* The FLOOR, read-only. Without this block a `minimal` profile's screen looks
                broken and the user wrongly concludes the container starts with no
                credentials. These are guarantees, satisfied by a host copy OR by a
                fallback synthesis — never selectable, and refused as extras too. */}
            <div className="flex flex-col gap-1" data-testid="staging-profile-floor">
              <span className="text-fg-3" style={{ fontSize: "10.5px" }}>
                Floor (always applied, not editable)
              </span>
              <ul className="flex flex-col gap-0.5">
                {current.floor.map((g) => (
                  <li key={g.id} className="flex items-baseline gap-2 text-fg-4" style={{ fontSize: "10.5px" }}>
                    <span>· {g.label}</span>
                    {g.path && <span className="truncate font-mono">{g.path}</span>}
                  </li>
                ))}
              </ul>
            </div>

            {/* From the default: checkable. `minimal` legitimately has none — say so
                explicitly, or the empty area reads as a loading failure. */}
            <div className="flex flex-col gap-1.5">
              <span className="text-fg-3" style={{ fontSize: "10.5px" }}>
                From the default
              </span>
              {current.entries.filter((e) => e.from_default).length === 0 ? (
                <span
                  className="text-fg-4"
                  style={{ fontSize: "10.5px" }}
                  data-testid="staging-profile-no-default-entries"
                >
                  No entries — <span className="font-mono">{current.name}</span> <em>is</em> the
                  floor.
                </span>
              ) : (
                current.entries
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
                        onChange={() => toggleDefault(e)}
                        className="mt-0.5 shrink-0 accent-acc"
                      />
                      <span className="flex min-w-0 flex-col">
                        <span className="flex items-baseline gap-2">
                          <span className="truncate font-mono text-fg" style={{ fontSize: "11.5px" }}>
                            {e.path}
                          </span>
                          <span className="shrink-0 text-fg-4" style={{ fontSize: "10px" }}>
                            {e.kind}
                          </span>
                          {e.exists === false && (
                            <span className="shrink-0 text-fg-4" style={{ fontSize: "10px" }}>
                              not on this host
                            </span>
                          )}
                        </span>
                        {e.note && (
                          <span className="text-fg-4" style={{ fontSize: "10px" }}>
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
              <span className="text-fg-3" style={{ fontSize: "10.5px" }}>
                Extras
              </span>
              {current.entries
                .filter((e) => !e.from_default)
                .map((e) => (
                  <div
                    key={e.path}
                    className="flex items-center gap-2"
                    data-testid={`staging-extra-${e.path}`}
                  >
                    <span className="truncate font-mono text-fg" style={{ fontSize: "11.5px" }}>
                      ~/{e.path}
                    </span>
                    <span className="shrink-0 text-fg-4" style={{ fontSize: "10px" }}>
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
                      <span className="shrink-0 text-fg-4" style={{ fontSize: "10px" }}>
                        missing
                      </span>
                    )}
                    <button
                      type="button"
                      onClick={() => removeExtra(e.path)}
                      disabled={busy}
                      aria-label={`Remove ${e.path}`}
                      data-testid={`staging-extra-remove-${e.path}`}
                      className="ml-auto shrink-0 rounded p-0.5 text-fg-4 transition-colors hover:bg-bg-5 hover:text-fg-2 disabled:opacity-40"
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
                  onClick={() => setPickerMode("file")}
                  data-testid="staging-extra-add-file"
                  className="rounded-md border border-line-strong bg-bg-3 px-2.5 py-1 text-fg-2 transition-colors hover:border-acc"
                  style={{ fontSize: "11px" }}
                >
                  <span className="inline-flex items-center gap-1.5">
                    <Search size={11} /> Add file…
                  </span>
                </button>
                <button
                  type="button"
                  onClick={() => setPickerMode("dir")}
                  data-testid="staging-extra-add-folder"
                  className="rounded-md border border-line-strong bg-bg-3 px-2.5 py-1 text-fg-2 transition-colors hover:border-acc"
                  style={{ fontSize: "11px" }}
                >
                  <span className="inline-flex items-center gap-1.5">
                    <Search size={11} /> Add folder…
                  </span>
                </button>
              </div>
              <div className="text-fg-4" style={{ fontSize: "10.5px" }}>
                An extra is <strong>copied</strong> into the run's staging and mounted
                read-write in the container. The host file is never mutated — and never
                bind-mounted, so nothing the container writes can reach it.
              </div>
              {current.entries.some((e) => !e.from_default && e.sensitive) && (
                <div
                  className="text-st-await"
                  style={{ fontSize: "10.5px" }}
                  data-testid="staging-profile-sensitive-warning"
                >
                  This profile stages secrets ({current.sensitive_prefixes.join(", ")}). They
                  are copied, not shared — but the container can read them.
                </div>
              )}
            </div>

            {/* Environment (#468, ADR-0031 §8): posed as `-e KEY=value` at `docker create`,
                which is often the ONLY handle on a plugin-provided MCP server whose
                `.mcp.json` PDO does not control. */}
            <div className="flex flex-col gap-1.5" data-testid="staging-profile-env">
              <span className="text-fg-3" style={{ fontSize: "10.5px" }}>
                Environment
              </span>
              {Object.entries(current.env).length === 0 ? (
                <span
                  className="text-fg-4"
                  style={{ fontSize: "10.5px" }}
                  data-testid="staging-profile-no-env"
                >
                  None — the container gets only the variables PDO sets itself.
                </span>
              ) : (
                Object.entries(current.env).map(([key, value]) => (
                  <div
                    key={key}
                    className="flex items-center gap-2"
                    data-testid={`staging-env-${key}`}
                  >
                    <span className="shrink-0 font-mono text-fg" style={{ fontSize: "11.5px" }}>
                      {key}
                    </span>
                    <span className="text-fg-4" style={{ fontSize: "11.5px" }}>
                      =
                    </span>
                    {/* Shown in clear, deliberately: the value is already in clear in the
                        database, in the Run's event file and in `docker inspect`. Masking it
                        would suggest PDO is protecting it. */}
                    <span
                      className="min-w-0 flex-1 truncate font-mono text-fg-2"
                      style={{ fontSize: "11.5px" }}
                      title={value}
                    >
                      {value === "" ? <em className="text-fg-4">(empty)</em> : value}
                    </span>
                    <button
                      type="button"
                      onClick={() => removeEnv(key)}
                      disabled={busy}
                      aria-label={`Remove ${key}`}
                      data-testid={`staging-env-remove-${key}`}
                      className="shrink-0 rounded p-0.5 text-fg-4 transition-colors hover:bg-bg-5 hover:text-fg-2 disabled:opacity-40"
                    >
                      <X size={12} />
                    </button>
                  </div>
                ))
              )}
              <div className="flex items-center gap-2 pt-0.5">
                <input
                  value={envKey}
                  onChange={(e) => setEnvKey(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") addEnv();
                  }}
                  placeholder="PUPPETEER_EXECUTABLE_PATH"
                  aria-label="Environment variable name"
                  data-testid="staging-env-new-key"
                  className="min-w-0 flex-1 rounded-md border border-line-strong bg-bg-3 px-2.5 py-1 font-mono text-fg placeholder:text-fg-4 focus:border-acc focus:outline-none"
                  style={{ fontSize: "11px" }}
                />
                <input
                  value={envValue}
                  onChange={(e) => setEnvValue(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") addEnv();
                  }}
                  placeholder="/usr/bin/chromium"
                  aria-label="Environment variable value"
                  data-testid="staging-env-new-value"
                  className="min-w-0 flex-1 rounded-md border border-line-strong bg-bg-3 px-2.5 py-1 font-mono text-fg placeholder:text-fg-4 focus:border-acc focus:outline-none"
                  style={{ fontSize: "11px" }}
                />
                <button
                  type="button"
                  onClick={addEnv}
                  disabled={busy || envKey.trim().length === 0}
                  data-testid="staging-env-add"
                  className="shrink-0 rounded-md border border-line-strong bg-bg-3 px-2.5 py-1 text-fg-2 transition-colors hover:border-acc disabled:opacity-40"
                  style={{ fontSize: "11px" }}
                >
                  Set
                </button>
              </div>
              <div className="text-fg-4" style={{ fontSize: "10.5px" }}>
                Posed at container creation, for every session of the Run. Often the only way
                to configure an MCP server whose <span className="font-mono">.mcp.json</span>{" "}
                comes from a plugin. <span className="font-mono">{current.reserved_env_keys.join(", ")}</span>{" "}
                are set by PDO and refused here.
              </div>
              {/* Load-bearing copy, not a disclaimer: without it someone puts an API key
                  here believing it is a secret store. */}
              <div
                className="text-st-await"
                style={{ fontSize: "10.5px" }}
                data-testid="staging-profile-env-not-a-vault"
              >
                <strong>This is not a secret store.</strong> Values are stored in clear in the
                PDO database, copied into the Run's frozen event log, and readable with{" "}
                <span className="font-mono">docker inspect</span>. They are kept out of the
                daemon log (names only), and nothing else protects them.
              </div>
            </div>

            {/* Image (#467, ADR-0031 §9): WHICH container the Run gets, as opposed to what
                lands in its home. `key` on the profile name so switching profiles resets the
                draft — the reset is the mount, not an effect (the #385 lesson). */}
            <ProfileImageEditor
              key={current.name}
              profile={current}
              busy={busy}
              onWrite={writeImage}
            />

            {/* Signalled no-ops (ADR-0031 §2). Not errors: the default may LOSE an entry
                tomorrow, and unchecking one a future release will add must be remembered. */}
            {current.inactive_disabled.length > 0 && (
              <div
                className="text-fg-4"
                style={{ fontSize: "10.5px" }}
                data-testid="staging-profile-inactive-disabled"
              >
                Remembered but inactive (not in this version's default):{" "}
                <span className="font-mono">{current.inactive_disabled.join(", ")}</span>
              </div>
            )}
            {current.redundant_extras.length > 0 && (
              <div
                className="text-fg-4"
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

      {/* Footer says DONE, not Save: every edit above already went to the daemon. That
          difference is what makes the drill-down honest — nothing is batched behind it. */}
      <div className="flex items-center justify-end gap-2 border-t border-line px-4 py-3">
        <button
          onClick={onDone}
          data-testid="staging-profiles-done"
          className="rounded-md bg-acc px-3 py-1.5 font-medium text-[#04140d] transition-colors hover:bg-acc-dim"
          style={{ fontSize: "11.5px" }}
        >
          Done
        </button>
      </div>

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
          onCancel={() => setConfirmDelete(null)}
          onConfirm={() => void doDelete(confirmDelete.name)}
        />
      )}
    </>
  );
}

/**
 * The profile's image source (#467, ADR-0031 §9): a three-way `<select>` (PDO's default /
 * Dockerfile / registry ref) plus the one field the chosen kind needs.
 *
 * Since #471 this is the ONLY place an image is chosen — the two instance-wide settings are gone,
 * and what they resolved to by default is a constant of the profile-defaults layer. So the copy
 * that used to live under the Settings field lives here now, in one sentence per kind: **the tag
 * is the SHA-256 of the Dockerfile's bytes.** That is what makes "edit the file to change the
 * image" comprehensible, and it belongs where the choice is made.
 *
 * Its own component for one reason: **the draft resets by remounting**. The parent keys it on the
 * profile name, so selecting another profile cannot leave a half-typed ref from the previous one
 * in the field — the `useEffect`-that-resets-state pattern is precisely what #385 was.
 *
 * The copy is the deliverable as much as the controls are. Two things have to be said about an
 * explicit ref, because nothing else in the product says them and each one is a real support
 * question:
 *
 * 1. **An explicit ref has no fallback.** No Dockerfile ⇒ no content hash ⇒ nothing to build if
 *    the pull fails, so the Run fails instead. That is the amendment to ADR-0030 pt 7.
 * 2. **PDO does not check the image contains `claude`.** Whoever supplies the ref owns that.
 */
function ProfileImageEditor({
  profile,
  busy,
  onWrite,
}: {
  profile: SandboxProfile;
  busy: boolean;
  onWrite: (image: SandboxProfileImage | null) => void;
}) {
  const [kind, setKind] = useState<"default" | "dockerfile" | "registry">(
    profile.image?.kind ?? "default",
  );
  const [path, setPath] = useState(
    profile.image?.kind === "dockerfile" ? profile.image.path : "",
  );
  const [ref, setRef] = useState(profile.image?.kind === "registry" ? profile.image.ref : "");
  const [pickerOpen, setPickerOpen] = useState(false);

  /** Switching to "default" IS the edit — there is no field to fill, so it writes straight
   *  away, consistent with every other control in this panel (nothing is batched). */
  const chooseKind = (next: "default" | "dockerfile" | "registry") => {
    setKind(next);
    if (next === "default" && profile.image !== null) onWrite(null);
  };

  const pending: SandboxProfileImage | null =
    kind === "dockerfile"
      ? { kind: "dockerfile", path: path.trim() }
      : kind === "registry"
        ? { kind: "registry", ref: ref.trim() }
        : null;
  const canSet =
    !busy &&
    ((pending?.kind === "dockerfile" && pending.path.length > 0) ||
      (pending?.kind === "registry" && pending.ref.length > 0));

  return (
    <div className="flex flex-col gap-1.5" data-testid="staging-profile-image">
      <span className="text-fg-3" style={{ fontSize: "10.5px" }}>
        Image
      </span>
      <select
        value={kind}
        onChange={(e) => chooseKind(e.target.value as "default" | "dockerfile" | "registry")}
        disabled={busy}
        aria-label="Image source"
        data-testid="staging-image-kind"
        className="w-full rounded-md border border-line-strong bg-bg-3 px-2.5 py-1 font-mono text-fg focus:border-acc focus:outline-none disabled:opacity-40"
        style={{ fontSize: "11px" }}
      >
        <option value="default">default (PDO's own sandbox image)</option>
        <option value="dockerfile">dockerfile (build from a Dockerfile)</option>
        <option value="registry">registry (pull an explicit ref)</option>
      </select>

      {kind === "default" && (
        <div className="text-fg-4" style={{ fontSize: "10.5px" }} data-testid="staging-image-none">
          PDO's own image, whose tag is the SHA-256 of the bytes of the Dockerfile it seeded at{" "}
          <span className="font-mono">~/.pdo/sandbox/Dockerfile</span> — edit that file to change
          the image, or pick another kind here.
        </div>
      )}

      {kind === "dockerfile" && (
        <>
          <div className="flex items-center gap-2">
            <div className="relative min-w-0 flex-1">
              <input
                value={path}
                onChange={(e) => setPath(e.target.value)}
                placeholder="/path/to/Dockerfile.chrome-dev"
                aria-label="Dockerfile path"
                data-testid="staging-image-path"
                className="w-full rounded-md border border-line-strong bg-bg-3 px-2.5 py-1 pr-8 font-mono text-fg placeholder:text-fg-4 focus:border-acc focus:outline-none"
                style={{ fontSize: "11px" }}
                autoComplete="off"
              />
              <button
                type="button"
                onClick={() => setPickerOpen(true)}
                className="absolute inset-y-0 right-0 flex items-center px-2 text-fg-4 transition-colors hover:text-fg-2"
                title="Browse for a Dockerfile"
                aria-label="Browse for a Dockerfile"
                data-testid="staging-image-browse"
              >
                <Search size={12} />
              </button>
            </div>
            <button
              type="button"
              onClick={() => pending && onWrite(pending)}
              disabled={!canSet}
              data-testid="staging-image-set"
              className="shrink-0 rounded-md border border-line-strong bg-bg-3 px-2.5 py-1 text-fg-2 transition-colors hover:border-acc disabled:opacity-40"
              style={{ fontSize: "11px" }}
            >
              Set
            </button>
          </div>
          <div className="text-fg-4" style={{ fontSize: "10.5px" }}>
            Every Run on this profile builds (or pulls) the image whose tag is the SHA-256 of this
            file's bytes — editing the file changes the tag, hence a rebuild. It must be{" "}
            <strong>self-contained</strong>: the build context is deliberately empty, so no{" "}
            <span className="font-mono">COPY</span>. A filename like{" "}
            <span className="font-mono">Dockerfile.chrome-dev</span> also names the image (
            <span className="font-mono">pdo-sandbox-chrome-dev</span>).
          </div>
        </>
      )}

      {kind === "registry" && (
        <>
          <div className="flex items-center gap-2">
            <input
              value={ref}
              onChange={(e) => setRef(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && canSet && pending) onWrite(pending);
              }}
              placeholder="ghcr.io/owner/image:tag"
              aria-label="Image reference"
              data-testid="staging-image-ref"
              className="min-w-0 flex-1 rounded-md border border-line-strong bg-bg-3 px-2.5 py-1 font-mono text-fg placeholder:text-fg-4 focus:border-acc focus:outline-none"
              style={{ fontSize: "11px" }}
              autoComplete="off"
            />
            <button
              type="button"
              onClick={() => pending && onWrite(pending)}
              disabled={!canSet}
              data-testid="staging-image-set"
              className="shrink-0 rounded-md border border-line-strong bg-bg-3 px-2.5 py-1 text-fg-2 transition-colors hover:border-acc disabled:opacity-40"
              style={{ fontSize: "11px" }}
            >
              Set
            </button>
          </div>
          {/* Load-bearing copy: the two properties an explicit ref LOSES. Without them the first
              failed pull looks like a PDO bug. */}
          <div
            className="text-st-await"
            style={{ fontSize: "10.5px" }}
            data-testid="staging-image-ref-no-fallback"
          >
            Pulled as-is, with <strong>no local build to fall back on</strong>: an explicit ref has
            no Dockerfile, so it has no content hash and nothing to rebuild — a failed pull{" "}
            <strong>fails the Run</strong>, naming the ref. PDO also cannot check that the image
            contains <span className="font-mono">claude</span>; that is on whoever supplies it.
          </div>
        </>
      )}

      {pickerOpen && (
        <FsExplorerModal
          mode="file"
          showHidden
          title="Choose a Dockerfile for this profile"
          confirmLabel="Use this Dockerfile"
          startPath={dockerfileStartDir(path)}
          onPick={(abs) => {
            setPath(abs);
            onWrite({ kind: "dockerfile", path: abs });
          }}
          onClose={() => setPickerOpen(false)}
        />
      )}
    </div>
  );
}

/**
 * Confirmation before deleting a profile — the "soft guard-rail" of ADR-0031 §7 (there is
 * no referential integrity in the database, and the `DELETE` is unconditional).
 *
 * It exists to say the two things nothing else in the UI says: deleting does **not**
 * repoint the referents, and their next Run **fails** rather than falling back to a
 * default — while live Runs already froze their entry list and are unaffected.
 *
 * `z-[60]`, the same layer as `FsExplorerModal`; they are never open at once.
 */
function DeleteProfileDialog({
  referents,
  busy,
  onCancel,
  onConfirm,
}: {
  referents: SandboxProfileReferents;
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const pointing = referents.triggers.length + (referents.instance_default ? 1 : 0);
  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/50"
      onClick={(e) => {
        e.stopPropagation();
        onCancel();
      }}
    >
      <div
        className="w-[420px] rounded-lg border border-line bg-bg-4 p-4 shadow-xl"
        onClick={(e) => e.stopPropagation()}
        data-testid="staging-profile-delete-dialog"
      >
        <h3 className="font-semibold text-fg" style={{ fontSize: "12.5px" }}>
          Delete profile <span className="font-mono">{referents.name}</span>?
        </h3>
        {pointing > 0 ? (
          <>
            <p className="mt-2 text-fg-2" style={{ fontSize: "11.5px" }}>
              {pointing} thing{pointing === 1 ? "" : "s"} still point at it. Deleting will{" "}
              <strong>not</strong> repoint them — the next Run they produce{" "}
              <strong>fails</strong>, it does not fall back to a default.
            </p>
            <ul className="mt-1.5 flex flex-col gap-0.5" data-testid="staging-profile-referents">
              {referents.instance_default && (
                <li className="text-fg-3" style={{ fontSize: "10.5px" }}>
                  · Instance default sandbox
                </li>
              )}
              {referents.triggers.map((t) => (
                <li key={t.id} className="text-fg-3" style={{ fontSize: "10.5px" }}>
                  · Trigger <span className="font-mono">{t.name}</span>
                  {t.enabled ? "" : " (disabled)"}
                </li>
              ))}
            </ul>
          </>
        ) : (
          <p className="mt-2 text-fg-2" style={{ fontSize: "11.5px" }}>
            Nothing points at it.
          </p>
        )}
        {referents.runs.length > 0 && (
          <p
            className="mt-2 text-fg-4"
            style={{ fontSize: "10.5px" }}
            data-testid="staging-profile-referent-runs"
          >
            {referents.runs.length} running Run{referents.runs.length === 1 ? "" : "s"} already
            froze their entry list at start and are unaffected.
          </p>
        )}
        <div className="mt-4 flex items-center justify-end gap-2">
          <button
            onClick={onCancel}
            className="rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 transition-colors hover:bg-bg-4"
            style={{ fontSize: "11.5px" }}
          >
            Cancel
          </button>
          <button
            onClick={onConfirm}
            disabled={busy}
            data-testid="staging-profile-delete-confirm"
            className="rounded-md bg-st-failed px-3 py-1.5 font-medium text-bg-1 transition-opacity hover:opacity-90 disabled:opacity-40"
            style={{ fontSize: "11.5px" }}
          >
            Delete
          </button>
        </div>
      </div>
    </div>
  );
}
