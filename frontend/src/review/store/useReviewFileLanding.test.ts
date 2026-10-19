import { describe, expect, it } from "vitest";
import type { DiffFile } from "../../shared/generated/DiffFile";
import { landingIndex } from "./useReviewFileLanding";

const file = (path: string) => ({ new_path: path, old_path: path }) as DiffFile;

describe("the file a Review link lands on", () => {
  const files = [file("README.md"), file("frontend/proto/a.tsx"), file("frontend/proto/b.tsx")];

  it("is the file it names, or the first one under the folder it names", () => {
    expect(landingIndex(files, "frontend/proto/b.tsx")).toBe(2);
    expect(landingIndex(files, "frontend/proto/")).toBe(1);
    expect(landingIndex(files, "frontend/proto")).toBe(1);
    expect(landingIndex(files, "nowhere")).toBe(-1);
  });
});
