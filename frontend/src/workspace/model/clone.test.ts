import { describe, expect, it } from "vitest";
import type { CloneJob } from "../../shared/generated/CloneJob";
import { isRemoteUrl, progressLine, progressPercent } from "./clone";

const job = (progress: CloneJob["progress"]): CloneJob => ({
  id: "c1",
  url: "https://github.com/acme/analytics.git",
  path: "/home/me/pdo/repos/analytics",
  project_ids: [],
  state: "running",
  reused: false,
  progress,
  error: null,
});

describe("isRemoteUrl", () => {
  it("tells a URL from a path", () => {
    for (const url of [
      "https://github.com/acme/app.git",
      "ssh://git@host:22/acme/app.git",
      "file:///srv/git/app.git",
      "git@github.com:acme/shop-web.git",
      "git+ssh://git@host/acme/app.git",
    ]) {
      expect(isRemoteUrl(url), url).toBe(true);
    }
    for (const path of ["/home/me/app", "~/code/app", "app", "https://", "git@host:", "ftp://h/a", "git+https://h/a", ""]) {
      expect(isRemoteUrl(path), path).toBe(false);
    }
  });
});

describe("progress", () => {
  it("reads as one line, the bar going on through the deltas", () => {
    expect(progressLine(job(null))).toBe("Starting the clone…");
    const receiving = job({ phase: "Receiving objects", percent: 64, size: "38.20 MiB" });
    expect(progressLine(receiving)).toBe("Receiving objects · 64 % · 38.20 MiB");
    expect(progressPercent(receiving)).toBeCloseTo(57.6);
    const resolving = job({ phase: "Resolving deltas", percent: 50, size: null });
    expect(progressLine(resolving)).toBe("Resolving deltas · 50 %");
    expect(progressPercent(resolving)).toBe(95);
    expect(progressPercent(job({ phase: "Counting objects", percent: 80, size: null }))).toBe(0);
  });
});
