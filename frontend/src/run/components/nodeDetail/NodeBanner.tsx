import type { ReactNode } from "react";
import { ToneIcon } from "../../../shared/ui/kit/tone";
import { toneColor, type StatusTone } from "../../../shared/ui/kit/tones";

/**
 * One banner of the node panel: a tinted line with the status icon, a short
 * fact, the detail truncated (whole in the tooltip) and its actions at the
 * right; `children` — a list of violations, of missing outputs — under it.
 */
export default function NodeBanner({
  tone,
  title,
  text,
  quote = false,
  aside,
  actions,
  testId,
  data,
  tip,
  children,
}: {
  tone: StatusTone;
  title: string;
  text?: ReactNode;
  /** The detail is the agent's own words: drawn in the main ink. */
  quote?: boolean;
  /** A small mono figure before the actions (how long it has waited). */
  aside?: ReactNode;
  actions?: ReactNode;
  testId?: string;
  /** `data-*` attributes, the assertion surface of a banner. */
  data?: Record<`data-${string}`, string | number | boolean | undefined>;
  /** The tooltip, when it says more than the fact and the detail. */
  tip?: string;
  children?: ReactNode;
}) {
  const tooltip = tip ?? (typeof text === "string" ? `${title} · ${text}` : title);
  return (
    <div
      className="ri-banner"
      style={{ ["--st" as string]: toneColor(tone) }}
      role={tone === "failed" ? "alert" : "status"}
      data-testid={testId}
      {...data}
    >
      <div className="d-banner" title={tooltip}>
        <ToneIcon tone={tone} size={14} />
        <strong className="d-banner__title" data-testid={testId ? `${testId}-title` : undefined}>
          {title}
        </strong>
        {text != null && text !== "" && <span className="sr-only"> — </span>}
        {text != null && text !== "" && (
          <span className={`d-banner__text${quote ? " ri-banner__quote" : ""}`} data-testid={testId ? `${testId}-message` : undefined}>
            {text}
          </span>
        )}
        {aside && <span className="ri-banner__since">{aside}</span>}
        {actions && <span className="d-banner__actions">{actions}</span>}
      </div>
      {children}
    </div>
  );
}
