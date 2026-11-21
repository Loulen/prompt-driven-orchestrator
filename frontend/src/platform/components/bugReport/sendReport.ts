import { copyImageToClipboard } from "../../../shared/lib/clipboard";
import { saveBlob } from "../../../shared/lib/download";
import { toast } from "../../../shared/ui/kit";
import type { BugReportSent } from "../../../shared/generated/BugReportSent";
import { sendBugReport } from "../../api/bugReport";
import type { BugReportDestination, BugReportFields } from "../../model/bugReport";
import { reportBody } from "../../model/bugReportDestinations";
import { githubIssueUrl } from "../../model/bugReportGithub";

export const SCREENSHOT_COPIED = "Screenshot copied: paste it in the issue";
export const REPORT_SENT = "Bug report sent";
/** The largest screenshot the daemon takes (`MAX_SCREENSHOT_BYTES`). */
export const MAX_SCREENSHOT_BYTES = 10 * 1024 * 1024;
export const SCREENSHOT_TOO_LARGE = "The screenshot is over 10 MB: crop it, or remove it";
export const SCREENSHOT_NOT_COPIED = "The screenshot could not be copied: attach it to the issue by hand";

/** The extension of a screenshot's file, from its type. */
const extensionOf = (image: Blob) => (image.type.split("/")[1] || "png").replace("jpeg", "jpg");

/**
 * A report to the core's public issues: the screenshot on the clipboard first
 * (the write needs the page focused, which the new tab takes), then the new
 * issue, prefilled, in a new tab. A toast says what happened to the
 * screenshot; where it could not be copied, it offers to download it.
 */
export function sendToGithub(fields: BugReportFields, capture: Blob | null, open: (url: string) => void = openTab): string {
  const copied = capture ? copyImageToClipboard(capture) : null;
  const url = githubIssueUrl(fields);
  open(url);
  if (capture && copied) {
    void copied.then((ok) =>
      ok
        ? toast(SCREENSHOT_COPIED, "success")
        : toast(SCREENSHOT_NOT_COPIED, "error", {
            actionLabel: "Download it",
            onAction: () => saveBlob(capture, `screenshot.${extensionOf(capture)}`),
          }),
    );
  }
  return url;
}

function openTab(url: string) {
  window.open(url, "_blank", "noopener,noreferrer");
}

/** The base64 of a screenshot's bytes, without the `data:` prefix. */
export function base64Of(image: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const url = String(reader.result ?? "");
      resolve(url.slice(url.indexOf(",") + 1));
    };
    reader.onerror = () => reject(reader.error ?? new Error("the screenshot could not be read"));
    reader.readAsDataURL(image);
  });
}

/**
 * A report to an extension's destination, through the daemon: the text the
 * Preview shows, its fields, the screenshot. Resolves with what the
 * destination created; rejects with the daemon's sentence.
 */
export async function sendToExtension(
  destination: BugReportDestination,
  text: string,
  fields: BugReportFields,
  capture: Blob | null,
): Promise<BugReportSent> {
  if (capture && capture.size > MAX_SCREENSHOT_BYTES) throw new Error(SCREENSHOT_TOO_LARGE);
  const screenshot = capture ? { mediaType: capture.type || "image/png", base64: await base64Of(capture) } : null;
  return sendBugReport(reportBody(destination, text, fields, screenshot));
}

/** Follows the link a destination answered: inside the interface (`#/…`), else in a new tab. */
export function openSentLink(link: string) {
  if (link.startsWith("#")) window.location.hash = link;
  else window.open(link, "_blank", "noopener,noreferrer");
}

/** The toast of a report an extension received, with its link. */
export function toastSent(sent: BugReportSent) {
  toast(`${REPORT_SENT}: ${sent.reference}`, "success", {
    actionLabel: "Open",
    onAction: () => openSentLink(sent.link),
  });
}
