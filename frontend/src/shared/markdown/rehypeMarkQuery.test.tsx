import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";
import Markdown from "react-markdown";
import { highlightParts, rehypeMarkQuery } from "./rehypeMarkQuery";

describe("highlightParts", () => {
  it("cuts the text around every match, ignoring case", () => {
    expect(highlightParts("The Debounce and the debounce.", "debounce")).toEqual([
      { text: "The ", match: false },
      { text: "Debounce", match: true },
      { text: " and the ", match: false },
      { text: "debounce", match: true },
      { text: ".", match: false },
    ]);
  });

  it("reads the query literally", () => {
    expect(highlightParts("a (x) b", "(x)")).toEqual([
      { text: "a ", match: false },
      { text: "(x)", match: true },
      { text: " b", match: false },
    ]);
  });

  it("leaves the text whole without a query or a match", () => {
    expect(highlightParts("abc", "  ")).toEqual([{ text: "abc", match: false }]);
    expect(highlightParts("abc", "z")).toEqual([{ text: "abc", match: false }]);
  });
});

describe("rehypeMarkQuery", () => {
  it("marks every match in the rendered Markdown, inline code included", () => {
    const { container } = render(
      <Markdown rehypePlugins={[[rehypeMarkQuery, { query: "debounce" }]]}>
        {"A **Debounce** here, `useDebounce` there."}
      </Markdown>,
    );
    const marks = [...container.querySelectorAll("mark")].map((m) => m.textContent);
    expect(marks).toEqual(["Debounce", "Debounce"]);
    expect(container.textContent).toBe("A Debounce here, useDebounce there.");
  });

  it("changes nothing without a query", () => {
    const { container } = render(
      <Markdown rehypePlugins={[[rehypeMarkQuery, { query: "" }]]}>{"plain"}</Markdown>,
    );
    expect(container.querySelector("mark")).toBeNull();
  });
});
