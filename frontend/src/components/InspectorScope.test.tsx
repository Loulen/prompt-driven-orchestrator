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

    // #759 tokenised the amber literals so the scope follows the theme. The
    // behaviour is unchanged — the inspector is still amber — so these assert
    // the binding and the amber declaration separately, instead of pinning an
    // rgba triple to the `.panel-r` rule where it no longer lives.
    it("binds .panel-r's accent tint and border to the amber status tokens", () => {
      expect(indexCss).toMatch(/\.panel-r[\s\S]*?--color-acc-bg:\s*var\(--color-st-await-bg\)/);
      expect(indexCss).toMatch(
        /\.panel-r[\s\S]*?--color-acc-border:\s*var\(--color-st-await-border\)/,
      );
    });

    it("declares the amber tint and border as amber in the base palette", () => {
      expect(indexCss).toMatch(/--color-st-await-bg:\s*rgba\(245,\s*158,\s*11/);
      expect(indexCss).toMatch(/--color-st-await-border:\s*rgba\(245,\s*158,\s*11/);
    });

    it("keeps the inspector amber in the light theme too", () => {
      const light = indexCss.slice(indexCss.indexOf(':root[data-theme="light"]'));
      expect(light).toMatch(/--color-st-await-bg:\s*rgba\(143,\s*69,\s*6/);
      expect(light).toMatch(/--color-st-await-border:\s*rgba\(143,\s*69,\s*6/);
    });

    it("defines .panel-r with --color-primary-foreground for legible text on amber", () => {
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
