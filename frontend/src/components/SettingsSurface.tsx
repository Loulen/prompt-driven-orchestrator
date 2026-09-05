import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { ExternalLink, FileText, X, RefreshCw, Copy, Check } from "lucide-react";
import { mockUpdateStatus, newerAvailable, absoluteTime, relativeTime, PROTO_SCENARIO, type UpdateStatus } from "../design-proto/updateMock";
import FullWindowShell from "./FullWindowShell";
import { announceSettingsChanged, useSettings } from "../hooks/useSettings";
import { useEditStore } from "../stores/editStore";
import type {
  AgentChoice,
  BoolSettingField,
  EnumSettingFieldWithReason,
  InstanceSettings,
  SettingField,
  SkillRef,
  StringSettingField,
  UpdateSettingsRequest,
} from "../types";
import ModelPicker from "./ModelPicker";
import SessionCounter from "./SessionCounter";
import HarnessSelect from "./HarnessSelect";
import { harnessCatalog, findHarnessOption } from "../lib/harness";
import AgentControl from "./AgentControl";
import { announceAgentProfilesChanged, useAgentProfiles } from "../hooks/useAgentProfiles";
import PersistedProvisioningEditor from "./PersistedProvisioningEditor";
import SkillBankPanel from "./SkillBankPanel";
import { announceSkillsChanged, useSkillBank } from "../hooks/useSkillBank";
import SkillSelector from "./SkillSelector";
import { announceSkillTiersChanged } from "../hooks/useSkillTiers";
import { useScrollSpy } from "../hooks/useScrollSpy";
import AgentProfilesPanel from "./AgentProfilesPanel";
import StagingProfilesPanel from "./StagingProfilesPanel";
import {
  SETTINGS_CATEGORIES,
  findCategory,
  rollupDirty,
  type DirtyRollup,
  type SettingsCategoryId,
  type SettingsFieldId,
  type SettingsSection,
  type SettingsSectionId,
} from "./settingsCategories";

export interface SettingsPosition {
  category: SettingsCategoryId;
  section?: SettingsSectionId;
}

export interface StatsOpenIntent {
  tab: "cost";
  pricingOpen: boolean;
}

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
  /**
   * Programmatic entry (#690, story 18): where to land. Read once at mount — the host
   * bumps the component `key` when it wants a new position applied. Without it the
   * surface lands on the last visited category › section of this page session, else
   * General › Interface.
   */
  initialPosition?: SettingsPosition;
  /** Diagnostics › Price table links to Stats › Cost › Pricing details (one surface at a time). */
  onOpenStats?: (intent: StatsOpenIntent) => void;
}

/** Advisory ceiling: caps above this enter the tmux-server-collapse zone
 *  (#77/#78). Not a hard limit (Sharp tool — ADR-0001), just an amber warning. */
const CAP_ADVISORY = 20;

/** The editable instance form, as strings/values the inputs hold. */
interface DraftValues {
  capStr: string;
  ttlStr: string;
  guardStr: string;
  model: string | null;
  defaultHarness: string;
  agentChoice: AgentChoice | null;
  instanceSkills: SkillRef[];
  harnessModels: Record<string, string>;
  defaultSandbox: string;
  autocompleteTurnEnd: boolean;
  defaultAutoName: boolean;
}

function seedFrom(settings: InstanceSettings): DraftValues {
  return {
    capStr: String(settings.session_cap.effective),
    ttlStr: String(settings.reaper_ttl_secs.effective),
    guardStr: String(settings.guard_timeout_secs.effective),
    model: settings.default_model.effective,
    defaultHarness: settings.default_harness.effective ?? "",
    agentChoice: settings.agent_choice ?? null,
    instanceSkills: settings.skills ?? [],
    harnessModels: { ...settings.default_harness_model.stored },
    defaultSandbox: settings.default_sandbox.effective ?? "off",
    autocompleteTurnEnd: settings.autocomplete_turn_end.effective,
    defaultAutoName: settings.default_auto_name.effective,
  };
}

/** The per-harness map the daemon will store: trimmed, empty entries dropped. */
function normaliseHarnessModels(map: Record<string, string>): Record<string, string> {
  const next: Record<string, string> = {};
  for (const [name, raw] of Object.entries(map)) {
    const v = raw.trim();
    if (v) next[name] = v;
  }
  return next;
}

function sameMap(a: Record<string, string>, b: Record<string, string>): boolean {
  return (
    Object.keys(a).length === Object.keys(b).length &&
    Object.entries(a).every(([k, v]) => b[k] === v)
  );
}

function numericDirty(str: string, effective: number): boolean {
  const t = str.trim();
  if (t === "") return true;
  return Number(t) !== effective;
}

/**
 * Which fields of the draft differ from the loaded settings. The same comparisons the
 * Save payload uses, so a dirty dot and a PUT key always agree.
 */
function computeDirty(values: DraftValues, settings: InstanceSettings): Set<SettingsFieldId> {
  const dirty = new Set<SettingsFieldId>();
  if (numericDirty(values.capStr, settings.session_cap.effective)) dirty.add("session-cap");
  if (numericDirty(values.ttlStr, settings.reaper_ttl_secs.effective)) dirty.add("reaper-ttl");
  if (numericDirty(values.guardStr, settings.guard_timeout_secs.effective)) {
    dirty.add("guard-timeout");
  }
  if (values.autocompleteTurnEnd !== settings.autocomplete_turn_end.effective) {
    dirty.add("autocomplete-turn-end");
  }
  if (values.defaultAutoName !== settings.default_auto_name.effective) {
    dirty.add("default-auto-name");
  }
  if (values.model !== settings.default_model.effective) dirty.add("default-model");
  if (values.defaultHarness !== (settings.default_harness.effective ?? "")) {
    dirty.add("default-harness");
  }
  if (JSON.stringify(values.agentChoice) !== JSON.stringify(settings.agent_choice ?? null)) {
    dirty.add("agent-choice");
  }
  const storedSkillIds = (settings.skills ?? []).map((skill) => skill.id).join("\u0000");
  if (values.instanceSkills.map((skill) => skill.id).join("\u0000") !== storedSkillIds) {
    dirty.add("skills");
  }
  if (
    !sameMap(normaliseHarnessModels(values.harnessModels), settings.default_harness_model.stored)
  ) {
    dirty.add("harness-models");
  }
  if (values.defaultSandbox !== settings.default_sandbox.effective) dirty.add("default-sandbox");
  return dirty;
}

type SaveState =
  | { status: "idle" }
  | { status: "saving" }
  | { status: "saved" }
  | { status: "error"; message: string };

/**
 * The one panel that still opens over Settings (#691, user decision 2026-09-04): the skill
 * bank. Browsing a folder, reading a skill, importing, updating from source is a journey of
 * its own, not a settings field — every other former drill-down is an inline section now.
 */
type DrawerKind = "skills";

const RAIL_WIDTH = 176;

/**
 * Instance-wide settings (#129, ADR-0015; full-window surface since #690 / CONTEXT.md
 * « Surface Settings »): the shell Stats uses, a rail of four categories, and per category
 * a scrollable page cut into sections that a second column lists and scroll-spies.
 *
 * Agents and Sandbox & worktrees (#691) mix two kinds of sections: fields of the instance
 * form (Save), and inline **panels with their own persistence** — agent profiles, staging
 * profiles, worktree provisioning — whose every edit is written at once (`saves as you go`
 * badge). A panel's write never touches the dirty set and Save never sends panel data; the
 * footer Save is disabled while the form is clean so the two never compete for the eye.
 *
 * One explicit **Save** for the whole instance form, visible on every category. The draft
 * lives here, in the outer component, as **overrides** on top of the loaded settings: a
 * category switch renders another page but never touches the draft, and the dirty set is
 * derived — per field (amber border), per section (sub-column dot), per category (rail
 * dot) — from the same comparisons the PUT payload uses.
 *
 * Precedence is `stored → env → default`: a stored value wins, so this page is
 * authoritative. It discloses a shadowed env var (D6) rather than ignoring it, and
 * validates fail-fast (D7) client-side, with the daemon's `400` surfaced next to Save.
 */
