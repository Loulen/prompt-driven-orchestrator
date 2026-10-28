/**
 * General › Public URL: the address humans open PDO at, which agents receive and
 * `pdo page mount` prints its links under — with a preview of those lines.
 */

import { guidedPageUrl, pageUrl } from "../../../platform/model/mountedPage";
import { isPublicUrl } from "../../model/draft";
import { publicUrlSourceNote } from "../../model/sourceNotes";
import { FieldBlock } from "./fields";
import type { FormSectionProps } from "./GeneralSections";
import Section from "./Section";

/** The page name the preview prints. */
const EXAMPLE_PAGE = "my-page";

export default function PublicUrlSection({ section, values, settings, dirty, setField }: FormSectionProps) {
  const typed = values.publicUrl.trim();
  const isDirty = dirty.has("public-url");
  const invalid = typed !== "" && !isPublicUrl(typed);
  // What `pdo page mount` would print once saved: the typed address, else the env tier.
  // Neither: links stay on the daemon's own address, which this browser may not show.
  const base = typed && !invalid ? typed : typed === "" ? settings.public_url.env : null;
  return (
    <Section section={section}>
      <FieldBlock
        label="Public URL"
        htmlFor="setting-public-url"
        dirty={isDirty}
        help={
          <>
            Empty: links stay on the daemon&apos;s own address (
            <span className="font-mono">http://localhost:&lt;port&gt;</span>) and only work on this
            machine. PDO does not guess it from your browser&apos;s address.
          </>
        }
        source={publicUrlSourceNote(settings.public_url)}
        sourceTestId="setting-source-public-url"
      >
        <input
          id="setting-public-url"
          data-testid="setting-public-url"
          type="url"
          inputMode="url"
          spellCheck={false}
          placeholder="https://pdo.example.net"
          value={values.publicUrl}
          onChange={(e) => setField("publicUrl", e.target.value)}
          aria-invalid={invalid || undefined}
          className={`w-full max-w-[640px] rounded-md border bg-surface-2 px-2.5 py-1.5 font-mono text-text placeholder:text-text-3 focus:outline-none ${
            invalid
              ? "border-st-failed"
              : isDirty
                ? "border-st-await focus:border-st-await"
                : "border-line-strong focus:border-primary"
          }`}
          style={{ fontSize: "12px" }}
        />
        {invalid && (
          <div className="text-st-failed" style={{ fontSize: "10.5px" }} data-testid="setting-public-url-invalid">
            An http:// or https:// origin: host and port, no path.
          </div>
        )}
      </FieldBlock>
      {base && (
        <div
          className="max-w-[640px] rounded-md border border-line bg-surface-1 px-3 py-2.5 font-mono"
          style={{ fontSize: "11px" }}
          data-testid="setting-public-url-preview"
        >
          <div className="mb-1.5 font-sans text-text-3" style={{ fontSize: "10.5px" }}>
            What an agent prints after <span className="font-mono">pdo page mount</span>
          </div>
          <div className="text-text-2">Mounted {EXAMPLE_PAGE}.</div>
          <div className="text-text-2">
            With its guide: <span className="text-primary">{guidedPageUrl(base, EXAMPLE_PAGE)}</span>
          </div>
          <div className="text-text-3">Page alone: {pageUrl(base, EXAMPLE_PAGE)}</div>
        </div>
      )}
    </Section>
  );
}
