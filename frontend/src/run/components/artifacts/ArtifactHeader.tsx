import type { ReactNode } from "react";
import { ChevronLeft, ChevronRight, FileCode2, FileImage, FileText, X } from "lucide-react";
import type { PortType } from "../../../shared/generated/PortType";
import { IconButton } from "../../../shared/ui/kit";

/** The port's kind at a glance: an image, an html page, or text. */
function TypeIcon({ portType }: { portType?: PortType }) {
  const Icon =
    portType === "image" || portType === "image_list" ? FileImage : portType === "html" ? FileCode2 : FileText;
  return <Icon size={14} strokeWidth={1.5} aria-hidden />;
}

/** The artifact viewer's header bar: the port and the file shown (or the
 *  caller's own header), the iteration and file pagers, the download and copy
 *  buttons, the close button. */
export default function ArtifactHeader({
  header,
  portName,
  portType,
  path,
  hasIterNav,
  iter,
  iterIndex,
  iterNumbers,
  goPrevIter,
  goNextIter,
  isRepeated,
  fileIndex,
  total,
  setFileIndex,
  downloadButton,
  copyButton,
  onClose,
}: {
  header?: ReactNode;
  portName: string;
  portType?: PortType;
  path?: string;
  hasIterNav: boolean;
  iter: number;
  iterIndex: number;
  iterNumbers: number[];
  goPrevIter: () => void;
  goNextIter: () => void;
  isRepeated: boolean;
  fileIndex: number;
  total: number;
  setFileIndex: (index: number) => void;
  /** The displayed file's download, absent where there is no file. */
  downloadButton?: ReactNode;
  /** #41: the « copy raw file » button, absent where nothing can be copied. */
  copyButton?: ReactNode;
  onClose: () => void;
}) {
  return (
    <header className="art__head">
      {header ?? (
        <span className="art__title">
          <TypeIcon portType={portType} />
          <span className="art__port" title={portName}>
            {portName}
          </span>
          {path && (
            <span className="art__path" title={path}>
              {path}
            </span>
          )}
        </span>
      )}
      {hasIterNav && (
        <span className="art__pager tnum" data-testid="iter-nav">
          <IconButton
            icon={ChevronLeft}
            label="Previous iteration"
            size="sm"
            onClick={goPrevIter}
            disabled={iterIndex <= 0}
            data-testid="iter-prev"
          />
          <span>
            iter {iter} of {iterNumbers[iterNumbers.length - 1]}
          </span>
          <IconButton
            icon={ChevronRight}
            label="Next iteration"
            size="sm"
            onClick={goNextIter}
            disabled={iterIndex >= iterNumbers.length - 1}
            data-testid="iter-next"
          />
        </span>
      )}
      {isRepeated && (
        <span className="art__pager tnum" data-testid="file-nav">
          <IconButton
            icon={ChevronLeft}
            label="Previous file"
            kbd="←"
            size="sm"
            onClick={() => setFileIndex(Math.max(0, fileIndex - 1))}
            disabled={fileIndex === 0}
            data-testid="file-prev"
          />
          <span>
            file {fileIndex + 1} of {total}
          </span>
          <IconButton
            icon={ChevronRight}
            label="Next file"
            kbd="→"
            size="sm"
            onClick={() => setFileIndex(Math.min(total - 1, fileIndex + 1))}
            disabled={fileIndex === total - 1}
            data-testid="file-next"
          />
        </span>
      )}
      <span className="d-grow" />
      {downloadButton}
      {copyButton}
      <span className="art__sep" aria-hidden="true" />
      <IconButton icon={X} label="Close" kbd="Esc" onClick={onClose} data-testid="artifact-close" />
    </header>
  );
}
