import { Download } from "lucide-react";
import type { FileInfo } from "../../api/nodes";
import { fileBadge, formatSize } from "../../model/nodeDetailFormat";

/**
 * One file of a port row: its extension badge, its name and size. A Run's input
 * file (#779) opens in a new tab; a `files` port's file downloads (`download`).
 */
export default function FileChip({
  file,
  href,
  download = false,
  testId,
}: {
  file: FileInfo;
  href: string;
  download?: boolean;
  testId: string;
}) {
  const name = file.path.split("/").pop() ?? file.path;
  let title = name;
  if (download) {
    title = file.inherited ? `${name} — present before this execution started` : `Download ${name}`;
  }
  return (
    <a
      href={href}
      {...(download ? { download: true } : { target: "_blank", rel: "noreferrer" })}
      // Above the row's open target: a chip never opens the artifact modal.
      onClick={(e) => e.stopPropagation()}
      className={`relative flex h-9 max-w-[220px] items-center gap-1.5 rounded border border-line bg-bg-0 px-1.5 transition-colors hover:border-fg-4${
        file.inherited ? " opacity-60" : ""
      }`}
      title={title}
      data-testid={testId}
    >
      <span
        className="grid h-6 w-6 flex-shrink-0 place-items-center rounded border border-line bg-bg-3 font-mono font-medium text-fg-3"
        style={{ fontSize: "7.5px" }}
        aria-hidden="true"
      >
        {fileBadge(name)}
      </span>
      <span className="flex min-w-0 flex-col leading-tight">
        <span className="truncate text-fg-2" style={{ fontSize: "10.5px" }}>
          {name}
        </span>
        {file.size != null && (
          <span className="font-mono text-fg-4" style={{ fontSize: "9px" }}>
            {formatSize(file.size)}
          </span>
        )}
      </span>
      {download && (
        <Download size={11} className="flex-shrink-0 text-fg-4" aria-hidden="true" />
      )}
    </a>
  );
}
