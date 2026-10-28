/// <reference types="node" />
import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import InspectorTabs from "./InspectorTabs";

const here = dirname(fileURLToPath(import.meta.url));
const indexCss = readFileSync(resolve(here, "../../index.css"), "utf-8");

describe("Inspector accent", () => {
  // The design system has one accent, ruby, for the selection and the one
  // primary action of a view: the right pane re-tints nothing.
  it("draws the right pane with the app's ruby, no amber scope", () => {
    expect(indexCss).not.toContain(".panel-r");
  });

  describe("inspector components use accent tokens", () => {
    it("marks the active tab, and only it, for the ruby underline", () => {
      render(
        <InspectorTabs activeTab="run" onTabChange={() => {}}>
          <div>content</div>
        </InspectorTabs>,
      );
      expect(screen.getByTestId("inspector-tab-run")).toHaveAttribute("aria-selected", "true");
      expect(screen.getByTestId("inspector-tab-edit")).toHaveAttribute("aria-selected", "false");
    });

    it("styles expected-content boxes with the inspector accent and bounded editor", () => {
      expect(indexCss).toMatch(
        /\.exp\.expanded[\s\S]*border-color:\s*color-mix\(in oklab, var\(--primary\) 30%, transparent\)/,
      );
      expect(indexCss).toMatch(/\.exp textarea[\s\S]*max-height:\s*150px/);
      expect(indexCss).toMatch(/\.exp-trigger:focus-visible[\s\S]*var\(--primary\)/);
    });
  });
});
