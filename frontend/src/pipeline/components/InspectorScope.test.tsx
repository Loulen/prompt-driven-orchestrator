/// <reference types="node" />
import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import InspectorTabs from "./InspectorTabs";

const here = dirname(fileURLToPath(import.meta.url));
const indexCss = readFileSync(resolve(here, "../../index.css"), "utf-8");
const inspectCss = readFileSync(resolve(here, "inspect.css"), "utf-8");

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

    it("bounds the expected-content editor and rings its trigger with the focus token", () => {
      expect(inspectCss).toMatch(/\.ei-exp \.d-area[\s\S]*max-height:\s*150px/);
      expect(inspectCss).toMatch(/\.ei-exp__trigger:focus-visible[\s\S]*var\(--focus-ring\)/);
    });
  });
});
