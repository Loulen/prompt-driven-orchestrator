import { describe, expect, it } from "vitest";
import { dockerfileStartDir, relativiseToHome } from "./paths";

describe("relativiseToHome (#432)", () => {
  it("keeps what lives under $HOME, relative and without a trailing slash", () => {
    expect(relativiseToHome("/home/user/.gitconfig", "/home/user")).toBe(".gitconfig");
    expect(relativiseToHome("/home/user/.config/gh/", "/home/user/")).toBe(".config/gh");
  });

  it("refuses $HOME itself, a sibling home, anything outside, and an unknown home", () => {
    expect(relativiseToHome("/home/user", "/home/user")).toBeNull();
    expect(relativiseToHome("/home/user2/.gitconfig", "/home/user")).toBeNull();
    expect(relativiseToHome("/etc/passwd", "/home/user")).toBeNull();
    expect(relativiseToHome("/home/user/.gitconfig", null)).toBeNull();
  });
});

describe("dockerfileStartDir (#431)", () => {
  it("opens the explorer on the folder of an absolute path", () => {
    expect(dockerfileStartDir("/repo/docker/Dockerfile")).toBe("/repo/docker");
    expect(dockerfileStartDir("/Dockerfile")).toBe("/");
  });

  it("falls back to the daemon's default for anything else", () => {
    expect(dockerfileStartDir(null)).toBeUndefined();
    expect(dockerfileStartDir("relative/Dockerfile")).toBeUndefined();
  });
});