export default function SettingsSurface({
  open,
  onClose,
  liveSessions = 0,
  onSaved,
  initialPosition,
  onOpenStats,
}: Props) {
  const { settings, save, refresh } = useSettings(open);
  const { profiles: agentProfiles, refresh: refreshAgentProfiles } = useAgentProfiles(open);
  const { bank: skillBank, loaded: skillsLoaded, refresh: refreshSkills } = useSkillBank(open);

  // Position — survives a close (the surface is unmounted-by-render below, so this state
  // IS the page-session memory, story 17). A reload remounts and lands on General.
  const [category, setCategory] = useState<SettingsCategoryId>(
    initialPosition?.category ?? "general",
  );
  const [lastSection, setLastSection] = useState<
    Partial<Record<SettingsCategoryId, SettingsSectionId>>
  >(() =>
    initialPosition?.section
      ? { [initialPosition.category]: initialPosition.section }
      : {},
  );

  // The draft: overrides on the loaded settings. `{}` = clean.
  const [draft, setDraft] = useState<Partial<DraftValues>>({});
  const [saveState, setSaveState] = useState<SaveState>({ status: "idle" });
  const [drawer, setDrawer] = useState<DrawerKind | null>(null);
  const [confirmClose, setConfirmClose] = useState(false);
  const saveButtonRef = useRef<HTMLButtonElement>(null);
  const savedTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(
    () => () => {
      if (savedTimer.current) clearTimeout(savedTimer.current);
    },
    [],
  );

  const seeded = useMemo(() => (settings ? seedFrom(settings) : null), [settings]);
  const values: DraftValues | null = useMemo(
    () => (seeded ? { ...seeded, ...draft } : null),
    [seeded, draft],
  );
  const rollup: DirtyRollup = useMemo(
    () => rollupDirty(values && settings ? computeDirty(values, settings) : new Set()),
    [values, settings],
  );
  const isDirty = rollup.fields.size > 0;

  const setField = useCallback(
    <K extends keyof DraftValues>(key: K, value: DraftValues[K]) => {
      setDraft((current) => ({ ...current, [key]: value }));
      setSaveState((current) => (current.status === "saved" ? { status: "idle" } : current));
    },
    [],
  );

  const missingDefaultProfile =
    !!values &&
    !!settings &&
    values.defaultSandbox !== "off" &&
    values.defaultSandbox !== "" &&
    !settings.sandbox_profiles.some((p) => p.name === values.defaultSandbox);

  /** Validate and build the PUT payload from the draft. `null` + error when invalid. */
  const buildPatch = (): { patch: UpdateSettingsRequest } | { error: string } => {
    if (!values || !settings) return { patch: {} };
    if (missingDefaultProfile) {
      return {
        error:
          `No staging profile named \`${values.defaultSandbox}\` any more. Pick another one (or ` +
          "`off`) — a Run that falls back to a missing profile fails at launch.",
      };
    }
    const patch: UpdateSettingsRequest = {};

    const capT = values.capStr.trim();
    const cap = Number(capT);
    if (capT === "" || !Number.isInteger(cap) || cap < 1) {
      if (capT !== "" || rollup.fields.has("session-cap")) {
        return { error: "Session cap must be a whole number ≥ 1." };
      }
    } else if (cap !== settings.session_cap.effective) patch.session_cap = cap;

    const ttlT = values.ttlStr.trim();
    const ttl = Number(ttlT);
    if (ttlT === "" || !Number.isInteger(ttl) || ttl < 1) {
      if (ttlT !== "" || rollup.fields.has("reaper-ttl")) {
        return { error: "Reaper TTL must be a whole number ≥ 1 second." };
      }
    } else if (ttl !== settings.reaper_ttl_secs.effective) patch.reaper_ttl_secs = ttl;

    const guardT = values.guardStr.trim();
    const guard = Number(guardT);
    if (guardT === "" || !Number.isInteger(guard) || guard < 1 || guard > 600) {
      if (guardT !== "" || rollup.fields.has("guard-timeout")) {
        return { error: "Guard timeout must be a whole number between 1 and 600 seconds." };
      }
    } else if (guard !== settings.guard_timeout_secs.effective) {
      patch.guard_timeout_secs = guard;
    }

    // Model: `null` (Default) clears via the "" sentinel; a string sets it.
    if (values.model !== settings.default_model.effective) {
      patch.default_model = values.model ?? "";
    }
    // #550: the harness axis. `""` clears the default harness (same sentinel as the model).
    if (values.defaultHarness !== (settings.default_harness.effective ?? "")) {
      patch.default_harness = values.defaultHarness;
    }
    if (JSON.stringify(values.agentChoice) !== JSON.stringify(settings.agent_choice ?? null)) {
      patch.agent_choice = values.agentChoice;
    }
    // #669: sent whole when the id list changed (an empty list clears the tier).
    const storedSkillIds = (settings.skills ?? []).map((skill) => skill.id).join("\u0000");
    if (values.instanceSkills.map((skill) => skill.id).join("\u0000") !== storedSkillIds) {
      patch.skills = values.instanceSkills;
    }
    // #616 (correctif 1): the per-harness map is sent WHOLE, so a harness whose row was
    // not touched (or not even shown) keeps its stored default.
    const nextModels = normaliseHarnessModels(values.harnessModels);
    if (!sameMap(nextModels, settings.default_harness_model.stored)) {
      patch.default_harness_model = nextModels;
    }
    // Default sandbox (#410): a concrete variant, only sent when it changed.
    if (values.defaultSandbox !== settings.default_sandbox.effective) {
      patch.default_sandbox = values.defaultSandbox;
    }
    // Plain bools (#469, #338): `false` PERSISTS as a stored `0` — never a clear sentinel.
    if (values.autocompleteTurnEnd !== settings.autocomplete_turn_end.effective) {
      patch.autocomplete_turn_end = values.autocompleteTurnEnd;
    }
    if (values.defaultAutoName !== settings.default_auto_name.effective) {
      patch.default_auto_name = values.defaultAutoName;
    }
    return { patch };
  };

  /** PUT the changed fields. Resolves `true` when the draft is clean afterwards. */
  const handleSave = async (): Promise<boolean> => {
    if (saveState.status === "saving") return false;
    const built = buildPatch();
    if ("error" in built) {
      setSaveState({ status: "error", message: built.error });
      saveButtonRef.current?.focus();
      return false;
    }
    if (Object.keys(built.patch).length === 0) {
      setSaveState({ status: "idle" });
      return true;
    }
    setSaveState({ status: "saving" });
    try {
      await save(built.patch);
      if (built.patch.skills) announceSkillTiersChanged();
      announceSettingsChanged();
      onSaved?.();
      setDraft({});
      setSaveState({ status: "saved" });
      if (savedTimer.current) clearTimeout(savedTimer.current);
      savedTimer.current = setTimeout(() => {
        setSaveState((current) => (current.status === "saved" ? { status: "idle" } : current));
      }, 2000);
      return true;
    } catch (e) {
      setSaveState({ status: "error", message: e instanceof Error ? e.message : String(e) });
      saveButtonRef.current?.focus();
      return false;
    }
  };

  // The surface is unmounted-by-render (`if (!open) return null`), so its state SURVIVES
  // a close: the position on purpose (story 17), the draft and the drawer on purpose NOT.
  const closeNow = () => {
    setDraft({});
    setDrawer(null);
    setConfirmClose(false);
    setSaveState({ status: "idle" });
    onClose();
  };

  /** ✕, Cancel, Escape: guarded by the dirty draft (story 16). */
  const requestClose = () => {
    if (isDirty) setConfirmClose(true);
    else closeNow();
  };

  // Escape order: tooltip (Radix, before us) → skill bank if open → confirm-close if dirty
  // → close.
  const onEscape = () => {
    if (confirmClose) {
      setConfirmClose(false);
      return;
    }
    if (drawer) {
      setDrawer(null);
      return;
    }
    requestClose();
  };

  if (!open) return null;

  const rail = SETTINGS_CATEGORIES.map((item) => ({
    id: item.id,
    label: item.label,
    dirty: rollup.categories.has(item.id),
  }));

  const dirtyCategory = SETTINGS_CATEGORIES.find((item) => rollup.categories.has(item.id));
  const dirtyCount = rollup.fields.size;
  const footerText =
    saveState.status === "saved"
      ? "Saved"
      : isDirty && dirtyCategory
        ? `Unsaved changes in ${dirtyCategory.label} (${dirtyCount} field${dirtyCount === 1 ? "" : "s"})`
        : "No unsaved changes";

  const catalog = settings ? harnessCatalog(settings.harness_descriptors) : null;

  const rememberSection = (cat: SettingsCategoryId, section: SettingsSectionId) =>
    setLastSection((current) =>
      current[cat] === section ? current : { ...current, [cat]: section },
    );

  const footer = (
    <div
      className="flex items-center gap-3 border-t border-line px-4 py-3"
      data-testid="settings-footer"
    >
      <span
        className={isDirty || saveState.status === "saved" ? "text-st-await" : "text-fg-4"}
        style={{ fontSize: "11px" }}
        data-testid="settings-footer-status"
        data-dirty={isDirty ? "true" : undefined}
      >
        {footerText}
      </span>
      <div className="ml-auto flex items-center gap-2">
        {saveState.status === "error" && (
          <span
            role="alert"
            className="text-st-failed"
            style={{ fontSize: "11px" }}
            data-testid="settings-error"
          >
            Save failed: {saveState.message}
          </span>
        )}
        <button
          type="button"
          onClick={requestClose}
          data-testid="settings-cancel"
          className="rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 transition-colors hover:bg-bg-4"
          style={{ fontSize: "11.5px" }}
        >
          Cancel
        </button>
        <button
          ref={saveButtonRef}
          type="button"
          onClick={() => void handleSave()}
          // Disabled while the form is clean (#691): on a clean page the only enabled primary
          // is a panel's own. A dangling stored default keeps it clickable so the blocker
          // is read next to the button, not guessed.
          disabled={
            saveState.status === "saving" || !settings || (!isDirty && !missingDefaultProfile)
          }
          className="rounded-md bg-acc px-3 py-1.5 font-medium text-[#04140d] transition-colors hover:bg-acc-dim disabled:opacity-40"
          style={{ fontSize: "11.5px" }}
          data-testid="settings-save"
        >
          {saveState.status === "saving" ? "Saving…" : "Save"}
        </button>
      </div>
    </div>
  );

  const drawerNode = drawer ? (
    <SettingsDrawer onClose={() => setDrawer(null)}>
      <SkillBankPanel
        bank={skillBank}
        loaded={skillsLoaded}
        home={settings?.home ?? null}
        onChanged={async () => {
          await refreshSkills();
          announceSkillsChanged();
        }}
      />
    </SettingsDrawer>
  ) : null;

  const loading = (
    <div
      className="px-1 py-4 text-fg-4"
      style={{ fontSize: "12px" }}
      data-testid="settings-loading"
    >
      Loading…
    </div>
  );

  const capPreview = values ? Number(values.capStr.trim()) : NaN;
  const capForPreview =
    settings && Number.isInteger(capPreview) && capPreview >= 1
      ? capPreview
      : settings?.session_cap.effective ?? 0;

  return (
    <>
      <FullWindowShell
        title="Settings"
        testId="settings-surface"
        onClose={requestClose}
        onEscape={onEscape}
        closeLabel="Close settings"
        rail={rail}
        activeRail={category}
        onRailChange={(id) => setCategory(id as SettingsCategoryId)}
        railWidth={RAIL_WIDTH}
        railAriaLabel="Settings categories"
        railTestIdPrefix="settings-category"
        headerActions={
          <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
            Esc closes · ↑↓ on the rail switch category
          </span>
        }
        footer={footer}
        drawer={drawerNode}
      >
        {SETTINGS_CATEGORIES.map((item) => (
          <CategoryPage
            key={item.id}
            categoryId={item.id}
            active={category === item.id}
            initialSection={lastSection[item.id]}
            highlight={initialPosition?.category === item.id && !!initialPosition.section}
            dirtySections={rollup.sections}
            onSectionChange={(section) => rememberSection(item.id, section)}
          >
            {item.id === "general" && (
              <>
                <InterfaceSection section={item.sections[0]} />
                {values && settings ? (
                  <>
                    <Section section={item.sections[1]}>
                      <SettingRow
                        id="session-cap"
                        label="Concurrent session cap"
                        help="Max live NodeRun sessions daemon-wide. Kept below the tmux-collapse zone (#77/#78)."
                        value={values.capStr}
                        onChange={(v) => setField("capStr", v)}
                        dirty={rollup.fields.has("session-cap")}
                        field={settings.session_cap}
                        envVar="PDO_SESSION_CAP"
                        unit=""
                        inline={
                          <span
                            className="flex items-center gap-1.5 text-fg-4"
                            style={{ fontSize: "10.5px" }}
                          >
                            live <SessionCounter live={liveSessions} cap={capForPreview} />
                          </span>
                        }
                      >
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
                      <SettingRow
                        id="reaper-ttl"
                        label="Reaper TTL (seconds)"
                        help="Seconds after a node completes before its idle tmux session is reaped. Sweep runs every 60 s, so values below ~60 s add little."
                        value={values.ttlStr}
                        onChange={(v) => setField("ttlStr", v)}
                        dirty={rollup.fields.has("reaper-ttl")}
                        field={settings.reaper_ttl_secs}
                        envVar="PDO_REAPER_TTL_SECS"
                        unit=" s"
                      />
                      <SettingRow
                        id="guard-timeout"
                        label="Trigger guard timeout (seconds)"
                        help="Hard timeout for a Trigger guard command. 1–600 s."
                        value={values.guardStr}
                        onChange={(v) => setField("guardStr", v)}
                        dirty={rollup.fields.has("guard-timeout")}
                        field={settings.guard_timeout_secs}
                        envVar="PDO_GUARD_TIMEOUT_MS"
                        unit=" ms"
                        envIsMs
                      />
                    </Section>
                    <Section section={item.sections[2]}>
                      {/* Default Run auto-naming (#338). On by default — the pre-#338
                          behaviour. */}
                      <CheckboxRow
                        id="default-auto-name"
                        checked={values.defaultAutoName}
                        onChange={(v) => setField("defaultAutoName", v)}
                        dirty={rollup.fields.has("default-auto-name")}
                        label="Let the manager auto-name a Run created without a name"
                        help={
                          <>
                            When a Run starts with no name — from the New Run dialog with
                            "Auto-generated" left on, or from a Trigger — the Pipeline Manager
                            gives it a short descriptive name. Turn this off and such a Run
                            keeps a stable <span className="font-mono">Untitled run …</span>{" "}
                            placeholder instead. This is only the <strong>default</strong>: the
                            New Run box and each Trigger can override it.
                          </>
                        }
                        source={defaultAutoNameSourceNote(settings.default_auto_name)}
                      />
                      {/* Turn-end auto-completion (#469). Labelled on what is MEASURED — an
                          end of turn constated in the agent's transcript — and deliberately
                          not on a duration. Off by default (ADR-0012). */}
                      <CheckboxRow
                        id="autocomplete-turn-end"
                        checked={values.autocompleteTurnEnd}
                        onChange={(v) => setField("autocompleteTurnEnd", v)}
                        dirty={rollup.fields.has("autocomplete-turn-end")}
                        label="Try to auto-complete a node when its agent has clearly finished its turn"
                        help={
                          <>
                            When an agent stops without running{" "}
                            <span className="font-mono">pdo complete</span>, PDO can finish the
                            node for it — but only when its transcript shows the turn actually
                            ended (no tool call in flight) <strong>and</strong> its declared
                            outputs validate. An agent still inside a long tool call, waiting on
                            a reply, or stopped to ask a question is never touched. Leave this off
                            and such a node waits for you.
                          </>
                        }
                        source={autocompleteSourceNote(settings.autocomplete_turn_end)}
                      />
                    </Section>
                    <VersionUpdateSection section={item.sections[3]} />
                  </>
                ) : (
                  loading
                )}
              </>
            )}

            {item.id === "agents" &&
              (values && settings && catalog ? (
                <>
                <Section section={item.sections[0]}>
                  <AgentControl
                    choice={values.agentChoice}
                    onChange={(choice) => setField("agentChoice", choice)}
                    profiles={agentProfiles}
                    catalog={catalog}
                    inherited={{ harness: "claude", model: null, effort: null }}
                    allowInherit={false}
                    label="Agent — Instance settings"
                    testId="instance-agent-control"
                  />
                  {/* Default harness (#550/ADR-0046). Precedence at spawn:
                      node → Run → Projet → instance (this) → claude floor. */}
                  <FieldBlock
                    label="Default harness"
                    testId="setting-default-harness"
                    dirty={rollup.fields.has("default-harness")}
                    help={
                      <>
                        Every new node runs on this harness unless it pins its own. "Default"
                        leaves it to the <span className="font-mono">claude</span> floor.
                      </>
                    }
                    source={modelSourceNote(settings.default_harness)}
                    sourceTestId="setting-source-default-harness"
                  >
                    <HarnessSelect
                      value={values.defaultHarness}
                      onChange={(v) => setField("defaultHarness", v)}
                      catalog={catalog}
                      inheritLabel="Default (claude floor)"
                      data-testid="setting-default-harness-select"
                      className={`w-full rounded border bg-bg-3 px-2 py-1 text-fg ${
                        rollup.fields.has("default-harness") ? "border-st-await" : "border-line-strong"
                      }`}
                      style={{ fontSize: "11px" }}
                    />
                  </FieldBlock>
                  {/* Default model (#347): folds under `claude` at resolve (#616). */}
                  <FieldBlock
                    label="Default model"
                    dirty={rollup.fields.has("default-model")}
                    help={
                      <>
                        The model every work node launches with unless it sets its own.
                        "Default" leaves it to your Claude account (no{" "}
                        <span className="font-mono">--model</span>).
                      </>
                    }
                    source={modelSourceNote(settings.default_model)}
                    sourceTestId="setting-source-default-model"
                  >
                    <ModelPicker
                      value={values.model}
                      onChange={(v) => setField("model", v)}
                      models={findHarnessOption(catalog, "claude")?.models ?? []}
                      testid="default-model"
                      subject="instance-default"
                    />
                  </FieldBlock>
                  {/* Default model per harness (#550, #616 correctif 1): rows DERIVED from
                      the served harness list; saving sends the map whole. */}
                  <FieldBlock
                    label="Default model per harness"
                    testId="setting-default-harness-model"
                    dirty={rollup.fields.has("harness-models")}
                    help="The model a node runs with on that harness when it sets none. Empty = the harness account default."
                  >
                    {[...catalog.builtin, ...catalog.descriptors].map((h) => (
                      <label
                        key={h.name}
                        className="flex items-center gap-2 text-fg-3"
                        style={{ fontSize: "11px" }}
                      >
                        <span className="w-16 font-mono">{h.name}</span>
                        <input
                          type="text"
                          value={values.harnessModels[h.name] ?? ""}
                          onChange={(e) =>
                            setField("harnessModels", {
                              ...values.harnessModels,
                              [h.name]: e.target.value,
                            })
                          }
                          placeholder="account default"
                          data-testid={`setting-default-model-${h.name}`}
                          className={`flex-1 rounded border bg-bg-3 px-2 py-1 text-fg ${
                            rollup.fields.has("harness-models")
                              ? "border-st-await"
                              : "border-line-strong"
                          }`}
                        />
                      </label>
                    ))}
                  </FieldBlock>
                </Section>
                <Section section={item.sections[1]}>
                  {/* Inline, list-first (#691): its own REST resource, its own buttons. */}
                  <AgentProfilesPanel
                    key={agentProfiles[0]?.id ?? "loading"}
                    profiles={agentProfiles}
                    onChanged={async () => {
                      await refreshAgentProfiles();
                      announceAgentProfilesChanged();
                      onSaved?.();
                    }}
                  />
                </Section>
                <Section section={item.sections[2]}>
                  {/* #669/ADR-0062: the instance tier of the skills selection — part of the
                      instance form (Save), unlike the bank below it. */}
                  <SkillSelector
                    tier="instance"
                    own={values.instanceSkills}
                    onChange={(skills) => setField("instanceSkills", skills)}
                    bank={skillBank}
                    label="Skills — Instance settings"
                    testId="instance-skill-selector"
                  />
                  {/* The bank is a journey of its own (browse, read, import, update from
                      source): it opens as its own surface, not inline. */}
                  <div
                    className="flex items-center justify-between gap-3 rounded-md border border-line bg-bg-3/40 px-3 py-2.5"
                    data-testid="setting-skill-bank-card"
                  >
                    <div className="flex flex-col gap-0.5">
                      <span className="font-medium text-fg-2" style={{ fontSize: "11.5px" }}>
                        Skill bank
                      </span>
                      <span className="text-fg-4" style={{ fontSize: "10.5px" }} data-testid="setting-skills-count">
                        {skillsLoaded
                          ? `${skillBank.skills.length} skill${skillBank.skills.length === 1 ? "" : "s"} · ${skillBank.folders.length} folder${skillBank.folders.length === 1 ? "" : "s"} · ~/.pdo/skills`
                          : ""}
                      </span>
                    </div>
                    <button
                      type="button"
                      data-testid="setting-open-skill-bank"
                      onClick={() => setDrawer("skills")}
                      className="flex items-center gap-1.5 rounded-md border border-line-strong bg-bg-3 px-2.5 py-1.5 text-fg-2 transition-colors hover:border-acc"
                      style={{ fontSize: 11 }}
                    >
                      <FileText size={11} />
                      Open skill bank
                    </button>
                  </div>
                </Section>
                </>
              ) : (
                loading
              ))}

            {item.id === "sandbox" &&
              (values && settings ? (
                <>
                <Section section={item.sections[0]}>
                  {/* Default sandbox (#410/#432): options are DATA — `off` plus the
                      instance's staging profiles — and a stored name can dangle. */}
                  <FieldBlock
                    label="Default sandbox"
                    htmlFor="setting-default-sandbox"
                    dirty={rollup.fields.has("default-sandbox")}
                    help={
                      <>
                        What a Run uses when neither the launch dialog nor a firing Trigger
                        picks one. <span className="font-mono">off</span> runs on the host; a{" "}
                        <span className="font-mono">staging profile</span> runs it inside a
                        Docker sandbox with that profile's home content (requires Docker).
                      </>
                    }
                    source={defaultSandboxSourceNote(settings.default_sandbox)}
                    sourceTestId="setting-source-default-sandbox"
                  >
                    <select
                      id="setting-default-sandbox"
                      data-testid="setting-default-sandbox"
                      value={values.defaultSandbox}
                      onChange={(e) => setField("defaultSandbox", e.target.value)}
                      className={`w-full rounded-md border bg-bg-3 px-2.5 py-1.5 font-mono text-fg transition-colors focus:border-acc focus:outline-none ${
                        rollup.fields.has("default-sandbox") ? "border-st-await" : "border-line-strong"
                      }`}
                      style={{ fontSize: "12px" }}
                    >
                      <option value="off">off (run on the host)</option>
                      {settings.sandbox_profiles.map((p) => (
                        <option key={p.name} value={p.name}>
                          {p.name}
                          {p.virtual ? " (built-in)" : ""}
                        </option>
                      ))}
                      {/* Tombstone: a seeded value is NEVER silently rewritten. */}
                      {missingDefaultProfile && (
                        <option
                          value={values.defaultSandbox}
                          data-testid="setting-default-sandbox-missing"
                        >
                          {values.defaultSandbox} — missing
                        </option>
                      )}
                    </select>
                    {/* Server-supplied: a `PDO_DEFAULT_SANDBOX` naming a vanished profile is
                        only visible here. */}
                    {settings.default_sandbox.reason && (
                      <div
                        className="text-st-failed"
                        style={{ fontSize: "10.5px" }}
                        data-testid="setting-default-sandbox-reason"
                      >
                        {settings.default_sandbox.reason}
                      </div>
                    )}
                  </FieldBlock>
                </Section>
                <Section section={item.sections[1]}>
                  {/* Inline (#691), natural height: the page scrolls, not the panel. */}
                  <div className="rounded-md border border-line bg-bg-3/40">
                    <StagingProfilesPanel
                      home={settings?.home ?? null}
                      // Refetch `GET /settings` so the Default-sandbox `<select>` above sees the
                      // new name list (and a freshly dangling `reason`) without a reopen, and
                      // tell New Run (mounted underneath) the same thing.
                      onChanged={() => {
                        void refresh();
                        announceSettingsChanged();
                        onSaved?.();
                      }}
                    />
                  </div>
                </Section>
                <Section section={item.sections[2]}>
                  {/* The instance-scope editor IS the section; its own Save provisioning stays. */}
                  <PersistedProvisioningEditor scope="instance" />
                </Section>
                </>
              ) : (
                loading
              ))}

            {item.id === "diagnostics" &&
              (settings ? (
                <>
                  <Section section={item.sections[0]}>
                    <PriceTableRows settings={settings} onOpenStats={onOpenStats} />
                  </Section>
                  <Section section={item.sections[1]}>
                    <HarnessDescriptorRows settings={settings} />
                  </Section>
                </>
              ) : (
                loading
              ))}
          </CategoryPage>
        ))}
      </FullWindowShell>

      {confirmClose && (
        <ConfirmCloseDialog
          rollup={rollup}
          saving={saveState.status === "saving"}
          onKeep={() => setConfirmClose(false)}
          onDiscard={closeNow}
          onSaveAndClose={async () => {
            const ok = await handleSave();
            if (ok) closeNow();
            else setConfirmClose(false);
          }}
        />
      )}
    </>
  );
}

