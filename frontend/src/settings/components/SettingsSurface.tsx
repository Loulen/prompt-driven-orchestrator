import { useMemo, useRef, useState } from "react";
import FullWindowShell from "../../shared/ui/FullWindowShell";
import { SettingsIcon } from "../../shared/ui/brand/AccentIcons";
import { harnessCatalog } from "../../harness/model/catalog";
import { useSkillBank } from "../../skill/store/useSkillBank";
import { useSettingsPanels } from "../../shared/extensions/catalogue";
import { leaveSettingsPage } from "../../shared/extensions/deepLink";
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
import AppearanceCategory from "./settingsSurface/AppearanceCategory";
import CategoryPage from "./settingsSurface/CategoryPage";
import GeneralCategory from "./settingsSurface/GeneralCategory";
import NotificationsCategory from "./settingsSurface/NotificationsCategory";
import { DiagnosticsCategory } from "./settingsSurface/DiagnosticsRows";
import { ConfirmCloseDialog } from "./settingsSurface/overlays";
import SandboxSections from "./settingsSurface/SandboxSections";
import ExtensionPanels from "./settingsSurface/ExtensionPanels";
import { SearchContext, searching, type SettingsQuery } from "./settingsSurface/search";
import { NoSettings, SearchLine, SettingsLoading, SettingsRefused } from "./settingsSurface/SearchLine";
import { InstanceLockContext, lockedSetter, useInstanceLock } from "./settingsSurface/lock";
import SettingsRail from "./settingsSurface/SettingsRail";
import { settingsRail } from "./settingsSurface/rail";
import SettingsFooter from "./settingsSurface/SettingsFooter";
import TelemetrySections from "./settingsSurface/TelemetrySections";
import ProjectsSections from "./settingsSurface/ProjectsSections";
import { skillBankDrawer } from "./settingsSurface/skillBankDrawer";
import type { SettingsSurfaceProps } from "./settingsSurface/surfaceProps";
import { useCloseRequest, useFound, useSaveShortcut } from "./settingsSurface/hooks";

export type { SettingsPosition, StatsOpenIntent } from "./settingsSurface/surfaceProps";

/** The rail entry of the extensions' settings panels (ADR-0081), shown only when one exists. */
const EXTENSIONS = "extensions";

/**
 * Instance-wide settings (#129, ADR-0015; full-window surface since #690 / CONTEXT.md
 * « Surface Settings »): the shell Stats uses, a rail of categories, and per category
 * a scrollable page cut into sections that a second column lists and scroll-spies.
 *
 * Each setting is a row: its name on the left — its help and the source of its value in
 * the name's tooltip —, its control on the right. A search line above the page finds rows
 * in every category, or only the changed ones.
 *
 * Agents and Sandbox & worktrees (#691) mix two kinds of sections: fields of the instance
 * form (Save), and inline **panels with their own persistence** — agent profiles, staging
 * profiles, worktree provisioning — whose every edit is written at once (the section's
 * « Saved at once » icon). A panel's write never touches the dirty set and Save never sends
 * panel data; the Save bar shows only while the form has something to save, so the two
 * never compete for the eye.
 *
 * One explicit **Save** for the whole instance form, on every category (`useSettingsForm`):
 * a category switch renders another page but never touches the draft, and the dirty set is
 * derived — per row (amber edge), per section (section-column dot), per category (rail
 * dot) — from the same comparisons the PUT payload uses.
 *
 * Precedence is `stored → env → default`: a stored value wins, so this page is
 * authoritative. It discloses a shadowed env var (D6) rather than ignoring it, and
 * validates fail-fast (D7) client-side, with the daemon's `400` surfaced next to Save.
 */
