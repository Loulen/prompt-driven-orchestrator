import { TriangleAlert } from "lucide-react";

/* Effort (#424): per-node reasoning-effort override. Like `model` (#296/#324),
   `null` ⇒ unset ⇒ never serialized ⇒ account default.

   A segmented radiogroup, NOT a range input, for two reasons:
   (1) `fill` on an `<input type=range>` assigns `element.value`, React's value
       tracker swallows the mutation and `onChange` never fires — the control is
       undrivable by the agentic browser tests (and there is no `type=range`
       anywhere else in this frontend);
   (2) the domain is not totally ordered — Opus 4.6 / Sonnet 4.6 support `max`
       but NOT `xhigh`, so a slider would assert a rank that is false for those
       models.

   An unrecognised non-empty value gets its own trailing segment rather than
   rendering as "nothing selected": the wire is free-text pass-through, so a
   hand-authored `effort: turbo` must stay visible and un-clobbered
   (ADR-0001/#268 — no silent loss). No free-text input: a 5-stop scale does not
   need ModelPicker's `Custom…` mode.

   #798: with `strict` (the levels are the AUTHORITATIVE support for the
   selected model — `effortOffer(...).authoritative`), an unrecognised value is
   UNSUPPORTED, not merely unknown: it stays visible in its pass-through segment
   (hand-authored config is never silently deleted), the segment is flagged
   `data-unsupported` and is not a supported option, and a warning names the
   way out — pick a supported level or Default. The click on the passthrough
   segment stays a no-op re-selection, exactly like before. With `strict` false
   (capabilities unknown — no served catalogue), the historical pass-through UI
   stands untouched.

   Keyboard: plain Tab + Enter/Space on native buttons. No roving tabindex, no
   arrow keys — deliberate: no choice control in this codebase has them (the
   three `role="tablist"` strips are click-only too). One a11y pass, not here. */

export default function EffortPicker({
  value,
  onChange,
  efforts,
  testid,
  disabled = false,
  strict = false,
}: {
  value: string | null;
  onChange: (v: string | null) => void;
  /* #616/ADR-0053: the effort levels the resolved harness's binary offers, SERVED
     by the daemon — the segments are rendered from THIS list, no hard-coded scale.
     A harness may enumerate more stops than claude (copilot's seven, AC #4) or
     fewer; the picker renders whatever the binary offers. Empty ⇒ no offer, and the
     picker is typically also `disabled` (no effort axis). */
  efforts: string[];
  testid: string; // "node-effort" | "merge-effort"
  /* #550/ADR-0046, #616: greyed when the resolved harness has no effort axis — a
     SERVED fact now (`has_effort`), not a client map. An absence declared by the
     binary, so the control is disabled, not hidden. Assert on the `disabled`
     attribute, never `.value` (a `.value` assertion cannot fail — a known trap). */
  disabled?: boolean;
  /* #798: the `efforts` list is the AUTHORITATIVE support for the model this
     surface selected/resolved (`effortOffer(...).authoritative`). A stored
     value outside it is then unsupported: warned, kept, never deleted, never
     offered as a supported option. Omitted (capabilities unknown — no served
     catalogue, hand-built option): the historical free-text pass-through, no
     warning. */
  strict?: boolean;
}) {
  const set = value != null && value !== "";
  const known = set && efforts.includes(value);
  const unsupported = strict && set && !known;
  const options: { id: string | null; label: string; slug: string }[] = [
    { id: null, label: "Default", slug: "default" },
    ...efforts.map((l) => ({ id: l, label: l, slug: l })),
    // Pass-through segment: only present when the node carries a value the served
    // set does not know. Clicking it is a no-op re-selection. Under `strict` it
    // renders as the preserved UNSUPPORTED value — flagged, warned, never a
    // supported option, never silently deleted (ADR-0001).
    ...(set && !known ? [{ id: value, label: value, slug: "passthrough" }] : []),
  ];

  return (
    <div
      role="radiogroup"
      aria-label="Effort"
      aria-disabled={disabled}
      className={`flex flex-wrap gap-1 ${disabled ? "opacity-50" : ""}`}
    >
      {options.map((o) => {
        // `""` is normalised to unset, so an empty-string value selects Default.
        const selected = (set ? value : null) === o.id;
        return (
          <button
            key={o.slug}
            type="button"
            role="radio"
            aria-checked={selected}
            disabled={disabled || (unsupported && o.slug === "passthrough")}
            data-testid={`${testid}-option-${o.slug}`}
            onClick={() => {
              if (!disabled) onChange(o.id);
            }}
            className={`flex-1 rounded border px-2 py-1 font-medium transition-colors ${
              disabled ? "cursor-not-allowed" : "cursor-pointer"
            } ${
              selected
                ? o.id == null
                  ? "border-fg-4 bg-bg-3 text-fg"
                  : unsupported
                    ? "border-st-blocked bg-st-blocked/10 text-st-blocked"
                    : "border-acc bg-acc-bg text-acc"
                : "border-line-strong bg-bg-3 text-fg-4 hover:text-fg-3"
            }`}
            style={{ fontSize: "10px" }}
            {...(unsupported && o.slug === "passthrough" ? { "data-unsupported": "true" } : {})}
          >
            {o.label}
          </button>
        );
      })}
      {unsupported && (
        <p
          data-testid={`${testid}-unsupported`}
          role="status"
          className="mt-1 flex basis-full items-start gap-1 text-st-blocked"
          style={{ fontSize: "9.5px", lineHeight: 1.4 }}
        >
          <TriangleAlert size={10} className="mt-[1px] shrink-0" />
          <span>
            "{value}" is not supported by the selected model. Pick a supported
            level, or Default to unset.
          </span>
        </p>
      )}
    </div>
  );
}
