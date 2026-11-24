import type { CSSProperties } from "react";
import { harnessColor } from "../model/color";
import { harnessInitial, harnessMark } from "../model/harnessLogos";

const FILL = { ink: "currentColor", dim: "currentColor" } as const;

/**
 * The logo of a harness: the mark its product publishes for the first-party
 * harnesses, else a square with its initial on its series colour
 * (`harnessColor`). Decorative unless given a `label`.
 */
export default function HarnessLogo({
  harness,
  size = 14,
  label,
  className,
}: {
  harness: string;
  size?: number;
  /** Names the logo for assistive technologies; without it, the logo is decorative. */
  label?: string;
  className?: string;
}) {
  const mark = harnessMark(harness);
  const a11y = label ? { role: "img", "aria-label": label } : { "aria-hidden": true };
  const classes = ["h-logo", className].filter(Boolean).join(" ");
  if (!mark) {
    return (
      <span
        className={`${classes} h-logo--initial`}
        data-harness-logo={harness}
        style={{ "--h-logo-size": `${size}px`, background: harnessColor(harness) } as CSSProperties}
        {...a11y}
      >
        {harnessInitial(harness)}
      </span>
    );
  }
  return (
    <svg
      className={classes}
      data-harness-logo={harness}
      viewBox={mark.viewBox}
      width={size}
      height={size}
      xmlns="http://www.w3.org/2000/svg"
      {...a11y}
    >
      {mark.paths.map((path, i) => (
        <path
          key={i}
          d={path.d}
          // A custom property reads only from a style, not from the `fill` attribute.
          style={{ fill: path.fill === "brand" ? (mark.brand ?? "currentColor") : FILL[path.fill] }}
          fillOpacity={path.fill === "dim" ? 0.45 : undefined}
          fillRule={path.evenOdd ? "evenodd" : undefined}
          clipRule={path.evenOdd ? "evenodd" : undefined}
        />
      ))}
    </svg>
  );
}