/* ------------------------------------------------------------------------------------ */
/* Page, sections, sub-column                                                            */
/* ------------------------------------------------------------------------------------ */

function CategoryPage({
  categoryId,
  active,
  initialSection,
  highlight,
  dirtySections,
  onSectionChange,
  children,
}: {
  categoryId: SettingsCategoryId;
  active: boolean;
  initialSection?: SettingsSectionId;
  /** Programmatic open (story 18): pulse the landed section so the eye finds it. */
  highlight?: boolean;
  dirtySections: Set<SettingsSectionId>;
  onSectionChange: (section: SettingsSectionId) => void;
  children: ReactNode;
}) {
  const category = findCategory(categoryId);
  const sectionIds = useMemo(() => category.sections.map((s) => s.id), [category]);
  const scrollRef = useRef<HTMLDivElement>(null);
  const { active: activeSection, scrollTo } = useScrollSpy(sectionIds, scrollRef, active);
  const restored = useRef(false);

  // Land on the remembered / requested section (stories 17 and 18) — once, instantly,
  // before paint. No dependency list on purpose: the sections mount after `GET /settings`
  // resolves, so the effect re-checks on each render until the target exists, then stops.
  // The target is captured once: while the page still shows "loading", the spy reports the
  // first section and the parent remembers it, which would silently replace the request.
  const target = useRef(initialSection);
  const landedTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useLayoutEffect(() => {
    if (!active || restored.current) return;
    const wanted = target.current;
    if (!wanted || wanted === sectionIds[0]) {
      restored.current = true;
      return;
    }
    const el = scrollRef.current?.querySelector<HTMLElement>(`[data-section-id="${wanted}"]`);
    if (!el) return;
    restored.current = true;
    // Instant, not smooth: the panels below still grow while they load, and a smooth scroll
    // racing that growth ends short. The spy re-picks on the content's ResizeObserver.
    el.scrollIntoView?.({ behavior: "auto", block: "start" });
    scrollTo(wanted);
    if (highlight) {
      // Pulse once (2.2 s, `index.css`): only on a programmatic open, never on a click.
      el.dataset.landed = "true";
      landedTimer.current = setTimeout(() => {
        delete el.dataset.landed;
      }, 2200);
    }
  });
  useEffect(
    () => () => {
      if (landedTimer.current) clearTimeout(landedTimer.current);
    },
    [],
  );

  useEffect(() => {
    if (active) onSectionChange(activeSection);
  }, [active, activeSection, onSectionChange]);

  return (
    <div
      hidden={!active}
      className={active ? "flex min-h-0 min-w-0 flex-1" : undefined}
      data-testid={`settings-page-${categoryId}`}
    >
      <nav
        className="w-[168px] shrink-0 border-r border-line px-3 py-4"
        aria-label={`${category.label} sections`}
        data-testid="settings-subcolumn"
      >
        <div
          className="mb-2 px-2 uppercase tracking-wider text-fg-4"
          style={{ fontSize: "10px" }}
        >
          {category.label}
        </div>
        <ul className="flex flex-col gap-0.5">
          {category.sections.map((section) => {
            const isActive = activeSection === section.id;
            const dirty = dirtySections.has(section.id);
            return (
              <li key={section.id}>
                <button
                  type="button"
                  onClick={() => scrollTo(section.id)}
                  aria-current={isActive ? "true" : undefined}
                  data-testid={`settings-section-${section.id}`}
                  data-dirty={dirty ? "true" : undefined}
                  className={`flex w-full items-center justify-between gap-2 border-l-2 px-2 py-1 text-left ${
                    isActive
                      ? "border-acc bg-acc-bg text-fg"
                      : "border-transparent text-fg-3 hover:text-fg-2"
                  }`}
                  style={{ fontSize: "11.5px" }}
                >
                  <span>{section.label}</span>
                  {dirty && (
                    <span
                      aria-label="Unsaved changes"
                      data-testid={`settings-section-${section.id}-dirty`}
                      className="h-1.5 w-1.5 shrink-0 rounded-full bg-st-await"
                    />
                  )}
                </button>
              </li>
            );
          })}
        </ul>
      </nav>
      <div
        ref={scrollRef}
        className="min-w-0 flex-1 overflow-y-auto px-7 py-5"
        data-testid={`settings-scroll-${categoryId}`}
      >
        <div className="flex max-w-[640px] flex-col gap-7 pb-10">{children}</div>
      </div>
    </div>
  );
}

