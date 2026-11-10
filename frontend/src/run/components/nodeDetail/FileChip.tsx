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
      className={`relative flex h-6 max-w-[220px] items-center gap-1.5 rounded border border-line bg-surface-0 px-1 transition-colors hover:border-text-3${
        file.inherited ? " opacity-60" : ""
      }`}
      title={title}
      data-testid={testId}
    >
      <span className="flex-shrink-0 font-mono text-2xs font-medium text-text-3" aria-hidden="true">
        {fileBadge(name)}
      </span>
      <span className="truncate text-xs text-text-2">{name}</span>
      {file.size != null && <span className="shrink-0 font-mono text-2xs text-text-3">{formatSize(file.size)}</span>}
      {download && (
        <Download size={11} className="flex-shrink-0 text-text-3" aria-hidden="true" />
      )}
    </a>
  );
}
