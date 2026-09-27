import { describe, it, expect, beforeEach, vi, afterEach } from "vitest";
import { posteId, resetPosteForTests } from "./poste";

describe("posteId (#869)", () => {
  beforeEach(() => {
    localStorage.clear();
    resetPosteForTests();
  });
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("draws an id once and keeps it in localStorage", () => {
    const id = posteId();
    expect(id).toMatch(/^[A-Za-z0-9]{16}$/);
    expect(localStorage.getItem("pdo.poste")).toBe(id);
    expect(posteId()).toBe(id);
  });

  it("reads the id another tab stored", () => {
    localStorage.setItem("pdo.poste", "fromOtherTab");
    expect(posteId()).toBe("fromOtherTab");
  });

  it("replaces a stored value the daemon would reject", () => {
    localStorage.setItem("pdo.poste", "not a token!");
    const id = posteId();
    expect(id).toMatch(/^[A-Za-z0-9]{16}$/);
    expect(localStorage.getItem("pdo.poste")).toBe(id);
  });

  it("falls back to an in-memory id when storage throws", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("disabled");
    });
    const id = posteId();
    expect(id).toMatch(/^[A-Za-z0-9]{16}$/);
    expect(posteId()).toBe(id);
  });
});
