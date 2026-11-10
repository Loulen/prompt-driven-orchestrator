import { Download } from "lucide-react";
import { Tooltip } from "../../../shared/ui/tooltip";

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
  size = "sm",
}: {
  href: string;
  label?: string;
  className?: string;
  testId?: string;
  /** 20 px in a row (`sm`), 24 px in a header bar (`md`). */
  size?: "sm" | "md";
}) {
  const box = size === "md" ? "h-(--h-control) w-(--h-control)" : "h-(--h-control-sm) w-(--h-control-sm)";
  return (
    // Own provider, as CopyButton: mounted by surfaces outside the app-level one.
    <>
      <Tooltip content={label}>
        <a
          href={href}
          download
          aria-label={label}
          data-testid={testId}
          // Never the row's click: a download must not also open the artifact.
          onClick={(e) => e.stopPropagation()}
          className={`flex ${box} items-center justify-center rounded text-text-3 hover:bg-hover hover:text-text ${className}`}
        >
          <Download size={14} strokeWidth={1.5} aria-hidden="true" />
        </a>
      </Tooltip>
    </>
  );
}
