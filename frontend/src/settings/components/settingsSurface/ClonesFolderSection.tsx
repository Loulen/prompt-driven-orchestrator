/**
 * Projects › Clones folder (CORE-97): where PDO clones a repository given by its URL,
 * each clone in `<folder>/<repository name>`. Saved with the instance form.
 */

import { isClonesFolder } from "../../model/draft";
import { clonesFolderSourceNote } from "../../model/sourceNotes";
import { FieldBlock } from "./fields";
import type { FormSectionProps } from "./GeneralSections";
import Section from "./Section";

export default function ClonesFolderSection({ section, values, settings, dirty, setField }: FormSectionProps) {
  const isDirty = dirty.has("clones-folder");
  const invalid = !isClonesFolder(values.clonesFolder);
  return (
    <Section section={section}>
      <FieldBlock
        label="Clones folder"
        htmlFor="setting-clones-folder"
        dirty={isDirty}
        help={<>Each clone goes in &lt;folder&gt;/&lt;repository name&gt;.</>}
        source={clonesFolderSourceNote(settings.clones_folder)}
        sourceTestId="setting-source-clones-folder"
      >
        <input
          id="setting-clones-folder"
          data-testid="setting-clones-folder"
          spellCheck={false}
          placeholder="~/pdo/repos"
          value={values.clonesFolder}
          onChange={(e) => setField("clonesFolder", e.target.value)}
          aria-invalid={invalid || undefined}
          className={`w-full max-w-[640px] rounded-md border bg-bg-3 px-2.5 py-1.5 font-mono text-fg placeholder:text-fg-4 focus:outline-none ${
            invalid
              ? "border-st-failed"
              : isDirty
                ? "border-st-await focus:border-st-await"
                : "border-line-strong focus:border-acc"
          }`}
          style={{ fontSize: "12px" }}
        />
        {invalid && (
          <div className="text-st-failed" style={{ fontSize: "10.5px" }} data-testid="setting-clones-folder-invalid">
            An absolute path, or one starting with ~/.
          </div>
        )}
      </FieldBlock>
    </Section>
  );
}
