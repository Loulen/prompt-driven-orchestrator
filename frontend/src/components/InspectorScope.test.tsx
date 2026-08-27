/// <reference types="node" />
import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import InspectorTabs from "./InspectorTabs";

const here = dirname(fileURLToPath(import.meta.url));
const indexCss = readFileSync(resolve(here, "../index.css"), "utf-8");

describe("Inspector amber accent scope", () => {
  describe("CSS custom property overrides", () => {
    it("defines .panel-r with --color-acc bound to amber (--color-st-await)", () => {
      expect(indexCss).toContain(".panel-r");
      expect(indexCss).toContain("--color-acc: var(--color-st-await)");
    });

    it("defines .panel-r with --color-acc-bg using amber rgba", () => {
      expect(indexCss).toMatch(/\.panel-r[\s\S]*rgba\(245,\s*158,\s*11/);
    });

    it("defines .panel-r with --color-acc-border using amber rgba", () => {
      expect(indexCss).toContain("--color-acc-border");
      expect(indexCss).toMatch(/--color-acc-border:\s*rgba\(245,\s*158,\s*11/);
    });

    it("defines .panel-r with --color-primary-foreground for dark-on-amber text", () => {
      expect(indexCss).toMatch(/\.panel-r[\s\S]*--color-primary-foreground/);
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
      expect(activeTab.className).toContain("border-acc");
    });

    it("InspectorTabs inactive tab does not use accent border", () => {
      render(
        <InspectorTabs activeTab="run" onTabChange={() => {}}>
          <div>content</div>
        </InspectorTabs>,
      );
      const inactiveTab = screen.getByTestId("inspector-tab-edit");
      expect(inactiveTab.className).not.toContain("border-acc");
    });

    it("styles expected-content boxes with the inspector accent and bounded editor", () => {
      expect(indexCss).toMatch(/\.exp\.expanded[\s\S]*border-color:\s*var\(--color-acc-border\)/);
      expect(indexCss).toMatch(/\.exp textarea[\s\S]*max-height:\s*150px/);
      expect(indexCss).toMatch(/\.exp-trigger:focus-visible[\s\S]*var\(--color-acc\)/);
    });
  });
});
