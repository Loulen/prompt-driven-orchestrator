import type { PortSide } from "../../shared/generated/PortSide";

export function chevronPoints(
  kind: "input" | "output",
  side: PortSide,
): string {
  const inward = kind === "input";
  switch (side) {
    case "left":
      return inward ? "2,2 6,5 2,8" : "6,2 2,5 6,8";
    case "right":
      return inward ? "6,2 2,5 6,8" : "2,2 6,5 2,8";
    case "top":
      return inward ? "2,2 5,6 8,2" : "2,6 5,2 8,6";
    case "bottom":
      return inward ? "2,6 5,2 8,6" : "2,2 5,6 8,2";
  }
}
