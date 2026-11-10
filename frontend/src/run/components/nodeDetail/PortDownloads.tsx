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
        className="d-btn d-btn--sm"
        title="Download all"
        data-testid="port-download-all"
      >
        <Download size={12} strokeWidth={1.5} aria-hidden="true" />
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
    />
  );
}
