import { useMemo, useRef, useState } from "react";
import { Loader2 } from "lucide-react";
import FullWindowShell from "../../shared/ui/FullWindowShell";
import { EmptyState } from "../../shared/ui/kit";
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
import CategoryPage from "./settingsSurface/CategoryPage";
import GeneralCategory from "./settingsSurface/GeneralCategory";
import { HarnessDescriptorRows, PriceTableRows } from "./settingsSurface/DiagnosticsRows";
import { ConfirmCloseDialog } from "./settingsSurface/overlays";
import SandboxSections from "./settingsSurface/SandboxSections";
import ExtensionPanels from "./settingsSurface/ExtensionPanels";
import { SearchContext, searching, type SettingsQuery } from "./settingsSurface/search";
import { NoSettings, SearchLine } from "./settingsSurface/SearchLine";
import SettingsRail from "./settingsSurface/SettingsRail";
import { settingsRail } from "./settingsSurface/rail";
import SettingsFooter from "./settingsSurface/SettingsFooter";
import TelemetrySections from "./settingsSurface/TelemetrySections";
import ProjectsSections from "./settingsSurface/ProjectsSections";
import { skillBankDrawer } from "./settingsSurface/skillBankDrawer";
import type { SettingsSurfaceProps } from "./settingsSurface/surfaceProps";
import { useFound, useSaveShortcut } from "./settingsSurface/hooks";

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
  liveSessions = 0,
  onSaved,
  initialPosition,
  onOpenStats,
  onOpenChangelog,
  onRequestUpdate,
  onStartTour,
  onStartFullTour,
  projects,
}: SettingsSurfaceProps) {
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
  const formProps = values && settings ? { values, settings, dirty: rollup.fields, setField } : null;
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
    bank: skillBank,
    loaded: skillsLoaded,
    home: settings?.home ?? null,
    refresh: refreshSkills,
  });

  // Under a search, a category still loading shows nothing.
  const loading = inSearch ? null : (
    <div className="set-extra" data-testid="settings-loading">
      <EmptyState icon={Loader2} spin layout="row">
        Loading…
      </EmptyState>
    </div>
  );

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
                    onRequestUpdate={onRequestUpdate}
                    onOpenChangelog={onOpenChangelog}
                    // #823 — guided tours. Every entry closes this surface first: a tour
                    // points at the app, which is underneath it.
                    onStartTour={(tourId) => startTourAfterClose(() => onStartTour?.(tourId))}
                    onStartFullTour={() => startTourAfterClose(() => onStartFullTour?.())}
                  />
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

                {item.id === "projects" && (
                  <ProjectsSections sections={item.sections} source={projects} home={settings?.home ?? null}
                    position={initialPosition} active={open && shownCategory === item.id}
                    form={formProps} loading={loading} />
                )}

                {item.id === "telemetry" &&
                  (formProps ? <TelemetrySections sections={item.sections} active={shownCategory === item.id} {...formProps} /> : loading)}

                {item.id === "diagnostics" &&
                  (settings ? (
                    <>
                      <PriceTableRows section={item.sections[0]} settings={settings} onOpenStats={onOpenStats} />
                      <HarnessDescriptorRows section={item.sections[1]} settings={settings} />
                    </>
                  ) : (
                    loading
                  ))}
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
