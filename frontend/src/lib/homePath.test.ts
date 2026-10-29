import { describe, expect, it } from "vitest";
import { tildify, untildify } from "./homePath";

describe("tildify", () => {
  it.each([
    ["/home/me/code/shop-app", "/home/me", "~/code/shop-app"],
    ["/home/me/code/shop-app", "/home/me/", "~/code/shop-app"],
    ["/home/me", "/home/me", "~"],
    ["/home/meadow/code", "/home/me", "/home/meadow/code"],
    ["/srv/repos/shop-app", "/home/me", "/srv/repos/shop-app"],
    ["/home/me/code/shop-app", null, "/home/me/code/shop-app"],
    ["/home/me/code/shop-app", "", "/home/me/code/shop-app"],
  ])("%s under %s reads %s", (path, home, expected) => {
    expect(tildify(path, home)).toBe(expected);
  });
});

describe("untildify", () => {
  it.each([
    ["~/code/shop-app", "/home/me", "/home/me/code/shop-app"],
    ["~/code/shop-app", "/home/me/", "/home/me/code/shop-app"],
    ["~", "/home/me", "/home/me"],
    ["~other/code", "/home/me", "~other/code"],
    ["/srv/repos/shop-app", "/home/me", "/srv/repos/shop-app"],
    ["~/code/shop-app", null, "~/code/shop-app"],
  ])("%s under %s is %s", (path, home, expected) => {
    expect(untildify(path, home)).toBe(expected);
  });
});
