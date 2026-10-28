/**
 * The app icons with one ruby part, after the product marks of the brand: an
 * ink line icon whose accent stroke takes `--primary`. Stats, the latest bar;
 * Settings, the mark's triangle in the gear. Same props as a `lucide-react`
 * icon, so they fit any `icon={...}` slot.
 */
import * as React from "react";
import type { LucideIcon, LucideProps } from "lucide-react";

function accentIcon(name: string, line: React.ReactNode, accent: React.ReactNode): LucideIcon {
  const Icon = React.forwardRef<SVGSVGElement, LucideProps>(
    ({ size = 24, strokeWidth = 2, className, color, ...rest }, ref) => (
      <svg
        ref={ref}
        xmlns="http://www.w3.org/2000/svg"
        width={size}
        height={size}
        viewBox="0 0 24 24"
        fill="none"
        stroke={color ?? "currentColor"}
        strokeWidth={strokeWidth}
        strokeLinecap="round"
        strokeLinejoin="round"
        className={["acc-icon", className].filter(Boolean).join(" ")}
        {...rest}
      >
        {line}
        <g className="acc-icon__acc" stroke="var(--primary)" strokeWidth={Number(strokeWidth) + 0.5}>
          {accent}
        </g>
      </svg>
    ),
  );
  Icon.displayName = name;
  return Icon as unknown as LucideIcon;
}

export const StatsIcon = accentIcon(
  "StatsIcon",
  <>
    <path d="M3 3v16a2 2 0 0 0 2 2h16" />
    <path d="M13 17V5" />
    <path d="M8 17v-3" />
  </>,
  <path d="M18 17V9" />,
);

export const SettingsIcon = accentIcon(
  "SettingsIcon",
  <path d="M9.671 4.136a2.34 2.34 0 0 1 4.659 0 2.34 2.34 0 0 0 3.319 1.915 2.34 2.34 0 0 1 2.33 4.033 2.34 2.34 0 0 0 0 3.831 2.34 2.34 0 0 1-2.33 4.033 2.34 2.34 0 0 0-3.319 1.915 2.34 2.34 0 0 1-4.659 0 2.34 2.34 0 0 0-3.32-1.915 2.34 2.34 0 0 1-2.33-4.033 2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051a2.34 2.34 0 0 0 3.319-1.915" />,
  <path d="M12 9.5 14.6 14H9.4Z" />,
);

export const ReviewIcon = accentIcon(
  "ReviewIcon",
  <>
    <circle cx="18" cy="18" r="3" />
    <circle cx="6" cy="6" r="3" />
  </>,
  <>
    <path d="M13 6h3a2 2 0 0 1 2 2v7" />
    <path d="M11 18H8a2 2 0 0 1-2-2V9" />
  </>,
);