function Section({ section, children }: { section: SettingsSection; children: ReactNode }) {
  return (
    <section
      id={`settings-section-${section.id}`}
      data-section-id={section.id}
      data-testid={`settings-section-body-${section.id}`}
      className="settings-section flex flex-col gap-4"
      style={{ scrollMarginTop: 16 }}
    >
      <div className="flex flex-col gap-1">
        <h3 className="flex items-center gap-2 font-semibold text-fg" style={{ fontSize: "13px" }}>
          {section.label}
          {section.readOnly && (
            <span
              className="rounded bg-bg-3 px-1.5 py-0.5 font-normal text-fg-4"
              style={{ fontSize: "9.5px" }}
            >
              read-only
            </span>
          )}
          {section.ownPersistence && (
            <span
              className="rounded bg-bg-3 px-1.5 py-0.5 font-normal text-fg-4"
              style={{ fontSize: "9.5px" }}
              title="Each edit is written when you make it. The Save button below does not apply here."
            >
              saves as you go
            </span>
          )}
        </h3>
        <p className="text-fg-4" style={{ fontSize: "10.5px" }}>
          {section.description}
        </p>
      </div>
      {children}
    </section>
  );
}

/**
 * Per-client UI preferences (#342). The single-tab toggle persists to localStorage AT
 * THE CHANGE via `setSingleTabMode` (Trap B) — NOT behind the instance form's Save, and
 * never part of the dirty set. Rendered even when `GET /settings` failed (Trap A).
 */
