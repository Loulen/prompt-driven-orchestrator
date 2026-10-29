/**
 * Projects › Clones folder (CORE-97): where a repository given by its URL is cloned, each
 * clone in `<folder>/<repository name>`. Saved with the instance form.
 */

import { TextInput } from "../../../shared/ui/kit";
import { isClonesFolder } from "../../model/draft";
import { clonesFolderSourceNote } from "../../model/sourceNotes";
import { RowError, SettingRow } from "./fields";
import type { FormSectionProps } from "./GeneralSections";
import Section from "./Section";

export default function ClonesFolderSection({ section, values, settings, dirty, setField }: FormSectionProps) {
  const invalid = !isClonesFolder(values.clonesFolder);
  return (
    <Section section={section}>
      <SettingRow
        label="Clones folder"
        help="Each clone goes in <folder>/<repository name>; a URL already cloned reuses its clone"
        source={clonesFolderSourceNote(settings.clones_folder)}
        kind="wide"
        dirty={dirty.has("clones-folder")}
        htmlFor="setting-clones-folder"
        testId="setting-row-clones-folder"
      >
        {invalid && (
          <RowError title="An absolute path, or one starting with ~/" testId="setting-clones-folder-invalid">
            ~/ or /
          </RowError>
        )}
        <TextInput
          id="setting-clones-folder"
          data-testid="setting-clones-folder"
          mono
          spellCheck={false}
          placeholder="~/pdo/repos"
          value={values.clonesFolder}
          onChange={(e) => setField("clonesFolder", e.target.value)}
          invalid={invalid}
        />
      </SettingRow>
    </Section>
  );
}
