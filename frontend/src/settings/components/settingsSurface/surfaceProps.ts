import type { ProjectsEntry } from "../../../workspace/model/projectsSettings";
import type { SettingsCategoryId, SettingsSectionId } from "../../model/settingsCategories";
import type { ProjectsSource } from "./ProjectsSections";

/** Where the Settings surface opens, and what it asks of its host. */
export interface SettingsPosition {
  category: SettingsCategoryId;
  section?: SettingsSectionId;
  /** Settings › Projects (CORE-96): the Projet to show, or a new one to name. */
  project?: ProjectsEntry;
}

export interface StatsOpenIntent {
  tab: "cost";
  pricingOpen: boolean;
}

export interface SettingsSurfaceProps {
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
   * General, on its first section.
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
  /** CORE-96: the live Projets and the Runs Settings › Projects counts. */
  projects?: ProjectsSource;
}
