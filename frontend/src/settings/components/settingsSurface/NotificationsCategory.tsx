import { Lock, Send, TriangleAlert, Volume2 } from "lucide-react";
import type { ReactNode } from "react";
import { TEST_CHIME } from "../../../run/model/chimes";
import { TEST_NOTIFICATION, type AlertPermission } from "../../../run/model/waitingAlert";
import type { AlertScope } from "../../../run/model/alertScope";
import { showNotification } from "../../../run/store/notificationAdapter";
import { playChime } from "../../../run/store/soundAdapter";
import {
  setAlertProject,
  setAlertScope,
  setNotificationEnabled,
  setSoundEnabled,
  useWaitingAlertSettings,
} from "../../../run/store/waitingAlertSettings";
import { Button, Checkbox, Segmented, Switch } from "../../../shared/ui/kit";
import type { Project } from "../../../workspace/model/project";
import type { SettingsSection } from "../../model/settingsCategories";
import { SettingRow } from "./fields";
import Section from "./Section";
import { brandName } from "../../../shared/ui/brand/brand";

export const WAITING_ALERTS_DESCRIPTION =
  `Alert this browser when a node starts waiting for you and no ${brandName()} tab is in view. Saved on this device only.`;

export const SOUND_NOTE = "A different chime per reason, at most once every 10 s. Needs no permission.";

const SCOPE_ITEMS: { value: AlertScope; label: string; description: string }[] = [
  { value: "launched", label: "Runs I launched", description: "The Runs you launched, and the Runs they launched" },
  { value: "projects", label: "Projects", description: "The Runs launched in the Projects you check" },
  { value: "all", label: "All I can see", description: "Every Run your rights let you see" },
];

function currentOrigin(): string {
  try {
    return window.location.origin;
  } catch {
    return "this address";
  }
}

/** The note under « Browser notification »: where the permission stands. */
function permissionNote(permission: AlertPermission, requesting: boolean): { note: ReactNode; tone?: "warn"; state: string } {
  if (requesting) return { note: "Waiting for your browser's answer…", state: "requesting" };
  const warn = (Icon: typeof Lock, text: string, state: string) => ({
    note: (
      <span className="set-note-icon" title={text}>
        <Icon size={11} strokeWidth={1.5} aria-hidden />
        {text}
      </span>
    ),
    tone: "warn" as const,
    state,
  });
  switch (permission) {
    case "granted":
      return { note: "Allowed by this browser.", state: "granted" };
    case "denied":
      return warn(
        TriangleAlert,
        "Blocked by this browser for this site. Allow notifications in the site settings, then come back: the switch unlocks on its own. Sound still works.",
        "denied",
      );
    case "insecure":
      return warn(
        Lock,
        `Browsers only allow notifications on https or localhost. You opened ${brandName()} at ${currentOrigin()}. Sound still works.`,
        "insecure",
      );
    case "unavailable":
      return warn(TriangleAlert, "This browser has no notification support. Sound still works.", "unavailable");
    default:
      return { note: "Turning it on asks your browser for permission.", state: "default" };
  }
}

/**
 * Settings › Notifications (CONTEXT.md « Alerte d'attente »): this browser's
 * switches of the waiting alerts and its Portée d'alerte. Device-local: every
 * control writes this browser's storage at the change, out of the instance
 * form's Save and the dirty set, and renders even when `GET /settings` failed.
 */
export default function NotificationsCategory({
  sections,
  projects,
}: {
  sections: SettingsSection[];
  /** The Projets this Actor may read: the ones the `projects` scope offers. */
  projects: Project[];
}) {
  const { prefs, permission, requesting } = useWaitingAlertSettings();
  // Blocked or impossible here: the switch is greyed and cannot be turned on.
  const blocked = permission === "denied" || permission === "insecure" || permission === "unavailable";
  const notificationOn = prefs.notification && permission === "granted";
  const { note, tone, state } = permissionNote(permission, requesting);

  return (
    <Section section={sections[0]}>
      <p className="set-sec__desc" data-testid="waiting-alerts-description">
        {WAITING_ALERTS_DESCRIPTION}
      </p>
      <SettingRow
        label="Browser notification"
        help={`A system notification when a node of a Run in your scope starts waiting for you (a question, a completion to approve, a Run paused on an incident) and no ${brandName()} tab is in view. Clicking it opens the Run on the node.`}
        keywords="notifications alert permission"
        scope="device"
        testId="setting-row-browser-notification"
        note={<span data-state={state}>{note}</span>}
        noteTone={tone}
        noteTestId="setting-browser-notification-status"
      >
        <Switch
          label="Browser notification"
          checked={notificationOn}
          disabled={blocked || requesting}
          onChange={(on) => void setNotificationEnabled(on)}
        />
        <Button
          size="sm"
          tone="ghost"
          icon={Send}
          disabledReason={notificationOn ? undefined : "Turn on the browser notification to test it"}
          onClick={() => showNotification(TEST_NOTIFICATION)}
          data-testid="setting-browser-notification-test"
        >
          Test
        </Button>
      </SettingRow>
      <SettingRow
        label="Sound"
        help="A short chime under the same conditions, one per reason. At most one every 10 seconds, however many nodes start waiting."
        keywords="chime alert audio"
        scope="device"
        testId="setting-row-sound"
        note={SOUND_NOTE}
        noteTestId="setting-sound-status"
      >
        <Switch label="Sound" checked={prefs.sound} onChange={setSoundEnabled} />
        <Button
          size="sm"
          tone="ghost"
          icon={Volume2}
          disabledReason={prefs.sound ? undefined : "Turn on the sound to test it"}
          // Not throttled, not coordinated, even with this tab in view: it is a test.
          onClick={() => playChime(TEST_CHIME)}
          data-testid="setting-sound-test"
        >
          Test
        </Button>
      </SettingRow>
      <SettingRow
        label="Alert me for"
        help="Which Runs may alert this browser. It only narrows what your rights let you see; a Run launched by another Run follows the Run at the top."
        keywords="scope portée runs projects"
        scope="device"
        testId="setting-row-alert-scope"
      >
        <Segmented<AlertScope> label="Alert me for" size="sm" value={prefs.scope} onChange={setAlertScope} items={SCOPE_ITEMS} />
      </SettingRow>
      {prefs.scope === "projects" && (
        <SettingRow
          label="Projects"
          help="The Projects whose Runs alert this browser"
          keywords="scope projects"
          scope="device"
          top
          testId="setting-row-alert-projects"
        >
          <div className="set-checks" data-testid="setting-alert-projects">
            {projects.length === 0 ? (
              <span className="set-checks__none">No Project you can see.</span>
            ) : (
              projects.map((project) => (
                <Checkbox
                  key={project.id}
                  label={project.name}
                  checked={prefs.projects.includes(project.id)}
                  onChange={(checked) => setAlertProject(project.id, checked)}
                >
                  {project.name}
                </Checkbox>
              ))
            )}
          </div>
        </SettingRow>
      )}
    </Section>
  );
}
