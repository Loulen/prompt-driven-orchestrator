/**
 * General › Runtime limits and General › Runs: the instance form's numeric knobs and the
 * defaults of what an agent may do on its own.
 */

import SessionCounter from "../../../run/components/SessionCounter";
import type { DraftValues } from "../../model/draft";
import type { InstanceSettings } from "../../model/settings";
import type { SettingsFieldId, SettingsSection } from "../../model/settingsCategories";
import {
  autocompleteSourceNote,
  defaultAutoNameSourceNote,
  reviewAgentCanResolveSourceNote,
} from "../../model/sourceNotes";
import { NumberRow, SwitchRow } from "./fields";
import Section from "./Section";

/** Advisory ceiling: caps above this enter the tmux-server-collapse zone
 *  (#77/#78). Not a hard limit (Sharp tool — ADR-0001), just an amber warning. */
const CAP_ADVISORY = 20;

export interface FormSectionProps {
  section: SettingsSection;
  values: DraftValues;
  settings: InstanceSettings;
  dirty: Set<SettingsFieldId>;
  setField: <K extends keyof DraftValues>(key: K, value: DraftValues[K]) => void;
}

export function SessionsSection({
  section,
  values,
  settings,
  dirty,
  setField,
  liveSessions,
}: FormSectionProps & { liveSessions: number }) {
  const capPreview = Number(values.capStr.trim());
  const capForPreview =
    Number.isInteger(capPreview) && capPreview >= 1 ? capPreview : settings.session_cap.effective;
  return (
    <Section section={section}>
      <NumberRow
        id="session-cap"
        label="Concurrent sessions"
        help="Live node sessions, daemon-wide"
        value={values.capStr}
        onChange={(v) => setField("capStr", v)}
        dirty={dirty.has("session-cap")}
        field={settings.session_cap}
        envVar="PDO_SESSION_CAP"
        unit=""
        before={<SessionCounter live={liveSessions} cap={capForPreview} />}
        note={
          Number.isInteger(capPreview) && capPreview > CAP_ADVISORY
            ? `Above ${CAP_ADVISORY}: tmux may collapse`
            : undefined
        }
        noteTone="warn"
        noteTestId="settings-cap-advisory"
      />
      <NumberRow
        id="reaper-ttl"
        label="Reaper TTL"
        help="Seconds after a node completes before its idle tmux session is reaped; the sweep runs every 60 s"
        value={values.ttlStr}
        onChange={(v) => setField("ttlStr", v)}
        dirty={dirty.has("reaper-ttl")}
        field={settings.reaper_ttl_secs}
        envVar="PDO_REAPER_TTL_SECS"
        unit=" s"
      />
      <NumberRow
        id="guard-timeout"
        label="Guard timeout"
        help="Hard timeout of a Trigger guard command"
        value={values.guardStr}
        onChange={(v) => setField("guardStr", v)}
        dirty={dirty.has("guard-timeout")}
        field={settings.guard_timeout_secs}
        envVar="PDO_GUARD_TIMEOUT_MS"
        unit=" ms"
        envIsMs
        max={600}
      />
      {/* #779: one budget for images + files per Run. Bounds the multipart body of
          POST /runs; the New run dialog reads the same value for its client-side gate. */}
      <NumberRow
        id="max-attachments-mb"
        label="Attachments per run"
        help="Total size of the images and files attached to a Run; one file may use all of it"
        value={values.attachmentsStr}
        onChange={(v) => setField("attachmentsStr", v)}
        dirty={dirty.has("max-attachments-mb")}
        field={settings.max_attachments_mb}
        envVar="PDO_MAX_ATTACHMENTS_MB"
        unit=" MB"
        max={4096}
      />
    </Section>
  );
}

export function RunsSection({ section, values, settings, dirty, setField }: FormSectionProps) {
  return (
    <Section section={section}>
      {/* Default Run auto-naming (#338). On by default — the pre-#338 behaviour. */}
      <SwitchRow
        label="Auto-name"
        help="The manager names a Run started without a name; New Run and each Trigger can override it"
        source={defaultAutoNameSourceNote(settings.default_auto_name)}
        testId="setting-row-default-auto-name"
        checked={values.defaultAutoName}
        onChange={(v) => setField("defaultAutoName", v)}
        dirty={dirty.has("default-auto-name")}
      />
      {/* Turn-end auto-completion (#469). Labelled on what is MEASURED — an end of
          turn constated in the agent's transcript — and deliberately not on a
          duration. Off by default (ADR-0012). */}
      <SwitchRow
        label="Auto-complete nodes"
        help="When the agent has clearly finished its turn and the outputs validate"
        source={autocompleteSourceNote(settings.autocomplete_turn_end)}
        testId="setting-row-autocomplete-turn-end"
        checked={values.autocompleteTurnEnd}
        onChange={(v) => setField("autocompleteTurnEnd", v)}
        dirty={dirty.has("autocomplete-turn-end")}
      />
      {/* #751 (ADR-0067 §4): off by default, the human resolves. */}
      <SwitchRow
        label="Agent resolves comments"
        help="A reply sent as resolved closes the review comment without a person confirming"
        source={reviewAgentCanResolveSourceNote(settings.review_agent_can_resolve)}
        testId="setting-row-review-agent-can-resolve"
        checked={values.reviewAgentCanResolve}
        onChange={(v) => setField("reviewAgentCanResolve", v)}
        dirty={dirty.has("review-agent-can-resolve")}
      />
    </Section>
  );
}
