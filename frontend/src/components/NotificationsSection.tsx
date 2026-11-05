/**
 * Settings › General › Notifications (#1011, chosen design « compact rows »):
 * the waiting alert's switches. Device-local like Accessibility — each switch
 * writes this browser's localStorage at the click, outside the form's Save and
 * the dirty set (`useWaitingAlertSettings`).
 *
 * Rows: « Browser notification » (its status line tells where the permission
 * stands — default, being asked, granted, blocked, not a secure context),
 * « Sound » (#1012 — independent: no permission, works where the notification
 * cannot), then « Send a test », which fires whatever is on.
 */
import { Bell, Info, Lock, TriangleAlert } from "lucide-react";
import { Tooltip, TooltipProvider } from "./ui/tooltip";
import { useWaitingAlertSettings } from "../hooks/useWaitingAlertSettings";
import { showNotification } from "../lib/notificationAdapter";
import { playChime } from "../lib/soundAdapter";
import { TEST_CHIME } from "../lib/chimes";
import { TEST_NOTIFICATION } from "../lib/waitingAlert";

export const BROWSER_NOTIFICATION_HELP =
  "A system notification when a node of any Run starts waiting for you (a question, a completion to release, a Run paused on an incident) and no PDO tab is in view. Clicking it opens the Run with the node selected.";

export const SOUND_HELP =
  "A short chime under the same conditions. At most one every 10 seconds, however many nodes start waiting.";

const STATUS_STYLE = { fontSize: "10.5px" } as const;

function currentOrigin(): string {
  try {
    return window.location.origin;
  } catch {
    return "this address";
  }
}

function NotificationStatus({
  permission,
  requesting,
}: {
  permission: ReturnType<typeof useWaitingAlertSettings>["permission"];
  requesting: boolean;
}) {
  const testId = "setting-browser-notification-status";
  if (requesting) {
    return (
      <div
        className="flex items-center gap-1.5 pl-[34px] text-fg-3"
        style={STATUS_STYLE}
        data-testid={testId}
        data-state="requesting"
      >
        <Bell size={11} className="animate-pulse text-acc" />
        Waiting for your browser's answer — look near the address bar.
      </div>
    );
  }
  if (permission === "denied") {
    return (
      <div
        className="flex items-start gap-1.5 pl-[34px] text-st-await"
        style={STATUS_STYLE}
        data-testid={testId}
        data-state="denied"
      >
        <TriangleAlert size={11} className="mt-px shrink-0" />
        <span>
          Blocked by this browser for this site. Allow notifications in the site settings (the icon
          left of the address), then come back — the switch unlocks on its own.
        </span>
      </div>
    );
  }
  if (permission === "insecure") {
    return (
      <div
        className="flex items-start gap-1.5 pl-[34px] text-st-await"
        style={STATUS_STYLE}
        data-testid={testId}
        data-state="insecure"
      >
        <Lock size={11} className="mt-px shrink-0" />
        <span>
          Browsers only allow notifications on https or localhost. You opened PDO at{" "}
          <code className="font-mono">{currentOrigin()}</code> — open it through localhost (an SSH
          tunnel works) to turn this on. Sound still works.
        </span>
      </div>
    );
  }
  if (permission === "unavailable") {
    return (
      <div
        className="flex items-start gap-1.5 pl-[34px] text-st-await"
        style={STATUS_STYLE}
        data-testid={testId}
        data-state="unavailable"
      >
        <TriangleAlert size={11} className="mt-px shrink-0" />
        <span>This browser has no notification support. Sound still works.</span>
      </div>
    );
  }
  if (permission === "granted") {
    return (
      <div
        className="flex items-center gap-1.5 pl-[34px] text-st-done"
        style={STATUS_STYLE}
        data-testid={testId}
        data-state="granted"
      >
        <span className="h-1.5 w-1.5 rounded-full bg-st-done" aria-hidden />
        Allowed by this browser.
      </div>
    );
  }
  return (
    <div className="pl-[34px] text-fg-4" style={STATUS_STYLE} data-testid={testId} data-state="default">
      Turning it on asks your browser for permission.
    </div>
  );
}

