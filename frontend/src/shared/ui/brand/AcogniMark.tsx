/**
 * The gear of Acogni, the publisher's logo: drawn in the current colour, its
 * triangle in the ruby under every Theme (`brand.css`). Since CORE-225 it is no
 * longer the product's logo (the Marque's mark is, `BrandMark`): it is the
 * Settings icon (`AccentIcons.SettingsIcon`) and signs « by Acogni »
 * (`BySignature`).
 */
import * as React from "react";
import type { LucideProps } from "lucide-react";

export const GEAR =
  "M-4.6 -19.8L-6.2 -19.4L-7.8 -18.8L-9.3 -18.1L-10.8 -17.3L-11.5 -18.2L-13.6 -20.8L-15.1 -22.4L-16.3 -23.1L-17.2 -23L-23 -17.2L-23.1 -16.3L-22.4 -15.1L-20.8 -13.6L-18.2 -11.5L-17.3 -10.8L-18.1 -9.3L-18.8 -7.8L-19.4 -6.2L-19.8 -4.6L-21 -4.7L-24.3 -5L-26.5 -5.1L-27.9 -4.8L-28.4 -4.1L-28.4 4.1L-27.9 4.8L-26.5 5.1L-24.3 5L-21 4.7L-19.8 4.6L-19.4 6.2L-18.8 7.8L-18.1 9.3L-17.3 10.8L-18.2 11.5L-20.8 13.6L-22.4 15.1L-23.1 16.3L-23 17.2L-17.2 23L-16.3 23.1L-15.1 22.4L-13.6 20.8L-11.5 18.2L-10.8 17.3L-9.3 18.1L-7.8 18.8L-6.2 19.4L-4.6 19.8L-4.7 21L-5 24.3L-5.1 26.5L-4.8 27.9L-4.1 28.4L4.1 28.4L4.8 27.9L5.1 26.5L5 24.3L4.7 21L4.6 19.8L6.2 19.4L7.8 18.8L9.3 18.1L10.8 17.3L11.5 18.2L13.6 20.8L15.1 22.4L16.3 23.1L17.2 23L23 17.2L23.1 16.3L22.4 15.1L20.8 13.6L18.2 11.5L17.3 10.8L18.1 9.3L18.8 7.8L19.4 6.2L19.8 4.6L21 4.7L24.3 5L26.5 5.1L27.9 4.8L28.4 4.1L28.4 -4.1L27.9 -4.8L26.5 -5.1L24.3 -5L21 -4.7L19.8 -4.6L19.4 -6.2L18.8 -7.8L18.1 -9.3L17.3 -10.8L18.2 -11.5L20.8 -13.6L22.4 -15.1L23.1 -16.3L23 -17.2L17.2 -23L16.3 -23.1L15.1 -22.4L13.6 -20.8L11.5 -18.2L10.8 -17.3L9.3 -18.1L7.8 -18.8L6.2 -19.4L4.6 -19.8L4.7 -21L5 -24.3L5.1 -26.5L4.8 -27.9L4.1 -28.4L-4.1 -28.4L-4.8 -27.9L-5.1 -26.5L-5 -24.3L-4.7 -21Z";
export const TRIANGLE = "M0 -9.4L8.1 4.7L-8.1 4.7Z";

/** The gear on a 64 grid, centred: the drawing both the icon and the signature use. */
export function GearDrawing() {
  return (
    <g transform="translate(32 32)" fill="none" strokeLinejoin="round" strokeWidth={4.2}>
      <path d={GEAR} stroke="currentColor" />
      <path d={TRIANGLE} stroke="var(--primary)" />
    </g>
  );
}

/**
 * The gear as an icon: the props of a `lucide-react` icon (`size`, `className`,
 * `aria-*`), so it fits any `icon={...}` slot. `strokeWidth` is ignored: the
 * gear keeps its own weight at every size.
 */
export const AcogniGear = React.forwardRef<SVGSVGElement, LucideProps>(
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  ({ size = 24, strokeWidth: _strokeWidth, absoluteStrokeWidth: _absolute, className, color, ...rest }, ref) => (
    <svg
      ref={ref}
      xmlns="http://www.w3.org/2000/svg"
      width={size}
      height={size}
      viewBox="0 0 64 64"
      className={["acogni-brand", className].filter(Boolean).join(" ")}
      color={color}
      data-testid="acogni-gear"
      {...rest}
    >
      <GearDrawing />
    </svg>
  ),
);
AcogniGear.displayName = "AcogniGear";
