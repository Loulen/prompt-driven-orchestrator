import { copyImageToClipboard } from "../../../shared/lib/clipboard";
import { saveBlob } from "../../../shared/lib/download";
import { toast } from "../../../shared/ui/kit";
import type { BugReportFields } from "../../model/bugReport";
import { githubIssueUrl } from "../../model/bugReportGithub";

export const SCREENSHOT_COPIED = "Screenshot copied: paste it in the issue";
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
