/**
 * General › Public URL: the address humans open the instance at, which agents receive and
 * `pdo page mount` prints its links under — an example link in the name's tooltip.
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
  // What `pdo page mount` would print once saved: the typed address, else the env tier.
  // Neither: links stay on the daemon's own address, which only this machine reaches.
  const base = typed && !invalid ? typed : typed === "" ? settings.public_url.env : null;
  const help = base
    ? `Agents and pdo page mount print their links with it: ${guidedPageUrl(base, "my-page")}`
    : "Agents and pdo page mount print their links with it; empty, links stay on http://localhost:<port>";
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
          placeholder="https://pdo.example.net"
          value={values.publicUrl}
          onChange={(e) => setField("publicUrl", e.target.value)}
          invalid={invalid}
        />
      </SettingRow>
    </Section>
  );
}
