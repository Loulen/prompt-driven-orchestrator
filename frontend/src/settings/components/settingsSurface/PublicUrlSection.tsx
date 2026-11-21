/**
 * General › Public URL: the address humans open the instance at, which agents receive and
 * `pdo page mount` prints its links under — an example link in the name's tooltip. Empty, the
 * field shows in its placeholder the address used instead: `PDO_PUBLIC_URL`, else the default,
 * this machine's address.
 */

import { guidedPageUrl } from "../../../platform/model/mountedPage";
import { TextInput } from "../../../shared/ui/kit";
import { isPublicUrl } from "../../model/draft";
import { publicUrlSourceNote } from "../../model/sourceNotes";
import { RowError, SettingRow } from "./fields";
import type { FormSectionProps } from "./GeneralSections";
import Section from "./Section";

export default function PublicUrlSection({ section, values, settings, dirty, setField }: FormSectionProps) {
  const typed = values.publicUrl.trim();
  const invalid = typed !== "" && !isPublicUrl(typed);
  // What an empty field leads to: the env tier, else the machine's address.
  const fallback = settings.public_url.env ?? settings.public_url.default;
  // What `pdo page mount` would print once saved.
  const base = invalid ? null : typed || fallback;
  const help = base
    ? `Agents and pdo page mount print their links with it: ${guidedPageUrl(base, "my-page")}`
    : "Agents and pdo page mount print their links with it";
  return (
    <Section section={section}>
      <SettingRow
        label="Public URL"
        help={help}
        source={publicUrlSourceNote(settings.public_url)}
        keywords="PDO_PUBLIC_URL"
        kind="wide"
        dirty={dirty.has("public-url")}
        htmlFor="setting-public-url"
        testId="setting-row-public-url"
      >
        {invalid && (
          <RowError title="An http:// or https:// origin: host and port, no path" testId="setting-public-url-invalid">
            Origin only
          </RowError>
        )}
        <TextInput
          id="setting-public-url"
          data-testid="setting-public-url"
          mono
          type="url"
          inputMode="url"
          spellCheck={false}
          placeholder={fallback}
          value={values.publicUrl}
          onChange={(e) => setField("publicUrl", e.target.value)}
          invalid={invalid}
        />
      </SettingRow>
    </Section>
  );
}