function InterfaceSection({ section }: { section: SettingsSection }) {
  const singleTabMode = useEditStore((s) => s.singleTabMode);
  const setSingleTabMode = useEditStore((s) => s.setSingleTabMode);

  return (
    <Section section={section}>
      <div className="flex flex-col gap-1.5">
        <div className="flex items-center gap-2">
          <span className="font-medium text-fg-2" style={{ fontSize: "11.5px" }}>
            Single-tab mode
          </span>
          <span
            className="rounded-full border border-acc-border bg-acc-bg px-2 py-0.5 text-acc"
            style={{ fontSize: "9.5px" }}
            data-testid="setting-tabs-disabled-badge"
          >
            Device-local · saved immediately
          </span>
        </div>
        <div className="flex items-center gap-2.5">
          <button
            type="button"
            role="switch"
            aria-checked={singleTabMode}
            aria-label="Single-tab mode"
            data-testid="setting-tabs-disabled"
            onClick={() => setSingleTabMode(!singleTabMode)}
            className={`relative h-3.5 w-6 shrink-0 rounded-full transition-colors ${
              singleTabMode ? "bg-acc" : "bg-fg-5"
            }`}
          >
            <span
              className={`absolute top-0.5 h-2.5 w-2.5 rounded-full bg-bg-1 transition-all ${
                singleTabMode ? "left-3" : "left-0.5"
              }`}
            />
          </button>
          <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
            Opening a pipeline or run replaces the current tab instead of stacking a new one.
            Enabling it closes the other open tabs.
          </span>
        </div>
        <div className="text-fg-3" style={{ fontSize: "10.5px" }}>
          Stored in this browser's localStorage. Not shared with other browsers or the daemon.
        </div>
      </div>
    </Section>
  );
}

