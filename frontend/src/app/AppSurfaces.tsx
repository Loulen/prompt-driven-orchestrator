import SettingsSurface from "../settings/components/SettingsSurface";
import StatsModal from "../stats/components/StatsModal";
import type { useSurfaces } from "./useSurfaces";
import type { ProjectsSource } from "../settings/components/settingsSurface/ProjectsSections";
import type { AppearanceEntities } from "../settings/components/settingsSurface/surfaceProps";

interface Props {
  surfaces: ReturnType<typeof useSurfaces>;
  liveSessions: number;
  onSessionsSaved: () => void;
  onOpenChangelog: () => void;
  onRequestUpdate: () => void;
  onStartTour: (id: string) => void;
  onStartFullTour: () => void;
  /** CORE-96: Settings › Projects reads the live Projets and counts the Runs. */
  projects: ProjectsSource;
  /** The live Runs and Triggers the previews of Settings › Appearance show. */
  entities: AppearanceEntities;
}

/** Settings and Stats (#690), opened under the top bar (CORE-183), one open at a time. */
export default function AppSurfaces({
  surfaces,
  liveSessions,
  onSessionsSaved,
  onOpenChangelog,
  onRequestUpdate,
  onStartTour,
  onStartFullTour,
  projects,
  entities,
}: Props) {
  return (
    <>
      <SettingsSurface
        // #717: keys MUST be namespaced per sibling — two always-mounted siblings sharing a
        // bare numeric key (both started at 0) collide in React 19's mapRemainingChildren,
        // which keys its lookup map by fiber.key alone; the wrong fiber wins the update and
        // the surface becomes permanently unclosable.
        key={"settings-" + surfaces.settingsEntry.key}
        open={surfaces.settingsOpen}
        onClose={surfaces.settingsClosed}
        closeRequest={surfaces.settingsCloseRequest}
        onCloseCancelled={surfaces.settingsCloseCancelled}
        liveSessions={liveSessions}
        onSaved={onSessionsSaved}
        initialPosition={surfaces.settingsEntry.position}
        onOpenStats={surfaces.openStats}
        // « What's new » and « Update » leave Settings first, through its draft
        // guard (the surface closes, then calls them): their modals open over the
        // app (`UpdateModals`, mounted after this), never under it.
        onOpenChangelog={onOpenChangelog}
        onRequestUpdate={onRequestUpdate}
        onStartTour={onStartTour}
        onStartFullTour={onStartFullTour}
        projects={projects}
        entities={entities}
      />
      <StatsModal
        // #717: see the SettingsSurface key above — sibling keys must never collide.
        key={"stats-" + surfaces.statsEntry.key}
        open={surfaces.statsOpen}
        onClose={() => surfaces.setStatsOpen(false)}
        initialTab={surfaces.statsEntry.intent?.tab}
        initialPricingOpen={surfaces.statsEntry.intent?.pricingOpen}
      />
    </>
  );
}
