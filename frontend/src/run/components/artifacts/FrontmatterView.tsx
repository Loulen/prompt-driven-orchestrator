import { useLayoutEffect, useRef, useState } from "react";
import { ArrowUpRight, ChevronDown, ChevronUp, TriangleAlert } from "lucide-react";
import { IconButton, Tooltip } from "../../../shared/ui/kit";
import type { SchemaWarning } from "../../../shared/generated/SchemaWarning";
import { frontmatterValue, isWebUrl, isWideValue, verdictTone } from "../../model/frontmatter";
import { useCopyToClipboard } from "../../store/useCopyToClipboard";

export type FrontmatterLayout = "band" | "rows";

/**
 * An artifact's frontmatter, the same wherever the Run shows it — the band of
 * the artifact viewer (`band`), an output card or a live artifact row of the
 * I/O tab (`rows`):
 *
 * - a click on a value copies it (the one clipboard path, « ✓ Copied » or
 *   « Copy failed » after it);
 * - an http(s) value also opens in a new tab (↗);
 * - a verdict-like value keeps its tone;
 * - in `rows`, a web address or a long value takes a line of its own, short
 *   values share one;
 * - when a value is cut, a chevron unfolds the fields as a key/value list,
 *   each value whole, and folds them back. Local: never persisted.
 *
 * `warnings` (a live artifact's schema check) paints a field off schema in
 * amber with its rule on hover, and names a declared field that is absent.
 */
export default function FrontmatterView({
  values,
  layout,
  warnings = [],
  testId = "frontmatter",
  fieldTestId = "frontmatter-field",
  warningTestId = "frontmatter-field-warning",
}: {
  /** The fields, in the order to show them (a map keeps its own). */
  values: Record<string, unknown> | ReadonlyArray<readonly [string, unknown]>;
  layout: FrontmatterLayout;
  warnings?: SchemaWarning[];
  testId?: string;
  fieldTestId?: string;
  warningTestId?: string;
}) {
  const entries = Array.isArray(values) ? values : Object.entries(values);
  const byField = new Map(warnings.map((w) => [w.field, w.reason]));
  const absent = warnings.filter((w) => !entries.some(([field]) => field === w.field));
  const [expanded, setExpanded] = useState(false);
  const [listRef, cut] = useCutValues(expanded, entries.map(([k, v]) => `${k}=${frontmatterValue(v)}`).join("\n"));

  if (entries.length === 0 && absent.length === 0) return null;
  return (
    <div className={`fmv fmv--${layout}`} data-testid={testId} data-expanded={expanded || undefined}>
      <dl className="fmv__list" ref={listRef}>
        {entries.map(([field, value]) => {
          const reason = byField.get(field);
          const text = frontmatterValue(value);
          return (
            <div
              key={field}
              className="fmv__item"
              data-testid={reason ? warningTestId : fieldTestId}
              data-wide={(layout === "rows" && isWideValue(text)) || undefined}
            >
              <dt className="fmv__k" data-st={reason ? "awaiting" : undefined}>
                {reason && <TriangleAlert size={10} strokeWidth={1.75} aria-label="Schema not respected" />}
                {field}
              </dt>
              <dd className="fmv__v">
                <FieldValue field={field} text={text} tone={reason ? "awaiting" : verdictTone(value)} warning={reason} />
              </dd>
            </div>
          );
        })}
        {absent.map((w) => (
          <div key={`absent:${w.field}`} className="fmv__item" data-testid={warningTestId}>
            <dt className="fmv__k" data-st="awaiting" title={`Schema: ${w.reason}`}>
              <TriangleAlert size={10} strokeWidth={1.75} aria-label="Schema not respected" />
              {w.field === "(frontmatter)" ? "frontmatter" : w.field}
            </dt>
            <dd className="fmv__v fmv__missing">{w.field === "(frontmatter)" ? w.reason : "missing"}</dd>
          </div>
        ))}
      </dl>
      {(cut || expanded) && (
        <IconButton
          icon={expanded ? ChevronUp : ChevronDown}
          size="sm"
          label={expanded ? "Fold the frontmatter" : "Unfold the frontmatter"}
          className="fmv__fold"
          data-testid="frontmatter-toggle"
          aria-expanded={expanded}
          onClick={() => setExpanded((v) => !v)}
        />
      )}
    </div>
  );
}

/** One value: a button that copies it, its ↗ when it is a web address, the copy's feedback. */
function FieldValue({
  field,
  text,
  tone,
  warning,
}: {
  field: string;
  text: string;
  tone?: string;
  warning?: string;
}) {
  const { status, error, copy } = useCopyToClipboard();
  const label = status === "failed" ? (error ?? "Copy failed") : warning ? `Schema: ${warning}` : "Click to copy";
  return (
    <>
      <Tooltip label={label}>
        <button
          type="button"
          className="fmv__val"
          data-st={tone}
          data-testid="frontmatter-value"
          data-field={field}
          onClick={() => void copy(text)}
        >
          {text}
        </button>
      </Tooltip>
      {isWebUrl(text) && (
        <a
          href={text.trim()}
          target="_blank"
          rel="noopener noreferrer"
          className="d-icon d-icon--sm fmv__open"
          aria-label={`Open ${field}`}
          data-testid="frontmatter-open"
        >
          <ArrowUpRight size={12} strokeWidth={1.5} aria-hidden />
        </a>
      )}
      {status === "copied" && (
        <span className="fmv__fb" data-st="done" role="status" data-testid="frontmatter-copied">
          ✓ Copied
        </span>
      )}
      {status === "failed" && (
        <span className="fmv__fb" data-st="failed" role="alert" data-testid="frontmatter-copy-failed">
          Copy failed
        </span>
      )}
    </>
  );
}

/** Whether a value of the folded list is cut by the width it has — measured
 *  when the values change and at each resize of the list. */
function useCutValues(expanded: boolean, contents: string) {
  const ref = useRef<HTMLDListElement>(null);
  const [any, setAny] = useState(false);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el || expanded) return;
    const measure = () => {
      const values = el.querySelectorAll<HTMLElement>(".fmv__val");
      setAny([...values].some((v) => v.scrollWidth > v.clientWidth + 1));
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [expanded, contents]);
  return [ref, any] as const;
}
