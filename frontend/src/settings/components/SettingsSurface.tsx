import { useCallback, useRef, useState } from "react";
import FullWindowShell, { type RailItem, type ShellDrawer } from "../../shared/ui/FullWindowShell";
import TutorialsSection from "../../app/tour/TutorialsSection";
import { harnessCatalog } from "../../harness/model/catalog";
import VersionUpdateSection from "../../platform/components/VersionUpdateSection";
import SkillBankPanel from "../../skill/components/SkillBankPanel";
import { announceSkillsChanged, useSkillBank } from "../../skill/store/useSkillBank";
import ExtensionSettingsPanels from "../../shared/extensions/ExtensionSettingsPanels";
import { useSettingsPanels } from "../../shared/extensions/catalogue";
import { leaveSettingsPage } from "../../shared/extensions/deepLink";
import StatsAbsorptionsPanel from "../../stats/components/StatsAbsorptionsPanel";
import { updateSettings } from "../api/settings";
import {
  SETTINGS_CATEGORIES,
  type SettingsCategoryId,
  type SettingsSectionId,
} from "../model/settingsCategories";
import { announceAgentProfilesChanged, useAgentProfiles } from "../store/useAgentProfiles";
import { announceSettingsChanged, useSettings } from "../store/useSettings";
import { useSettingsForm } from "../store/useSettingsForm";
import { settingsPageOpen, useSettingsExtensionPages } from "../store/useSettingsExtensionPages";
import AgentsSections from "./settingsSurface/AgentsSections";
import CategoryPage from "./settingsSurface/CategoryPage";
import { HarnessDescriptorRows, PriceTableRows } from "./settingsSurface/DiagnosticsRows";
import { RunsSection, SessionsSection } from "./settingsSurface/GeneralSections";
import InterfaceSection from "./settingsSurface/InterfaceSection";
import { ConfirmCloseDialog } from "./settingsSurface/overlays";
import SandboxSections from "./settingsSurface/SandboxSections";
import Section from "./settingsSurface/Section";
import SettingsFooter from "./settingsSurface/SettingsFooter";

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
  /** #698: Version & update › « What's new » opens the changelog modal over the surface. */
  onOpenChangelog?: () => void;
  /** #699: Version & update › « Update » — the host owns the confirm and the waiting flow. */
  onRequestUpdate?: () => void;
  /** #823: Tutorials › Start / Replay. Fired AFTER this surface has closed. */
  onStartTour?: (tourId: string) => void;
  /** #823: Tutorials › Full tour. Fired AFTER this surface has closed. */
  onStartFullTour?: () => void;
}

const RAIL_WIDTH = 176;


