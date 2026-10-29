import { artifactUrl } from "../../api/artifacts";
import type { FileInfo } from "../../api/nodes";
import type { PortType } from "../../../shared/generated/PortType";

/** The images of an `image` / `image_list` port: one at a time, or the gallery. */
export default function ImageBody({
  runId,
  files,
  filesLoading,
  fileIndex,
  portType,
  onZoom,
}: {
  runId: string;
  files: FileInfo[];
  filesLoading: boolean;
  fileIndex: number;
  portType: PortType;
  onZoom: (images: string[], index: number) => void;
}) {
  const existingFiles = files.filter((f) => f.exists);

  if (filesLoading) {
    return (
      <span className="art__state">
        Loading...
      </span>
    );
  }

  if (existingFiles.length === 0) {
    return (
      <span className="art__state">
        No image files yet.
      </span>
    );
  }

  if (portType === "image_list") {
    return (
      <div className="flex flex-col gap-3" data-testid="image-gallery">
        {existingFiles.map((f, i) => (
          <div key={f.path} className="flex flex-col gap-1">
            <img
              src={artifactUrl(runId, f.path)}
              alt={f.path.split("/").pop() ?? ""}
              className="max-h-[calc(60vh/var(--app-zoom,1))] w-full cursor-zoom-in border border-line object-contain transition-opacity hover:opacity-90"
              onClick={() =>
                onZoom(
                  existingFiles.map((ef) => artifactUrl(runId, ef.path)),
                  i,
                )
              }
              data-testid={`gallery-image-${i}`}
            />
            <span className="art__cap">
              {f.path.split("/").pop()}
            </span>
          </div>
        ))}
      </div>
    );
  }

  const currentIdx = existingFiles[fileIndex] ? fileIndex : 0;
  const current = existingFiles[currentIdx];
  if (!current) return null;

  return (
    <div className="flex flex-col items-center gap-2" data-testid="image-viewer">
      <img
        src={artifactUrl(runId, current.path)}
        alt={current.path.split("/").pop() ?? ""}
        className="max-h-[calc(60vh/var(--app-zoom,1))] max-w-full cursor-zoom-in border border-line object-contain transition-opacity hover:opacity-90"
        onClick={() =>
          onZoom(
            existingFiles.map((f) => artifactUrl(runId, f.path)),
            currentIdx,
          )
        }
        data-testid="image-viewer-img"
      />
      <span className="art__cap">
        {current.path.split("/").pop()}
      </span>
    </div>
  );
}