export default function NotificationsSection() {
  const { prefs, permission, requesting, setNotificationEnabled, setSoundEnabled } =
    useWaitingAlertSettings();
  // Blocked or impossible here: the switch is greyed and cannot be turned on.
  const blocked = permission === "denied" || permission === "insecure" || permission === "unavailable";
  const notificationOn = prefs.notification && permission === "granted";
  const soundOn = prefs.sound;
  const nothingOn = !notificationOn && !soundOn;

  const sendTest = () => {
    if (notificationOn) {
      // Its click only brings the tab back: no Run to open.
      showNotification(TEST_NOTIFICATION);
    }
    // The chime of a question — not throttled, not coordinated: it is a test.
    if (soundOn) playChime(TEST_CHIME);
  };

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-1">
        <div className="flex items-center gap-2.5">
          <button
            type="button"
            role="switch"
            id="setting-browser-notification"
            aria-checked={notificationOn}
            aria-describedby="setting-browser-notification-help"
            data-testid="setting-browser-notification"
            disabled={blocked || requesting}
            onClick={() => void setNotificationEnabled(!notificationOn)}
            className={`relative h-3.5 w-6 shrink-0 rounded-full transition-colors disabled:cursor-not-allowed disabled:opacity-40 ${
              notificationOn ? "bg-acc" : "bg-fg-5"
            }`}
          >
            <span
              className={`absolute top-0.5 h-2.5 w-2.5 rounded-full bg-bg-1 transition-all ${
                notificationOn ? "left-3" : "left-0.5"
              }`}
            />
          </button>
          <label
            htmlFor="setting-browser-notification"
            className={`font-medium ${blocked ? "cursor-not-allowed text-fg-4" : "cursor-pointer text-fg-2"}`}
            style={{ fontSize: "11.5px" }}
          >
            Browser notification
          </label>
          <TooltipProvider>
            <Tooltip content={BROWSER_NOTIFICATION_HELP} side="right">
              <button
                type="button"
                aria-label="About browser notifications"
                data-testid="setting-browser-notification-info"
                className="grid cursor-help place-items-center text-fg-4 hover:text-fg-2"
              >
                <Info size={12} />
              </button>
            </Tooltip>
          </TooltipProvider>
          <span id="setting-browser-notification-help" className="sr-only">
            {BROWSER_NOTIFICATION_HELP}
          </span>
        </div>
        <NotificationStatus permission={permission} requesting={requesting} />
      </div>

      <div className="flex flex-col gap-1">
        <div className="flex items-center gap-2.5">
          <button
            type="button"
            role="switch"
            id="setting-sound"
            aria-checked={soundOn}
            aria-describedby="setting-sound-help"
            data-testid="setting-sound"
            onClick={() => setSoundEnabled(!soundOn)}
            className={`relative h-3.5 w-6 shrink-0 rounded-full transition-colors disabled:cursor-not-allowed disabled:opacity-40 ${
              soundOn ? "bg-acc" : "bg-fg-5"
            }`}
          >
            <span
              className={`absolute top-0.5 h-2.5 w-2.5 rounded-full bg-bg-1 transition-all ${
                soundOn ? "left-3" : "left-0.5"
              }`}
            />
          </button>
          <label
            htmlFor="setting-sound"
            className="cursor-pointer font-medium text-fg-2"
            style={{ fontSize: "11.5px" }}
          >
            Sound
          </label>
          <TooltipProvider>
            <Tooltip content={SOUND_HELP} side="right">
              <button
                type="button"
                aria-label="About the sound"
                data-testid="setting-sound-info"
                className="grid cursor-help place-items-center text-fg-4 hover:text-fg-2"
              >
                <Info size={12} />
              </button>
            </Tooltip>
          </TooltipProvider>
          <span id="setting-sound-help" className="sr-only">
            {SOUND_HELP}
          </span>
        </div>
        <div className="pl-[34px] text-fg-4" style={STATUS_STYLE} data-testid="setting-sound-status">
          A short chime, at most once every 10 s. Needs no permission.
        </div>
      </div>

      <div className="flex items-center gap-3">
        <button
          type="button"
          onClick={sendTest}
          disabled={nothingOn}
          data-testid="setting-waiting-alert-test"
          title={nothingOn ? "Turn on the browser notification or the sound to send a test." : undefined}
          className="flex items-center gap-1.5 rounded border border-line-strong bg-bg-3 px-2.5 py-1.5 text-fg-2 hover:border-acc disabled:cursor-not-allowed disabled:opacity-40"
          style={{ fontSize: 11 }}
        >
          <Bell size={11} />
          Send a test
        </button>
        <span className="text-fg-4" style={STATUS_STYLE}>
          Fires now, even with this tab in view.
        </span>
      </div>
    </div>
  );
}
