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

   Keyboard: plain Tab + Enter/Space on native buttons. No roving tabindex, no
   arrow keys — deliberate: no choice control in this codebase has them (the
   three `role="tablist"` strips are click-only too). One a11y pass, not here. */

/* Module-local, deliberately NOT exported: a component file that also exports a
   constant breaks the `react-refresh/only-export-components` lint rule, which IS
   gating (`npm run lint` in CI). Same shape as ModelPicker's `ALIASES`; the tests
   spell the list out, exactly as ModelPicker.test.tsx does. */
const EFFORT_LEVELS = ["low", "medium", "high", "xhigh", "max"] as const;

export default function EffortPicker({
  value,
  onChange,
  testid,
}: {
  value: string | null;
  onChange: (v: string | null) => void;
  testid: string; // "node-effort" | "merge-effort"
}) {
  const set = value != null && value !== "";
  const known = set && (EFFORT_LEVELS as readonly string[]).includes(value);
  const options: { id: string | null; label: string; slug: string }[] = [
    { id: null, label: "Default", slug: "default" },
    ...EFFORT_LEVELS.map((l) => ({ id: l as string, label: l, slug: l })),
    // Pass-through segment: only present when the node carries a value the
    // curated set does not know. Clicking it is a no-op re-selection.
    ...(set && !known ? [{ id: value, label: value, slug: "passthrough" }] : []),
  ];

  return (
    <div role="radiogroup" aria-label="Effort" className="flex gap-1">
      {options.map((o) => {
        // `""` is normalised to unset, so an empty-string value selects Default.
        const selected = (set ? value : null) === o.id;
        return (
          <button
            key={o.slug}
            type="button"
            role="radio"
            aria-checked={selected}
            data-testid={`${testid}-option-${o.slug}`}
            onClick={() => onChange(o.id)}
            className={`flex-1 cursor-pointer rounded border px-2 py-1 font-medium transition-colors ${
              selected
                ? o.id == null
                  ? "border-fg-4 bg-bg-3 text-fg"
                  : "border-acc bg-acc-bg text-acc"
                : "border-line-strong bg-bg-3 text-fg-4 hover:text-fg-3"
            }`}
            style={{ fontSize: "10px" }}
          >
            {o.label}
          </button>
        );
      })}
    </div>
  );
}
