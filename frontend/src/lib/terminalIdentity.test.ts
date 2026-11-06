import { describe, expect, it } from "vitest";
import { sessionChipLabel, terminalTitle } from "./terminalIdentity";

describe("terminalTitle", () => {
  it("names the Run first, then the subject", () => {
    expect(terminalTitle("US #5 - Sécurité", "Grill")).toBe("US #5 - Sécurité › Grill");
  });

  it("falls back to the subject when the Run has no name", () => {
    expect(terminalTitle(null, "Grill")).toBe("Grill");
    expect(terminalTitle(undefined, "Manager")).toBe("Manager");
    expect(terminalTitle("   ", "Shell")).toBe("Shell");
  });
});

describe("sessionChipLabel", () => {
  const session = "pdo-20261102-163157-15c4172-grill-iter-1";

  it("gives the full tmux session name", () => {
    expect(sessionChipLabel(session, null)).toContain(session);
  });

  it("says when the name was copied, and keeps it readable when copy failed", () => {
    expect(sessionChipLabel(session, "copied")).toBe("Session name copied");
    expect(sessionChipLabel(session, "failed")).toContain(session);
  });
});
