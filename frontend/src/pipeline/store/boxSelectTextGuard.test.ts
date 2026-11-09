import { afterEach, describe, expect, it } from "vitest";
import { boxSelectTextGuard } from "./boxSelectTextGuard";

// F1-5: a Shift + drag box-select on the canvas must not select page text.

function selectSomeText() {
  const p = document.createElement("p");
  p.textContent = "100%";
  document.body.appendChild(p);
  const range = document.createRange();
  range.selectNodeContents(p);
  window.getSelection()?.addRange(range);
  return p;
}

describe("boxSelectTextGuard", () => {
  afterEach(() => {
    boxSelectTextGuard.onSelectionEnd();
    document.body.innerHTML = "";
  });

  it("turns text selection off for the whole box-select and drops a selection already begun", () => {
    selectSomeText();
    expect(window.getSelection()?.toString()).toBe("100%");
    boxSelectTextGuard.onSelectionStart();
    expect(document.body).toHaveClass("pdo-box-selecting");
    expect(window.getSelection()?.toString()).toBe("");
    boxSelectTextGuard.onSelectionEnd();
    expect(document.body).not.toHaveClass("pdo-box-selecting");
  });

  it("lets text be selected again when the box ends off the pane", () => {
    boxSelectTextGuard.onSelectionStart();
    window.dispatchEvent(new Event("pointerup"));
    expect(document.body).not.toHaveClass("pdo-box-selecting");
  });
});
