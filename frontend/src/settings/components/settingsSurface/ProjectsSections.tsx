/**
 * Settings › Projects (CORE-96): the Projets and their repositories, each gesture
 * written at once — beside the instance form, never through its Save; then the
 * clones folder (CORE-97), which the form's Save writes.
 */

import type { ReactNode } from "react";
import type { RunListEntry } from "../../../shared/generated/RunListEntry";
import ProjectsPanel from "../../../workspace/components/projects/ProjectsPanel";
import type { Project } from "../../../workspace/model/project";
import type { SettingsPosition } from "../SettingsSurface";
import type { SettingsSection } from "../../model/settingsCategories";
import ClonesFolderSection from "./ClonesFolderSection";
import type { FormSectionProps } from "./GeneralSections";
import Section, { SectionExtra } from "./Section";

/** What the shell hands Settings › Projects: the live Projets, the Runs it counts. */
export interface ProjectsSource {
  projects: Project[];
  runs: RunListEntry[];
  /** A Projet changed here: the shell reads the Projets again. */
  onChanged: () => void;
}

export default function ProjectsSections({
  sections,
  source,
  home,
  position,
  active,
  form,
  loading,
}: {
  sections: SettingsSection[];
  source: ProjectsSource | undefined;
  home: string | null;
  /** Where Settings opened: on a Projet, or on a new one to name. */
  position: SettingsPosition | undefined;
  active: boolean;
  /** The instance form, once `GET /settings` answered. */
  form: Omit<FormSectionProps, "section"> | null;
  loading: ReactNode;
}) {
  return (
    <>
      <Section section={sections[0]}>
        <SectionExtra>
          <ProjectsPanel
            projects={source?.projects ?? []}
            runs={source?.runs ?? []}
            home={home}
            entry={
              position?.category === "projects" ? position.project : undefined
            }
            active={active}
            onChanged={() => source?.onChanged()}
          />
        </SectionExtra>
      </Section>
      {form ? <ClonesFolderSection section={sections[1]} {...form} /> : loading}
    </>
  );
}
