/// <reference types="node" />
import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import InspectorTabs from "./InspectorTabs";

const here = dirname(fileURLToPath(import.meta.url));
const indexCss = readFileSync(resolve(here, "../../index.css"), "utf-8");

describe("Inspector amber accent scope", () => {
  describe("CSS custom property overrides", () => {
    it("defines .panel-r with --primary bound to amber (--color-st-await)", () => {
      expect(indexCss).toContain(".panel-r");
      expect(indexCss).toContain("--primary: var(--color-st-await)");
    });

    // The scope re-tints the roles, so every utility inside follows: the
    // selection fill and edge turn amber with the accent.
    it("binds .panel-r's selection fill and edge to the amber status colour", () => {
      expect(indexCss).toMatch(
        /\.panel-r[\s\S]*?--selected:\s*color-mix\(in oklab, var\(--color-st-await\) 14%, transparent\)/,
      );
      expect(indexCss).toMatch(/\.panel-r[\s\S]*?--selected-edge:\s*var\(--color-st-await\)/);
    });

    it("defines .panel-r with --primary-ink for legible text on amber", () => {
      expect(indexCss).toMatch(/\.panel-r[\s\S]*--primary-ink/);
    });
  });

  describe("inspector components use accent tokens", () => {
    it("InspectorTabs active tab border uses accent color token", () => {
      render(
        <InspectorTabs activeTab="run" onTabChange={() => {}}>
          <div>content</div>
        </InspectorTabs>,
      );
      const activeTab = screen.getByTestId("inspector-tab-run");
      expect(activeTab.className).toContain("border-primary");
    });

    it("InspectorTabs inactive tab does not use accent border", () => {
      render(
        <InspectorTabs activeTab="run" onTabChange={() => {}}>
          <div>content</div>
        </InspectorTabs>,
      );
      const inactiveTab = screen.getByTestId("inspector-tab-edit");
      expect(inactiveTab.className).not.toContain("border-primary");
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