/**
 * DESIGN PROTOTYPE (#697) — Version & update. Read view of the daemon's version check +
 * two controls: "Check now" (immediate POST) and the `update_check` toggle. In the
 * prototype the toggle saves at the change (own persistence) so the FP reads
 * "disable → latest becomes —, badge disappears" without a Save round-trip; whether it
 * should instead join the form's dirty set is an open design question.
 */
function VersionUpdateSection({ section }: { section: SettingsSection }) {
  const [status, setStatus] = useState<UpdateStatus>(() => mockUpdateStatus());
  const [checking, setChecking] = useState(false);
  const [checkError, setCheckError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const newer = newerAvailable(status);
  const upToDate =
    status.check_enabled && status.latest_version != null && status.latest_version === status.installed_version;

  const checkNow = () => {
    setChecking(true);
    setCheckError(null);
    window.setTimeout(() => {
      setChecking(false);
      if (PROTO_SCENARIO === "offline") {
        setCheckError("Release source unreachable (github.com timed out after 5 s). Last known values kept.");
        setStatus((s) => ({ ...s, checked_at: new Date().toISOString() }));
      } else {
        setStatus((s) => ({ ...s, ...mockUpdateStatus("newer"), check_enabled: s.check_enabled, checked_at: new Date().toISOString(), reason: null }));
      }
    }, 900);
  };
  const toggle = (on: boolean) => {
    setCheckError(null);
    setStatus((s) =>
      on
        ? { ...s, check_enabled: true, reason: "Not checked yet since re-enabling." }
        : { ...s, check_enabled: false, latest_version: null, reason: "Update check is off." },
    );
  };
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(status.manual_command);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      /* clipboard unavailable: the command is selectable text anyway */
    }
  };

  const methodLabel: Record<UpdateStatus["install_method"], string> = {
    homebrew: "Homebrew",
    script: "Install script (cargo-dist)",
    unknown: "Unknown",
  };
  const supervisionLabel: Record<UpdateStatus["supervision"], string> = {
    systemd: "systemd service",
    launchd: "launchd agent",
    none: "manual (no service)",
  };

  return (
    <Section section={section}>
      <div className="flex flex-col gap-1.5" data-testid="setting-version-update">
        <KeyValueRow label="Installed version" testId="setting-version-installed">
          <span className="font-mono">v{status.installed_version}</span>
          {upToDate && (
            <span className="ml-2 rounded-full bg-st-done-bg px-1.5 py-px text-st-done" style={{ fontSize: "9.5px" }}>
              up to date
            </span>
          )}
        </KeyValueRow>
        <KeyValueRow label="Latest release" testId="setting-version-latest">
          {status.latest_version ? (
            <>
              <span className={`font-mono ${newer ? "text-st-await" : ""}`}>v{status.latest_version}</span>
            </>
          ) : (
            <>
              <span className="font-mono">—</span>
              {status.reason && <span className="ml-2 text-fg-4">{status.reason}</span>}
            </>
          )}
        </KeyValueRow>
        <KeyValueRow label="Last check" testId="setting-version-checked-at">
          {status.checked_at ? (
            <>
              {absoluteTime(status.checked_at)}
              <span className="text-fg-4"> · {relativeTime(status.checked_at)} · {status.source}</span>
            </>
          ) : (
            <span className="font-mono">—</span>
          )}
        </KeyValueRow>
        <KeyValueRow label="Install method" testId="setting-version-install-method">
          {methodLabel[status.install_method]}
          <span className="text-fg-4"> · {supervisionLabel[status.supervision]}</span>
        </KeyValueRow>
        <div
          className="flex items-center justify-between gap-3 rounded border border-line bg-bg-2 px-2.5 py-1.5"
          style={{ fontSize: "10.5px" }}
          data-testid="setting-version-manual-command"
        >
          <div className="min-w-0 flex flex-col gap-0.5">
            <span className="text-fg-4">
              {status.install_method === "unknown"
                ? "Install method not detected — PDO will not update itself. To update:"
                : "To update manually (the future Update button runs exactly this):"}
            </span>
            <code className="truncate font-mono text-fg-2">{status.manual_command}</code>
          </div>
          {status.install_method !== "unknown" && (
            <button
              type="button"
              onClick={copy}
              className="flex shrink-0 items-center gap-1 rounded border border-line-strong bg-bg-3 px-2 py-1 text-fg-2 hover:border-acc"
              style={{ fontSize: 10.5 }}
              title="Copy command"
            >
              {copied ? <Check size={11} /> : <Copy size={11} />}
              {copied ? "Copied" : "Copy"}
            </button>
          )}
        </div>
        {checkError && (
          <KeyValueRow label="Check failed" tone="failed" testId="setting-version-check-error">
            {checkError}
          </KeyValueRow>
        )}
        <div className="mt-1 flex flex-wrap items-center gap-4">
          <button
            type="button"
            onClick={checkNow}
            disabled={checking || !status.check_enabled}
            data-testid="setting-version-check-now"
            title={!status.check_enabled ? "Turn the update check on to check now." : undefined}
            className="flex items-center gap-1.5 rounded border border-line-strong bg-bg-3 px-2.5 py-1.5 text-fg-2 hover:border-acc disabled:opacity-40"
            style={{ fontSize: 11 }}
          >
            <RefreshCw size={11} className={checking ? "animate-spin" : ""} />
            {checking ? "Checking…" : "Check now"}
          </button>
          <label className="flex cursor-pointer items-center gap-2.5">
            <button
              type="button"
              role="switch"
              aria-checked={status.check_enabled}
              aria-label="Check for updates"
              data-testid="setting-update-check"
              onClick={() => toggle(!status.check_enabled)}
              className={`relative h-3.5 w-6 shrink-0 rounded-full transition-colors ${
                status.check_enabled ? "bg-acc" : "bg-fg-5"
              }`}
            >
              <span
                className={`absolute top-0.5 h-2.5 w-2.5 rounded-full bg-bg-1 transition-all ${
                  status.check_enabled ? "left-3" : "left-0.5"
                }`}
              />
            </button>
            <span className="font-medium text-fg-2" style={{ fontSize: "11.5px" }}>
              Check for updates
            </span>
            <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
              at start, every 6 h and on demand. Off: no request ever leaves the daemon.
            </span>
          </label>
        </div>
      </div>
    </Section>
  );
}

