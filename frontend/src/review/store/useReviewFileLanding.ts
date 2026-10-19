import { useEffect, useRef } from "react";
import type { DiffFile } from "../../shared/generated/DiffFile";
import { filePath } from "../../run/model/runRefs";

/**
 * The file a Review link names (`/runs/<id>/review?file=<path>`, written by a reading
 * guide): once the diff is first shown, land on that file, or on the first file under
 * it when it names a folder. A path the diff does not hold leaves the page at its top.
 */
export function landingIndex(files: DiffFile[], wanted: string): number {
  const exact = files.findIndex((f) => filePath(f) === wanted);
  if (exact >= 0) return exact;
  const folder = `${wanted.replace(/\/+$/, "")}/`;
  return files.findIndex((f) => filePath(f).startsWith(folder));
}

export function useReviewFileLanding(files: DiffFile[], land: (index: number) => void): void {
  const landed = useRef(false);
  useEffect(() => {
    if (landed.current || files.length === 0) return;
    landed.current = true;
    const wanted = new URLSearchParams(window.location.search).get("file");
    if (!wanted) return;
    const index = landingIndex(files, wanted);
    if (index >= 0) land(index);
  }, [files, land]);
}
