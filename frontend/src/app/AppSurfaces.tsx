import SettingsSurface from "../settings/components/SettingsSurface";
import StatsModal from "../stats/components/StatsModal";
import type { useSurfaces } from "./useSurfaces";
import type { ProjectsSource } from "../settings/components/settingsSurface/ProjectsSections";

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
}

/** The two full-window surfaces of the shell (#690): Settings and Stats, one open at a time. */
export default function AppSurfaces({
  surfaces,
  liveSessions,
  onSessionsSaved,
  onOpenChangelog,
  onRequestUpdate,
  onStartTour,
  onStartFullTour,
  projects,
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
        onClose={() => surfaces.setSettingsOpen(false)}
        liveSessions={liveSessions}
        onSaved={onSessionsSaved}
        initialPosition={surfaces.settingsEntry.position}
        onOpenStats={surfaces.openStats}
        // « What's new » and « Update » leave Settings first: their modals open
        // over the app (`UpdateModals`, mounted after this), never under it.
        onOpenChangelog={() => {
          surfaces.setSettingsOpen(false);
          onOpenChangelog();
        }}
        onRequestUpdate={() => {
          surfaces.setSettingsOpen(false);
          onRequestUpdate();
        }}
        onStartTour={onStartTour}
        onStartFullTour={onStartFullTour}
        projects={projects}
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
