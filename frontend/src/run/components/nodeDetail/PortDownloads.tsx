import { Download } from "lucide-react";
import { artifactDownloadUrl, commonFolder, portZipUrl } from "../../api/artifacts";
import type { FileInfo } from "../../api/nodes";
import DownloadButton from "../artifacts/DownloadButton";

/**
 * A port row's downloads. Any port: its main file, or the zip of a multi-image
 * folder, behind an icon shown on hover. A `files` port: « Download all », the
 * zip of its folder (each file downloads from its own chip).
 */
export default function PortDownloads({
  runId,
  existing,
  isImage,
  isFiles,
  repeated,
}: {
  runId: string;
  /** The port's files on disk. */
  existing: FileInfo[];
  isImage: boolean;
  isFiles: boolean;
  repeated: boolean;
}) {
  if (existing.length === 0) return null;
  const folder = commonFolder(existing.map((f) => f.path));

  if (isFiles) {
    if (!folder) return null;
    return (
      <a
        href={portZipUrl(runId, folder)}
        download
        onClick={(e) => e.stopPropagation()}
        className="relative flex items-center gap-1 rounded border border-line-strong bg-bg-2 px-1.5 py-0.5 text-fg-2 transition-colors hover:border-fg-4 hover:text-fg"
        style={{ fontSize: "10px" }}
        data-testid="port-download-all"
      >
        <Download size={11} aria-hidden="true" />
        Download all
      </a>
    );
  }

  let href: string | null = null;
  if (existing.length === 1 || (!isImage && !repeated)) {
    href = artifactDownloadUrl(runId, existing[0].path);
  } else if (folder) {
    href = portZipUrl(runId, folder);
  }
  if (!href) return null;
  return (
    <DownloadButton
      href={href}
      testId="port-download"
      className="relative -my-1 opacity-0 group-hover:opacity-100 group-focus-within:opacity-100"
    />
  );
}
