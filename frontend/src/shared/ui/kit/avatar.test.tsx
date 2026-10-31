import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { Actor } from "../../generated/Actor";
import { Avatar, AvatarStack } from "./avatar";
import { AVATAR_HUES, avatarHue, avatarIdentity, personColor } from "./avatarHue";
import { initials } from "./util";

const alice: Actor = { kind: "identified", namespace: "test", id: "alice" };
const bob: Actor = { kind: "identified", namespace: "test", id: "bob" };

describe("the avatar's initials", () => {
  it("take the first and the last word, one rule for every surface", () => {
    expect(initials("Alice Martin")).toBe("AM");
    expect(initials("Ada Byron Lovelace")).toBe("AL");
    expect(initials("bob")).toBe("B");
    expect(initials("  ")).toBe("?");
    expect(initials("élise durand")).toBe("ÉD");
  });
});

describe("the avatar's hue", () => {
  it("is one of the 8 hues, the same for the same identity", () => {
    for (const id of ["identified:test/alice", "local_operator", "Alice Martin", ""]) {
      const hue = avatarHue(id);
      expect(hue).toBeGreaterThanOrEqual(0);
      expect(hue).toBeLessThan(AVATAR_HUES);
      expect(avatarHue(id)).toBe(hue);
    }
    expect(avatarHue("Alice Martin")).toBe(avatarHue("  alice martin "));
  });

  it("spreads people over the hues", () => {
    const hues = new Set(Array.from({ length: 64 }, (_, i) => avatarHue(`identified:test/user-${i}`)));
    expect(hues.size).toBe(AVATAR_HUES);
  });

  it("follows the Actor, whatever its name says", () => {
    expect(avatarIdentity("Alice", alice)).toBe(avatarIdentity("Alice Martin", alice));
    expect(avatarIdentity("Alice", null)).toBe("Alice");
  });

  it("colours the person's marks with the avatar's hue", () => {
    const hue = avatarHue(avatarIdentity("Alice", alice));
    expect(personColor({ name: "Alice", actor: alice })).toBe(`oklch(var(--av-solid-l) var(--av-solid-c) var(--av-h${hue}))`);
    expect(personColor({ actor: alice })).toBe(personColor({ name: "Someone else", actor: alice }));
    expect(personColor({ name: "Bob" })).toContain(`--av-h${avatarHue("Bob")}`);
  });
});

describe("Avatar", () => {
  it("draws the same hue and initials for the same person, whatever the size or the name shown", () => {
    render(
      <>
        <Avatar name="Alice Martin" actor={alice} size={16} data-testid="a" />
        <Avatar name="Alice Martin" actor={alice} size={24} hollow data-testid="b" />
        <Avatar name="Bob Stone" actor={bob} data-testid="c" />
      </>,
    );
    const [a, b] = [screen.getByTestId("a"), screen.getByTestId("b")];
    expect(a.dataset.hue).toBe(b.dataset.hue);
    expect(a.textContent).toBe("AM");
    expect(b.textContent).toBe("AM");
    expect(a.dataset.hue).toBe(String(avatarHue("identified:test/alice")));
    expect(screen.getByTestId("c").dataset.hue).toBe(String(avatarHue("identified:test/bob")));
  });

  it("names the person, or its tooltip", () => {
    render(<Avatar name="Alice Martin" title="Alice Martin · Viewing now" present />);
    const av = screen.getByRole("img", { name: "Alice Martin · Viewing now" });
    expect(av).toHaveAttribute("title", "Alice Martin · Viewing now");
    expect(av).toHaveAttribute("data-present");
  });

  it("keeps a neutral fill and the glyph for an agent or a trigger", () => {
    render(
      <>
        <Avatar name="implement" actor={{ kind: "agent", run_id: "r", node_id: "n" }} data-testid="agent" />
        <Avatar name="nightly" kind="trigger" data-testid="trigger" />
      </>,
    );
    for (const id of ["agent", "trigger"]) {
      const av = screen.getByTestId(id);
      expect(av.dataset.hue).toBeUndefined();
      expect(av.querySelector("svg")).not.toBeNull();
    }
    expect(screen.getByTestId("agent").dataset.kind).toBe("agent");
  });

  it("marks an account turned off, dashed", () => {
    render(<Avatar name="Olivier Lambert" off data-testid="off" />);
    expect(screen.getByTestId("off")).toHaveAttribute("data-off");
  });
});

describe("AvatarStack", () => {
  const twelve = Array.from({ length: 12 }, (_, i) => ({ name: `Person ${String.fromCharCode(65 + i)}` }));

  it("draws three, overlapping, then « +n » naming the others one per line", () => {
    render(<AvatarStack people={twelve} data-testid="stack" />);
    const stack = screen.getByTestId("stack");
    const avatars = stack.querySelectorAll(".d-av");
    expect(avatars).toHaveLength(3);
    expect(avatars[0]).not.toHaveAttribute("data-stacked");
    expect(avatars[1]).toHaveAttribute("data-stacked");
    const more = screen.getByRole("img", { name: /^9 more:/ });
    expect(more.textContent).toBe("+9");
    expect(more.getAttribute("title")!.split("\n")).toEqual(twelve.slice(3).map((p) => p.name));
  });

  it("draws nothing without anybody, and no « +n » up to the max", () => {
    const { container } = render(<AvatarStack people={[]} />);
    expect(container.firstChild).toBeNull();
    render(<AvatarStack people={twelve.slice(0, 3)} data-testid="three" />);
    expect(screen.getByTestId("three").textContent).not.toContain("+");
  });
});
