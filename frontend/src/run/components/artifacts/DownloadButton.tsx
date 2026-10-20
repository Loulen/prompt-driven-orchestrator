import { Download } from "lucide-react";
import { Tooltip, TooltipProvider } from "../../../shared/ui/tooltip";

export const DOWNLOAD_FILE = "Download";

/**
 * The « download » icon of an artifact: a port row, a file chip, the artifact
 * modal header. A plain link to the attachment URL — the daemon names the file
 * (`Content-Disposition`), so the browser saves it without opening it.
 */
export default function DownloadButton({
  href,
  label = DOWNLOAD_FILE,
  className = "",
  testId = "artifact-download",
}: {
  href: string;
  label?: string;
  className?: string;
  testId?: string;
}) {
  return (
    // Own provider, as CopyButton: mounted by surfaces outside the app-level one.
    <TooltipProvider>
      <Tooltip content={label}>
        <a
          href={href}
          download
          aria-label={label}
          data-testid={testId}
          // Never the row's click: a download must not also open the artifact.
          onClick={(e) => e.stopPropagation()}
          className={`flex items-center rounded p-1 text-fg-3 hover:bg-bg-3 hover:text-fg ${className}`}
        >
          <Download size={13} aria-hidden="true" />
        </a>
      </Tooltip>
    </TooltipProvider>
  );
}
