import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { FilePath } from "./diffTabParts";
import type { DiffFile } from "../../shared/generated/DiffFile";

function file(path: string): DiffFile {
  return { new_path: path, old_path: null, status: "added", binary: false, additions: 1, deletions: 0, hunks: [] } as unknown as DiffFile;
}

describe("FilePath", () => {
  it("keeps the end of a long file name apart, so it stays visible when cut", () => {
    const { container } = render(<FilePath file={file("src/a_very_long_new_file_number_03.ts")} />);
    expect(screen.getByTitle("a_very_long_new_file_number_03.ts")).toHaveTextContent("a_very_long_new_file_number_03.ts");
    expect(container.querySelector(".pi-dfile__tail")).toHaveTextContent("ber_03.ts");
  });

  it("leaves a short name whole", () => {
    const { container } = render(<FilePath file={file("src/main.rs")} />);
    expect(container.querySelector(".pi-dfile__tail")).toBeNull();
  });
});