/* ------------------------------------------------------------------------------------ */
/* Diagnostics (read-only)                                                               */
/* ------------------------------------------------------------------------------------ */

function KeyValueRow({
  label,
  children,
  tone = "neutral",
  testId,
}: {
  label: ReactNode;
  children: ReactNode;
  tone?: "neutral" | "failed";
  testId?: string;
}) {
  return (
    <div
      className={`flex items-center justify-between gap-4 rounded px-2.5 py-1.5 ${
        tone === "failed" ? "bg-st-failed-bg text-st-failed" : "bg-bg-3 text-fg-3"
      }`}
      style={{ fontSize: "10.5px" }}
      data-testid={testId}
    >
      <span className="shrink-0">{label}</span>
      <span className={`min-w-0 truncate text-right ${tone === "failed" ? "" : "text-fg-2"}`}>
        {children}
      </span>
    </div>
  );
}

/**
 * Price table (#427, ADR-0034) — READ ONLY: which of the three price tiers is in force.
 * The paths are shown even when the files are absent, because nothing is ever seeded and
 * naming them is the only way a user learns where to write. The resolved read view and
 * "Sync costs" live in Stats › Cost › Pricing details, next to the numbers they change.
 */
function PriceTableRows({
  settings,
  onOpenStats,
}: {
  settings: InstanceSettings;
  onOpenStats?: (intent: StatsOpenIntent) => void;
}) {
  const table = settings.price_table;
  // Not dead code despite the type: under `vite dev` the SPA may face an older daemon.
  if (!table) return null;
  return (
    <div className="flex flex-col gap-1.5" data-testid="setting-price-table">
      <KeyValueRow label="Manual overrides (wins)" testId="setting-price-table-manual-path">
        <span className="font-mono">{table.manual_path ?? "— (HOME unset)"}</span>
        {table.manual_keys.length > 0 && (
          <span>
            {" · "}
            {table.manual_keys.length} model{table.manual_keys.length === 1 ? "" : "s"}:{" "}
            {table.manual_keys.join(", ")}
          </span>
        )}
      </KeyValueRow>
      <KeyValueRow label="Fetched table (PDO writes this)" testId="setting-price-table-fetched-path">
        <span className="font-mono">{table.fetched_path ?? "— (HOME unset)"}</span>
        {` · ${table.fetched_rows} model${table.fetched_rows === 1 ? "" : "s"}`}
      </KeyValueRow>
      <KeyValueRow label="Last sync" testId="setting-price-table-fetched-at">
        {table.fetched_at
          ? `${table.fetched_at}${table.source ? ` from ${table.source}` : ""}`
          : "Never synced — only the built-in prices apply."}
      </KeyValueRow>
      {table.reason && (
        <KeyValueRow label="Refused" tone="failed" testId="setting-price-table-reason">
          {table.reason}
        </KeyValueRow>
      )}
      <div className="mt-1 flex flex-wrap items-center gap-3">
        <button
          type="button"
          onClick={() => onOpenStats?.({ tab: "cost", pricingOpen: true })}
          disabled={!onOpenStats}
          data-testid="settings-open-stats-pricing"
          className="flex items-center gap-1.5 rounded border border-line-strong bg-bg-3 px-2.5 py-1.5 text-fg-2 hover:border-acc disabled:opacity-40"
          style={{ fontSize: 11 }}
        >
          Open Stats › Cost › Pricing details
          <ExternalLink size={11} />
        </button>
        <span className="text-fg-3" style={{ fontSize: "10.5px" }}>
          Sync lives in Stats, next to the numbers it changes. Costs resolve{" "}
          <span className="font-mono">manual → fetched → built-in</span>.
        </span>
      </div>
    </div>
  );
}

/**
 * Harness descriptors (#553/ADR-0045) — the disk tier. Always named (nothing is seeded);
 * a broken/refused descriptor is named here, the only place, since a hand-edited file
 * passes through no validator.
 */
function HarnessDescriptorRows({ settings }: { settings: InstanceSettings }) {
  const view = settings.harness_descriptors;
  if (!view) return null;
  return (
    <div className="flex flex-col gap-1.5" data-testid="setting-harness-descriptors">
      <KeyValueRow label="Descriptor file" testId="setting-harness-descriptors-path">
        <span className="font-mono">{view.path ?? "— (HOME unset)"}</span>
      </KeyValueRow>
      <KeyValueRow label="Harnesses" testId="setting-harness-descriptors-names">
        <span className="font-mono">{view.names.join(" · ")}</span>
      </KeyValueRow>
      {(view.rejected ?? []).map((item) => (
        <KeyValueRow
          key={item.name}
          label={<span className="font-mono">{item.name}</span>}
          tone="failed"
          testId={`setting-harness-descriptor-rejected-${item.name}`}
        >
          refused: {item.why}
        </KeyValueRow>
      ))}
      {view.reason && (
        <KeyValueRow label="Refused" tone="failed" testId="setting-harness-descriptors-reason">
          {view.reason}
        </KeyValueRow>
      )}
    </div>
  );
}

/* ------------------------------------------------------------------------------------ */
/* Drawer + confirm                                                                      */
/* ------------------------------------------------------------------------------------ */

/**
 * The shell's right drawer hosting the skill bank (#690 decision 5, kept by #691 for this
 * one panel): the rail stays visible, Escape returns to Settings › Skills first, and the
 * Save footer hides under it so its own writes and Save are never both in view.
 */
