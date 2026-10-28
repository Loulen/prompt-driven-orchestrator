import type { ReactNode } from "react";
import { X, ChevronLeft, ChevronRight } from "lucide-react";

/** The artifact viewer's header: the port and the file shown (or the caller's
 *  own header), the iteration and file pagers, the download and copy buttons,
 *  the close button. */
export default function ArtifactHeader({
  header,
  portName,
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
    <div className="flex items-center justify-between border-b border-line px-4 py-3">
      {header ?? (
        <div className="flex items-center gap-2">
          <span className="font-medium text-text" style={{ fontSize: "13px" }}>
            {portName}
          </span>
          {path && (
            <span
              className="font-mono text-text-3"
              style={{ fontSize: "10px" }}
            >
              {path}
            </span>
          )}
        </div>
      )}
      <div className="flex items-center gap-2">
        {hasIterNav && (
          <div className="flex items-center gap-1" data-testid="iter-nav">
            <button
              data-testid="iter-prev"
              onClick={goPrevIter}
              disabled={iterIndex <= 0}
              className="rounded p-0.5 text-text-3 hover:bg-hover hover:text-text disabled:text-text-3"
              aria-label="Previous iteration"
            >
              <ChevronLeft size={14} />
            </button>
            <span
              className="font-mono text-text-3"
              style={{ fontSize: "11px" }}
            >
              iter {iter} of {iterNumbers[iterNumbers.length - 1]}
            </span>
            <button
              data-testid="iter-next"
              onClick={goNextIter}
              disabled={iterIndex >= iterNumbers.length - 1}
              className="rounded p-0.5 text-text-3 hover:bg-hover hover:text-text disabled:text-text-3"
              aria-label="Next iteration"
            >
              <ChevronRight size={14} />
            </button>
          </div>
        )}
        {isRepeated && (
          <div className="flex items-center gap-1">
            <button
              onClick={() => setFileIndex(Math.max(0, fileIndex - 1))}
              disabled={fileIndex === 0}
              className="rounded p-0.5 text-text-3 hover:bg-hover hover:text-text disabled:text-text-3"
              aria-label="Previous file"
            >
              <ChevronLeft size={14} />
            </button>
            <span
              className="font-mono text-text-3"
              style={{ fontSize: "11px" }}
            >
              file {fileIndex + 1} of {total}
            </span>
            <button
              onClick={() =>
                setFileIndex(Math.min(total - 1, fileIndex + 1))
              }
              disabled={fileIndex === total - 1}
              className="rounded p-0.5 text-text-3 hover:bg-hover hover:text-text disabled:text-text-3"
              aria-label="Next file"
            >
              <ChevronRight size={14} />
            </button>
          </div>
        )}
        {(downloadButton || copyButton) && (
          <>
            {(hasIterNav || isRepeated) && (
              <span className="h-4 w-px bg-line-strong" aria-hidden="true" />
            )}
            {downloadButton}
            {copyButton}
          </>
        )}
        <button
          onClick={onClose}
          className="rounded p-1 text-text-3 hover:bg-hover hover:text-text"
          aria-label="Close"
        >
          <X size={14} />
        </button>
      </div>
    </div>
  );
}
