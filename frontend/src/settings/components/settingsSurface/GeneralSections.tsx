/**
 * General › Sessions and General › Runs: the instance form's numeric knobs and the defaults
 * of what an agent may do on its own.
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
import { CheckboxRow, SettingRow } from "./fields";
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
      <SettingRow
        id="session-cap"
        label="Concurrent session cap"
        help="Max live NodeRun sessions daemon-wide. Kept below the tmux-collapse zone (#77/#78)."
        value={values.capStr}
        onChange={(v) => setField("capStr", v)}
        dirty={dirty.has("session-cap")}
        field={settings.session_cap}
        envVar="PDO_SESSION_CAP"
        unit=""
        inline={
          <span
            className="flex items-center gap-1.5 text-text-3"
            style={{ fontSize: "10.5px" }}
          >
            live <SessionCounter live={liveSessions} cap={capForPreview} />
          </span>
        }
      >
        {Number.isInteger(capPreview) && capPreview > CAP_ADVISORY && (
          <div
            className="text-st-await"
            style={{ fontSize: "10.5px" }}
            data-testid="settings-cap-advisory"
          >
            Caps above {CAP_ADVISORY} risk collapsing the tmux server (#77/#78).
          </div>
        )}
      </SettingRow>
      <SettingRow
        id="reaper-ttl"
        label="Reaper TTL (seconds)"
        help="Seconds after a node completes before its idle tmux session is reaped. Sweep runs every 60 s, so values below ~60 s add little."
        value={values.ttlStr}
        onChange={(v) => setField("ttlStr", v)}
        dirty={dirty.has("reaper-ttl")}
        field={settings.reaper_ttl_secs}
        envVar="PDO_REAPER_TTL_SECS"
        unit=" s"
      />
      <SettingRow
        id="guard-timeout"
        label="Trigger guard timeout (seconds)"
        help="Hard timeout for a Trigger guard command. 1–600 s."
        value={values.guardStr}
        onChange={(v) => setField("guardStr", v)}
        dirty={dirty.has("guard-timeout")}
        field={settings.guard_timeout_secs}
        envVar="PDO_GUARD_TIMEOUT_MS"
        unit=" ms"
        envIsMs
      />
      {/* #779: one budget for images + files per Run. Bounds the
          multipart body of POST /runs; the New run dialog reads the
          same value for its client-side gate. */}
      <SettingRow
        id="max-attachments-mb"
        label="Max attachments per run (MB)"
        help="Total size of the images and files attached to a Run (New run dialog, `pdo run create --image/--file`). One budget per run; a single file may use all of it. 1–4096 MB."
        value={values.attachmentsStr}
        onChange={(v) => setField("attachmentsStr", v)}
        dirty={dirty.has("max-attachments-mb")}
        field={settings.max_attachments_mb}
        envVar="PDO_MAX_ATTACHMENTS_MB"
        unit=" MB"
      />
    </Section>
  );
}

export function RunsSection({ section, values, settings, dirty, setField }: FormSectionProps) {
  return (
    <Section section={section}>
      {/* Default Run auto-naming (#338). On by default — the pre-#338
          behaviour. */}
      <CheckboxRow
        id="default-auto-name"
        checked={values.defaultAutoName}
        onChange={(v) => setField("defaultAutoName", v)}
        dirty={dirty.has("default-auto-name")}
        label="Let the manager auto-name a Run created without a name"
        help={
          <>
            When a Run starts with no name — from the New Run dialog with
            "Auto-generated" left on, or from a Trigger — the Pipeline Manager
            gives it a short descriptive name. Turn this off and such a Run
            keeps a stable <span className="font-mono">Untitled run …</span>{" "}
            placeholder instead. This is only the <strong>default</strong>: the
            New Run box and each Trigger can override it. Naming is the
            manager's job, so a managerless Run stays unnamed until its first
            manager start (which passes the same instruction).
          </>
        }
        source={defaultAutoNameSourceNote(settings.default_auto_name)}
      />
      {/* #751 (ADR-0067 §4): right after auto-name — both are about how much
          the agent may do on its own. Off by default: the human resolves. */}
      <CheckboxRow
        id="review-agent-can-resolve"
        checked={values.reviewAgentCanResolve}
        onChange={(v) => setField("reviewAgentCanResolve", v)}
        dirty={dirty.has("review-agent-can-resolve")}
        label="Let the agent resolve review comments directly"
        help={
          <>
            By default a reply sent with{" "}
            <span className="font-mono">pdo review reply --resolved</span> is only a{" "}
            <strong>proposal</strong>: the comment shows "Resolution proposed" and you
            click Resolve or Reopen. Turn this on and such a reply resolves the comment
            on the spot, for every Run of this instance. You can always reopen a
            resolved comment, whoever closed it. Off is the safer default: it keeps a
            comment from being a ticket the agent validates for itself.
          </>
        }
        source={reviewAgentCanResolveSourceNote(settings.review_agent_can_resolve)}
      />
      {/* Turn-end auto-completion (#469). Labelled on what is MEASURED — an
          end of turn constated in the agent's transcript — and deliberately
          not on a duration. Off by default (ADR-0012). */}
      <CheckboxRow
        id="autocomplete-turn-end"
        checked={values.autocompleteTurnEnd}
        onChange={(v) => setField("autocompleteTurnEnd", v)}
        dirty={dirty.has("autocomplete-turn-end")}
        label="Try to auto-complete a node when its agent has clearly finished its turn"
        help={
          <>
            When an agent stops without running{" "}
            <span className="font-mono">pdo complete</span>, PDO can finish the
            node for it — but only when its transcript shows the turn actually
            ended (no tool call in flight) <strong>and</strong> its declared
            outputs validate. An agent still inside a long tool call, waiting on
            a reply, or stopped to ask a question is never touched. Leave this off
            and such a node waits for you.
          </>
        }
        source={autocompleteSourceNote(settings.autocomplete_turn_end)}
      />
    </Section>
  );
}
