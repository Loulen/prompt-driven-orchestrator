import { describe, expect, it } from "vitest";
import type { ExtensionEntry } from "../generated/ExtensionEntry";
import type { UiContribution } from "../generated/UiContribution";
import { hideableContributions } from "./hideableContributions";
import type { LoadedExtension } from "./store";

function loaded(id: string, name: string, contributions: Partial<UiContribution>[], status: LoadedExtension["status"] = "ready"): LoadedExtension {
  const entry = {
    id,
    name,
    version: "1.0.0",
    ui: { bundle_url: "", react_major: 19, ui_module_major: null, contributions, commands: [], locations: [] },
  } as unknown as ExtensionEntry;
  return { entry, status, module: { Badge: () => null, People: () => null } } as unknown as LoadedExtension;
}

const decoration = (location: string, id: string, label: string, component = "Badge") =>
  ({ location, id, label, type: "decoration", component }) as Partial<UiContribution>;

describe("the hideable contributions of a surface", () => {
  it("lists each decoration of its locations as « Extension · label », in the catalogue's order", () => {
    const extensions = [
      loaded("alpha", "Alpha", [decoration("run/item", "tasks", "Tasks"), decoration("trigger/item", "x", "Elsewhere")]),
      loaded("beta", "Beta", [decoration("run/item/people", "people", "People", "People")]),
    ];
    expect(hideableContributions(extensions, "runs.list", ["run/item", "run/item/people"])).toEqual([
      { id: "runs.list.ext.alpha:tasks", label: "Alpha · Tasks", location: "run/item" },
      { id: "runs.list.ext.beta:people", label: "Beta · People", location: "run/item/people" },
    ]);
  });

  it("leaves out a contribution whose bundle is not ready or exports nothing to draw", () => {
    const extensions = [
      loaded("alpha", "Alpha", [decoration("run/item", "tasks", "Tasks")], "loading"),
      loaded("beta", "Beta", [decoration("run/item", "none", "None", "Missing")]),
    ];
    expect(hideableContributions(extensions, "runs.list", ["run/item"])).toEqual([]);
  });
});