function SettingsDrawer({ onClose, children }: { onClose: () => void; children: ReactNode }) {
  const kind: DrawerKind = "skills";
  return (
    <aside
      className="absolute bottom-0 right-0 top-14 z-20 flex w-[min(880px,90vw)] flex-col border-l border-line bg-bg-4 shadow-2xl"
      data-testid="settings-drawer"
      data-drawer={kind}
    >
      <div className="flex items-center gap-3 border-b border-line px-4 py-3">
        <h3 className="font-semibold text-fg" style={{ fontSize: "13px" }}>
          Skill bank
        </h3>
        <span className="ml-auto text-fg-4" style={{ fontSize: "10.5px" }}>
          saves as you go · Esc returns to Settings
        </span>
        <button
          type="button"
          onClick={onClose}
          aria-label="Close panel"
          data-testid="settings-drawer-close"
          className="grid h-6 w-6 shrink-0 place-items-center rounded text-fg-3 transition-colors hover:bg-bg-5 hover:text-fg"
        >
          <X size={14} />
        </button>
      </div>
      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">{children}</div>
    </aside>
  );
}

function ConfirmCloseDialog({
  rollup,
  saving,
  onKeep,
  onDiscard,
  onSaveAndClose,
}: {
  rollup: DirtyRollup;
  saving: boolean;
  onKeep: () => void;
  onDiscard: () => void;
  onSaveAndClose: () => void;
}) {
  const keepRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    keepRef.current?.focus();
  }, []);

  const places = SETTINGS_CATEGORIES.flatMap((category) =>
    category.sections
      .filter((section) => rollup.sections.has(section.id))
      .map((section) => `${category.label} › ${section.label}`),
  );
  const count = rollup.fields.size;

  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/50"
      onClick={onKeep}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="settings-confirm-title"
        className="w-[380px] rounded-lg border border-line bg-bg-4 p-4 shadow-xl"
        onClick={(e) => e.stopPropagation()}
        data-testid="settings-confirm-close"
      >
        <h3 id="settings-confirm-title" className="font-semibold text-fg" style={{ fontSize: "13px" }}>
          Discard unsaved changes?
        </h3>
        <p className="mt-2 text-fg-3" style={{ fontSize: "11px" }}>
          You edited{" "}
          {places.map((place, index) => (
            <span key={place}>
              {index > 0 && ", "}
              <strong className="text-fg-2">{place}</strong>
            </span>
          ))}{" "}
          ({count} field{count === 1 ? "" : "s"}). Closing Settings without saving drops the
          edit{count === 1 ? "" : "s"}.
        </p>
        <div className="mt-4 flex items-center justify-end gap-2">
          <button
            ref={keepRef}
            type="button"
            onClick={onKeep}
            data-testid="settings-confirm-keep"
            className="rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 hover:bg-bg-5"
            style={{ fontSize: "11.5px" }}
          >
            Keep editing
          </button>
          <button
            type="button"
            onClick={onDiscard}
            data-testid="settings-confirm-discard"
            className="rounded-md border border-st-failed/40 bg-st-failed-bg px-3 py-1.5 text-st-failed hover:border-st-failed"
            style={{ fontSize: "11.5px" }}
          >
            Discard
          </button>
          <button
            type="button"
            onClick={onSaveAndClose}
            disabled={saving}
            data-testid="settings-confirm-save-close"
            className="rounded-md bg-acc px-3 py-1.5 font-medium text-[#04140d] hover:bg-acc-dim disabled:opacity-40"
            style={{ fontSize: "11.5px" }}
          >
            {saving ? "Saving…" : "Save & close"}
          </button>
        </div>
      </div>
    </div>
  );
}

/* ------------------------------------------------------------------------------------ */
/* Field primitives                                                                      */
/* ------------------------------------------------------------------------------------ */

/** Label / control / help / source note — today's field anatomy, plus the dirty border. */
function FieldBlock({
  label,
  htmlFor,
  testId,
  dirty,
  help,
  source,
  sourceTestId,
  children,
}: {
  label: string;
  htmlFor?: string;
  testId?: string;
  dirty?: boolean;
  help?: ReactNode;
  source?: string;
  sourceTestId?: string;
  children: ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1.5" data-testid={testId} data-dirty={dirty ? "true" : undefined}>
      <label htmlFor={htmlFor} className="font-medium text-fg-2" style={{ fontSize: "11.5px" }}>
        {label}
      </label>
      {children}
      {help && (
        <div className="text-fg-4" style={{ fontSize: "10.5px" }}>
          {help}
        </div>
      )}
      {source && (
        <div className="text-fg-3" style={{ fontSize: "10.5px" }} data-testid={sourceTestId}>
          {source}
        </div>
      )}
    </div>
  );
}

interface SettingRowProps {
  id: string;
  label: string;
  help: string;
  value: string;
  onChange: (v: string) => void;
  dirty: boolean;
  field: SettingField;
  envVar: string;
  /** Unit suffix appended to the env value in the disclosure (e.g. " s", " ms"). */
  unit: string;
  /** The env value is in milliseconds while the knob is in seconds (guard). */
  envIsMs?: boolean;
  /** Rendered right of the input (the live session preview). */
  inline?: ReactNode;
  children?: ReactNode;
}

function SettingRow({
  id,
  label,
  help,
  value,
  onChange,
  dirty,
  field,
  envVar,
  unit,
  envIsMs,
  inline,
  children,
}: SettingRowProps) {
  return (
    <div className="flex flex-col gap-1.5" data-dirty={dirty ? "true" : undefined}>
      <label htmlFor={`setting-${id}`} className="font-medium text-fg-2" style={{ fontSize: "11.5px" }}>
        {label}
      </label>
      <div className="flex items-center gap-3">
        <input
          id={`setting-${id}`}
          data-testid={`setting-${id}`}
          type="number"
          min={1}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          className={`w-[220px] rounded-md border bg-bg-3 px-2.5 py-1.5 font-mono text-fg placeholder:text-fg-4 focus:outline-none ${
            dirty ? "border-st-await focus:border-st-await" : "border-line-strong focus:border-acc"
          }`}
          style={{ fontSize: "12px" }}
        />
        {inline}
      </div>
      <div className="text-fg-4" style={{ fontSize: "10.5px" }}>
        {help}
      </div>
      <div className="text-fg-3" style={{ fontSize: "10.5px" }} data-testid={`setting-source-${id}`}>
        {sourceNote(field, envVar, unit, envIsMs)}
      </div>
      {children}
    </div>
  );
}

function CheckboxRow({
  id,
  checked,
  onChange,
  dirty,
  label,
  help,
  source,
}: {
  id: string;
  checked: boolean;
  onChange: (v: boolean) => void;
  dirty: boolean;
  label: string;
  help: ReactNode;
  source: string;
}) {
  return (
    <div className="flex flex-col gap-1.5" data-dirty={dirty ? "true" : undefined}>
      <label htmlFor={`setting-${id}`} className="flex cursor-pointer items-start gap-2">
        <input
          id={`setting-${id}`}
          data-testid={`setting-${id}`}
          type="checkbox"
          checked={checked}
          onChange={(e) => onChange(e.target.checked)}
          className={`mt-0.5 shrink-0 accent-acc ${dirty ? "outline outline-1 outline-st-await" : ""}`}
        />
        <span className="font-medium text-fg-2" style={{ fontSize: "11.5px" }}>
          {label}
        </span>
      </label>
      <div className="pl-5 text-fg-4" style={{ fontSize: "10.5px" }}>
        {help}
      </div>
      <div className="text-fg-3" style={{ fontSize: "10.5px" }} data-testid={`setting-source-${id}`}>
        {source}
      </div>
    </div>
  );
}

/* ------------------------------------------------------------------------------------ */
/* Source notes (D6): which tier a knob's value comes from                                */
/* ------------------------------------------------------------------------------------ */

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