/** The rail entry of the extensions' settings panels (ADR-0081), shown only when one exists. */
const EXTENSIONS = "extensions";

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
 * One explicit **Save** for the whole instance form, visible on every category
 * (`useSettingsForm`): a category switch renders another page but never touches the draft,
 * and the dirty set is derived — per field (amber border), per section (sub-column dot), per
 * category (rail dot) — from the same comparisons the PUT payload uses.
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
  onOpenChangelog,
  onRequestUpdate,
  onStartTour,
  onStartFullTour,
}: Props) {
  const { settings, settled, save, refresh } = useSettings(open);
  const { profiles: agentProfiles, refresh: refreshAgentProfiles } = useAgentProfiles(open);
  const { bank: skillBank, loaded: skillsLoaded, refresh: refreshSkills } = useSkillBank(open);
  const { saveButtonRef, ...form } = useSettingsForm({ settings, save, onSaved });
  const { values, rollup, isDirty, setField } = form;

  // Position — survives a close (the surface is unmounted-by-render below, so this state
  // IS the page-session memory, story 17). A reload remounts and lands on General, or on
  // the extension page (`settings/page`) its deep link names.
  const [category, setCategory] = useState<string>(
    () => initialPosition?.category ?? settingsPageOpen()?.key ?? "general",
  );
  const extensionPanels = useSettingsPanels();
  const [lastSection, setLastSection] = useState<
    Partial<Record<SettingsCategoryId, SettingsSectionId>>
  >(() =>
    initialPosition?.section ? { [initialPosition.category]: initialPosition.section } : {},
  );
  // The one panel that still opens over Settings (#691, user decision 2026-09-04): the
  // skill bank — a journey of its own, not a settings field.
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [confirmClose, setConfirmClose] = useState(false);
  // #697: the Version & update switch writes `update_check` at the change — outside
  // the form's dirty set, like « Check now ».
  const toggleUpdateCheck = useCallback(async (on: boolean) => {
    await updateSettings({ update_check: on });
    announceSettingsChanged();
  }, []);
  /** #823 — a Tutorials Start waiting for this surface to actually close. */
  const pendingTour = useRef<(() => void) | null>(null);

  // The surface is unmounted-by-render (`if (!open) return null`), so its state SURVIVES
  // a close: the position on purpose (story 17), the draft and the drawer on purpose NOT.
  const closeNow = () => {
    form.reset();
    setDrawerOpen(false);
    setConfirmClose(false);
    leaveSettingsPage();
    onClose();
    // #823: a tour parked by `startTourAfterClose` runs once the surface is
    // actually gone — a Projecteur aiming at the app under a full-window
    // Settings would point at nothing.
    const parked = pendingTour.current;
    pendingTour.current = null;
    parked?.();
  };

  /** ✕, Cancel, Escape: guarded by the dirty draft (story 16). */
  const requestClose = () => {
    if (isDirty) setConfirmClose(true);
    else closeNow();
  };
  const extensionPages = useSettingsExtensionPages({ open, category, setCategory, requestClose });

  /**
   * #823 — Tutorials › Start / Replay. The tour goes through the SAME dirty guard
   * as every other exit: a half-edited instance form is not something to drop
   * because the reader clicked "Replay". Cancelling the confirm cancels the tour
   * too, which is why the parked action is cleared on the way out either way.
   */
  const startTourAfterClose = (run: () => void) => {
    pendingTour.current = run;
    requestClose();
  };

  // Escape order: tooltip (Radix, before us) → skill bank if open → confirm-close if dirty
  // → close.
  const onEscape = () => {
    if (confirmClose) {
      setConfirmClose(false);
      return;
    }
    if (drawerOpen) {
      setDrawerOpen(false);
      return;
    }
    requestClose();
  };

  if (!open) return null;

  const rail: RailItem[] = SETTINGS_CATEGORIES.map((item) => ({
    id: item.id,
    label: item.label,
    dirty: rollup.categories.has(item.id),
  }));
  if (extensionPanels.length > 0) rail.push({ id: EXTENSIONS, label: "Extensions", dirty: false });
  rail.push(...extensionPages.rail);
  const known = SETTINGS_CATEGORIES.some((item) => item.id === category) || extensionPages.active;
  // An extension page (or the Extensions entry) whose extension left: back to General.
  const shownCategory = known || (category === EXTENSIONS && extensionPanels.length > 0) ? category : "general";
  const catalog = settings ? harnessCatalog(settings.harness_descriptors) : null;
  const formProps = values && settings ? { values, settings, dirty: rollup.fields, setField } : null;

  const rememberSection = (cat: SettingsCategoryId, section: SettingsSectionId) =>
    setLastSection((current) =>
      current[cat] === section ? current : { ...current, [cat]: section },
    );

  // The skill bank in the shell's secondary panel (#690 decision 5, framed by the shell
  // since #944): the rail stays visible, Escape returns to Settings › Skills first, and the
  // Save footer hides under it so its own writes and Save are never both in view.
  const shellDrawer: ShellDrawer | null = drawerOpen
    ? {
        title: "Skill bank",
        escHint: "saves as you go · Esc returns to Settings",
        widthClassName: "w-[min(880px,90vw)]",
        testId: "settings-drawer",
        dataAttributes: { "data-drawer": "skills" },
        onClose: () => setDrawerOpen(false),
        content: (
          <SkillBankPanel
            bank={skillBank}
            loaded={skillsLoaded}
            home={settings?.home ?? null}
            onChanged={async () => {
              await refreshSkills();
              announceSkillsChanged();
            }}
          />
        ),
      }
    : null;

  const loading = (
    <div className="px-1 py-4 text-fg-4" style={{ fontSize: "12px" }} data-testid="settings-loading">
      Loading…
    </div>
  );

  return (
    <>
      <FullWindowShell
        title="Settings"
        testId="settings-surface"
        onClose={requestClose}
        onEscape={onEscape}
        closeLabel="Close settings"
        rail={rail}
        activeRail={shownCategory}
        onRailChange={extensionPages.select}
        railWidth={RAIL_WIDTH}
        railAriaLabel="Settings categories"
        railTestIdPrefix="settings-category"
        headerExtras={extensionPages.headerExtras}
        headerActions={extensionPages.headerActions(
          <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
            Esc closes · ↑↓ on the rail switch category
          </span>,
        )}
        // An extension's page owns its writes: Save would read as saving it.
        footer={!extensionPages.active && (
          <SettingsFooter form={form} saveButtonRef={saveButtonRef} loaded={!!settings} onCancel={requestClose} />
        )}
        drawer={shellDrawer}
      >
        {SETTINGS_CATEGORIES.map((item) => (
          <CategoryPage
            key={item.id}
            categoryId={item.id}
            active={shownCategory === item.id}
            ready={settled}
            initialSection={lastSection[item.id]}
            highlight={initialPosition?.category === item.id && !!initialPosition.section}
            dirtySections={rollup.sections}
            onSectionChange={(section) => rememberSection(item.id, section)}
          >
            {item.id === "general" && (
              <>
                <InterfaceSection section={item.sections[0]} />
                {formProps ? (
                  <>
                    <SessionsSection
                      section={item.sections[1]}
                      {...formProps}
                      liveSessions={liveSessions}
                    />
                    <RunsSection section={item.sections[2]} {...formProps} />
                  </>
                ) : (
                  loading
                )}
                <Section section={item.sections[3]}>
                  <VersionUpdateSection
                    onRequestUpdate={onRequestUpdate}
                    active={open}
                    onOpenChangelog={onOpenChangelog}
                    onToggleCheck={toggleUpdateCheck}
                  />
                </Section>
                {/* #823 — guided tours. Every entry closes this surface first:
                    a tour points at the app, which is underneath it. */}
                <Section section={item.sections[4]}>
                  <TutorialsSection
                    onStartTour={(tourId) => startTourAfterClose(() => onStartTour?.(tourId))}
                    onStartFullTour={() => startTourAfterClose(() => onStartFullTour?.())}
                  />
                </Section>
                {/* #891 — Stats absorptions: saves as you go, whatever `GET /settings` said. */}
                <Section section={item.sections[5]}>
                  <StatsAbsorptionsPanel active={open} />
                </Section>
              </>
            )}

            {item.id === "agents" &&
              (formProps && catalog ? (
                <AgentsSections
                  sections={item.sections}
                  {...formProps}
                  catalog={catalog}
                  agentProfiles={agentProfiles}
                  onProfilesChanged={async () => {
                    await refreshAgentProfiles();
                    announceAgentProfilesChanged();
                    onSaved?.();
                  }}
                  skillBank={skillBank}
                  skillsLoaded={skillsLoaded}
                  onOpenSkillBank={() => setDrawerOpen(true)}
                />
              ) : (
                loading
              ))}

            {item.id === "sandbox" &&
              (formProps ? (
                <SandboxSections
                  sections={item.sections}
                  {...formProps}
                  missingDefaultProfile={form.missingDefaultProfile}
                  // Refetch `GET /settings` so the Default-sandbox `<select>` sees the new
                  // name list (and a freshly dangling `reason`) without a reopen, and tell
                  // New Run (mounted underneath) the same thing.
                  onProfilesChanged={() => {
                    void refresh();
                    announceSettingsChanged();
                    onSaved?.();
                  }}
                />
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
        {extensionPanels.length > 0 && <ExtensionSettingsPanels active={shownCategory === EXTENSIONS} />}
        {extensionPages.content}
      </FullWindowShell>

      {confirmClose && (
        <ConfirmCloseDialog
          rollup={rollup}
          saving={form.saveState.status === "saving"}
          onKeep={() => {
            setConfirmClose(false);
            // Staying also cancels a tour parked by Tutorials (#823).
            pendingTour.current = null;
          }}
          onDiscard={closeNow}
          onSaveAndClose={async () => {
            const ok = await form.handleSave();
            if (ok) closeNow();
            else setConfirmClose(false);
          }}
        />
      )}
    </>
  );
}
