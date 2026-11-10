import { useCallback, type ReactNode } from "react";
import TutorialsSection from "../../../app/tour/TutorialsSection";
import VersionUpdateSection from "../../../platform/components/VersionUpdateSection";
import StatsAbsorptionsPanel from "../../../stats/components/StatsAbsorptionsPanel";
import { updateSettings } from "../../api/settings";
import type { SettingsSection } from "../../model/settingsCategories";
import { announceSettingsChanged } from "../../store/useSettings";
import { RunsSection, SessionsSection, type FormSectionProps } from "./GeneralSections";
import PublicUrlSection from "./PublicUrlSection";
import Section, { SectionExtra } from "./Section";

/**
 * Settings › General: the instance form's address, limits and Run defaults (once `GET /settings` answered), then the sections that write on their own —
 * Version & update, Tutorials, Stats absorptions.
 */
export default function GeneralCategory({
  sections,
  form,
  loading,
  liveSessions,
  active,
  onRequestUpdate,
  onOpenChangelog,
  onStartTour,
  onStartFullTour,
}: {
  sections: SettingsSection[];
  /** The instance form, once `GET /settings` answered. */
  form: Omit<FormSectionProps, "section"> | null;
  loading: ReactNode;
  liveSessions: number;
  /** The surface is open: Version & update and Stats absorptions read then. */
  active: boolean;
  onRequestUpdate?: () => void;
  onOpenChangelog?: () => void;
  onStartTour: (tourId: string) => void;
  onStartFullTour: () => void;
}) {
  // #697: the Version & update switch writes `update_check` at the change — outside
  // the form's dirty set, like « Check now ».
  const toggleUpdateCheck = useCallback(async (on: boolean) => {
    await updateSettings({ update_check: on });
    announceSettingsChanged();
  }, []);
  return (
    <>
      {form ? (
        <>
          <PublicUrlSection section={sections[0]} {...form} />
          <SessionsSection section={sections[1]} {...form} liveSessions={liveSessions} />
          <RunsSection section={sections[2]} {...form} />
        </>
      ) : (
        loading
      )}
      <VersionUpdateSection
        section={sections[3]}
        onRequestUpdate={onRequestUpdate}
        active={active}
        onOpenChangelog={onOpenChangelog}
        onToggleCheck={toggleUpdateCheck}
      />
      <TutorialsSection section={sections[4]} onStartTour={onStartTour} onStartFullTour={onStartFullTour} />
      {/* #891 — Stats absorptions: saved at once, whatever `GET /settings` said. */}
      <Section section={sections[5]}>
        <SectionExtra>
          <StatsAbsorptionsPanel active={active} />
        </SectionExtra>
      </Section>
    </>
  );
}
