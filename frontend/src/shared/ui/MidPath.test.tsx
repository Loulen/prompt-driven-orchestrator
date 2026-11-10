import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import MidPath from "./MidPath";

describe("MidPath", () => {
  it("keeps the last segment apart from the folders", () => {
    render(<MidPath path="~/work/clients/shop-web" testId="p" />);
    expect(screen.getByTestId("p-head")).toHaveTextContent("~/work/clients");
    expect(screen.getByTestId("p-tail")).toHaveTextContent("/shop-web");
  });

  it("lets the folders shrink to two characters and an ellipsis", () => {
    render(<MidPath path="/tmp/code/clients/shop-web" testId="p" />);
    expect(screen.getByTestId("p").style.getPropertyValue("--ln-path-head")).toBe("3ch");
  });

  it("never holds short folders wider than they are", () => {
    render(<MidPath path="~/shop-web" testId="p" />);
    expect(screen.getByTestId("p").style.getPropertyValue("--ln-path-head")).toBe("1ch");
  });

  it("holds no room for folders a bare name does not have", () => {
    render(<MidPath path="shop-web" testId="p" />);
    expect(screen.queryByTestId("p-head")).toBeNull();
    expect(screen.getByTestId("p").style.getPropertyValue("--ln-path-head")).toBe("");
  });
});
