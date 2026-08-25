import "@testing-library/jest-dom/vitest";
import { afterEach } from "vitest";
import { useSelectionStore } from "../stores/selectionStore";

// #577 — the multi-select store is a module singleton shared across every test;
// reset it after each so a selection made in one test can't leak a floating bar
// or a tab badge into the next.
afterEach(() => {
  useSelectionStore.getState().clearAll();
});