export default function SettingsSurface({
  open,
  onClose,
  closeRequest = 0,
  onCloseCancelled,
  liveSessions = 0,
  onSaved,
  initialPosition,
  onOpenStats,
  onOpenChangelog,
  onRequestUpdate,
  onStartTour,
  onStartFullTour,
  projects,
  entities,
}: SettingsSurfaceProps) {
  const { settings, settled, refused: readRefused, save, refresh } = useSettings(open);
  const updateRefused = useInstanceLock(open);
  const { profiles: agentProfiles, refused: agentProfilesRefused, refresh: refreshAgentProfiles } =
    useAgentProfiles(open);
  const skills = useSkillBank(open);
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
  // The search line: a few words, or only the changed settings, across every category.
  const [needle, setNeedle] = useState("");
  const [modifiedOnly, setModifiedOnly] = useState(false);
  const query = useMemo<SettingsQuery>(
    () => ({ needle: needle.trim().toLowerCase(), modifiedOnly }),
    [needle, modifiedOnly],
  );
  // The section column: the page on screen draws its list there.
  const [navSlot, setNavSlot] = useState<HTMLDivElement | null>(null);
  const pagesRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const found = useFound(pagesRef, searching(query) && open);
  /** What waits for this surface to actually close: a tour (#823), Update, What's new. */
  const afterClose = useRef<(() => void) | null>(null);

  // The surface is unmounted-by-render (`if (!open) return null`), so its state SURVIVES
  // a close: the position on purpose (story 17), the draft and the drawer on purpose NOT.
  const closeNow = () => {
    form.reset();
    setDrawerOpen(false);
    setConfirmClose(false);
    leaveSettingsPage();
    onClose();
    // What `leaveThen` parked runs once the surface is gone: a tour aims at the app
    // (#823), and Update or What's new opens over the app alone.
    const parked = afterClose.current;
    afterClose.current = null;
    parked?.();
  };

  /** ✕, Cancel, Escape: guarded by the dirty draft (story 16). */
  const requestClose = () => {
    if (isDirty) setConfirmClose(true);
    else closeNow();
  };
  const extensionPages = useSettingsExtensionPages({ open, category, setCategory, requestClose });
  useCloseRequest(closeRequest, open, requestClose);
  /** The confirm answered by staying: a parked action or switch (CORE-183) is cancelled. */
  const keepEditing = () => {
    setConfirmClose(false);
    afterClose.current = null;
    onCloseCancelled?.();
  };

  /** Leave Settings through the SAME dirty guard as every exit, then `run` (kept: nothing
   *  runs): a tour (#823), Update, What's new. */
  const leaveThen = (run: () => void) => {
    afterClose.current = run;
    requestClose();
  };
  /** An entry that opens over the app: it leaves Settings first. Absent, it stays absent. */
  const leavingFor = (run?: () => void) => run && (() => leaveThen(run));

  // Escape order: tooltip (Radix, before us) → skill bank if open → confirm-close if dirty
  // → close.
  const onEscape = () => {
    if (confirmClose) {
      keepEditing();
      return;
    }
    if (drawerOpen) {
      setDrawerOpen(false);
      return;
    }
    // A search on screen is cleared first.
    if (needle || modifiedOnly) {
      setNeedle("");
      setModifiedOnly(false);
      return;
    }
    requestClose();
  };

  // Ctrl+S saves the instance form, not while an extension's page or the close
  // confirmation is on screen.
  useSaveShortcut(open, extensionPages.active || confirmClose ? null : form.handleSave);

  if (!open) return null;

  const rail = settingsRail(
    SETTINGS_CATEGORIES.map((item) => ({ id: item.id, label: item.label, dirty: rollup.categories.has(item.id) })),
    [...(extensionPanels.length > 0 ? [{ id: EXTENSIONS, label: "Extensions" }] : []), ...extensionPages.rail],
  );
  const known = SETTINGS_CATEGORIES.some((item) => item.id === category) || extensionPages.active;
  // An extension page (or the Extensions entry) whose extension left: back to General.
  const shownCategory = known || (category === EXTENSIONS && extensionPanels.length > 0) ? category : "general";
  const catalog = settings ? harnessCatalog(settings.harness_descriptors) : null;
  const instanceSetField = lockedSetter(setField, updateRefused);
  const formProps = values && settings ? { values, settings, dirty: rollup.fields, setField: instanceSetField } : null;
  const inSearch = searching(query);
  const shownLabel = rail.find((item) => item.id === shownCategory)?.label ?? "";

  const rememberSection = (cat: SettingsCategoryId, section: SettingsSectionId) =>
    setLastSection((current) =>
      current[cat] === section ? current : { ...current, [cat]: section },
    );

  const selectCategory = (id: string) => {
    setNeedle("");
    setModifiedOnly(false);
    extensionPages.select(id);
  };

  const shellDrawer = skillBankDrawer({
    open: drawerOpen,
    onClose: () => setDrawerOpen(false),
    ...skills,
    home: settings?.home ?? null,
  });

  // Under a search, a category still loading shows nothing; a refused read says why (CORE-248).
  const loading = inSearch ? null : readRefused !== null ? <SettingsRefused sentence={readRefused} /> : <SettingsLoading />;

  return (
    <>
      <FullWindowShell
        title="Settings"
        titleIcon={<SettingsIcon size={16} strokeWidth={1.5} aria-hidden />}
        testId="settings-surface"
        onClose={requestClose}
        onEscape={onEscape}
        closeLabel="Close settings"
        headerExtras={
          inSearch ? undefined : (
            <>
              <span className="set-sub" title={shownLabel} data-testid="settings-header-category">
                {shownLabel}
              </span>
              {extensionPages.headerExtras}
            </>
          )
        }
        headerActions={extensionPages.headerActions(null)}
        drawer={shellDrawer}
        mainClassName="set-main"
      >
        {/* Under a search the rows come from every category: no category is current, and
            the section column, which lists one category's sections, steps aside. */}
        <SettingsRail entries={rail} active={inSearch ? null : shownCategory} onSelect={selectCategory} />
        <div
          ref={setNavSlot}
          className="set-nav set-nav--sec"
          hidden={!!extensionPages.active || inSearch}
        />
        <div className="set-right">
          {!extensionPages.active && (
            <SearchLine
              ref={searchRef}
              needle={needle}
              onNeedle={setNeedle}
              count={inSearch ? found.rows : null}
              modifiedOnly={modifiedOnly}
              onModifiedOnly={setModifiedOnly}
            />
          )}
          <SearchContext.Provider value={query}>
            <InstanceLockContext.Provider value={updateRefused}>
            <div ref={pagesRef} className={inSearch ? "set-pages set-pages--search" : "set-pages"}>
            {SETTINGS_CATEGORIES.map((item) => (
              <CategoryPage
                key={item.id}
                categoryId={item.id}
                active={shownCategory === item.id}
                searching={inSearch}
                navSlot={navSlot}
                ready={settled}
                initialSection={lastSection[item.id]}
                highlight={initialPosition?.category === item.id && !!initialPosition.section}
                dirtySections={rollup.sections}
                onSectionChange={(section) => rememberSection(item.id, section)}
              >
                {item.id === "general" && (
                  <GeneralCategory
                    sections={item.sections}
                    form={formProps}
                    loading={loading}
                    liveSessions={liveSessions}
                    active={open}
                    // Every entry closes this surface first: a tour points at the app (#823),
                    // Update and What's new show alone over it, and Settings does not come back.
                    onRequestUpdate={leavingFor(onRequestUpdate)}
                    onOpenChangelog={leavingFor(onOpenChangelog)}
                    onStartTour={(tourId) => leaveThen(() => onStartTour?.(tourId))}
                    onStartFullTour={() => leaveThen(() => onStartFullTour?.())}
                  />
                )}

                {item.id === "appearance" && <AppearanceCategory sections={item.sections} entities={entities} />}
                {item.id === "notifications" && <NotificationsCategory sections={item.sections} projects={projects?.projects ?? []} />}

                {item.id === "agents" &&
                  (formProps && catalog ? (
                    <AgentsSections
                      sections={item.sections}
                      {...formProps}
                      catalog={catalog}
                      agentProfiles={agentProfiles}
                      agentProfilesRefused={agentProfilesRefused}
                      skillsRefused={skills.refused}
                      onProfilesChanged={async () => {
                        await refreshAgentProfiles();
                        announceAgentProfilesChanged();
                        onSaved?.();
                      }}
                      skillBank={skills.bank}
                      skillsLoaded={skills.loaded}
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

                {item.id === "projects" && (
                  <ProjectsSections sections={item.sections} source={projects} home={settings?.home ?? null}
                    position={initialPosition} active={open && shownCategory === item.id}
                    form={formProps} loading={loading} />
                )}

                {item.id === "telemetry" &&
                  (formProps ? <TelemetrySections sections={item.sections} active={shownCategory === item.id} {...formProps} /> : loading)}

                {item.id === "diagnostics" && (
                  <DiagnosticsCategory sections={item.sections} settings={settings} onOpenStats={onOpenStats} loading={loading} />
                )}
              </CategoryPage>
            ))}
              {extensionPanels.length > 0 && !inSearch && (
                <ExtensionPanels active={shownCategory === EXTENSIONS} navSlot={navSlot} />
              )}
              {!inSearch && extensionPages.content}
              {inSearch && found.sections === 0 && (
                <NoSettings
                  onClear={() => {
                    setNeedle("");
                    setModifiedOnly(false);
                  }}
                />
              )}
            </div>
            </InstanceLockContext.Provider>
          </SearchContext.Provider>
          {/* An extension's page owns its writes: Save would read as saving it. */}
          {!extensionPages.active && (
            <SettingsFooter
              form={form}
              saveButtonRef={saveButtonRef}
              loaded={!!settings}
              onDiscard={() => {
                form.reset();
                // The bar goes with the draft: the focus goes to the search line.
                searchRef.current?.focus();
              }}
            />
          )}
        </div>
      </FullWindowShell>

      {confirmClose && (
        <ConfirmCloseDialog
          rollup={rollup}
          saving={form.saveState.status === "saving"}
          onKeep={keepEditing}
          onDiscard={closeNow}
          onSaveAndClose={async () => {
            const ok = await form.handleSave();
            if (ok) closeNow();
            else keepEditing();
          }}
        />
      )}
    </>
  );
}
