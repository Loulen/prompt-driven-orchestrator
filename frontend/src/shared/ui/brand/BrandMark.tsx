/**
 * The marks of the Marques (CORE-225): the logo where the name does not fit,
 * drawn in the current colour with its accent in `--primary`, which keeps the
 * Acogni ruby under every Theme (`acogni-brand`, `brand.css`). The same drawings
 * the daemon serves under `/shared/brand/` (`crates/pdo/assets/brand/`).
 *
 * - PDO, « Étapes »: three steps of a Run, done (ink), running (ruby), to come
 *   (outlined), joined by two edges.
 * - Acogni Driver: the A of Acogni, its hollow in ruby.
 */
import type { BrandId } from "./brand";
import { currentBrand } from "./brand";
import { ACOGNI_A, ACOGNI_A_ACCENT, MARK_VIEWBOX } from "./marks";

/** PDO's « Étapes », on a 64 grid. */
function PdoSteps() {
  return (
    <>
      <g fill="currentColor" stroke="currentColor">
        <rect x="3" y="20" width="15" height="24" rx="2" stroke="none" />
        <path d="M18 32H25" fill="none" strokeWidth="4.5" strokeLinejoin="miter" strokeLinecap="butt" />
        <path d="M39 32H46" fill="none" strokeWidth="4.5" strokeLinejoin="miter" strokeLinecap="butt" />
        <rect x="48.25" y="22.25" width="12.5" height="19.5" rx="1.5" fill="none" strokeWidth="4.5" />
      </g>
      <rect x="25" y="20" width="14" height="24" rx="2" fill="var(--primary)" />
    </>
  );
}

/** The drawing of the mark of `brand`, without its `<svg>`. */
export function MarkDrawing({ brand }: { brand: BrandId }) {
  return brand === "pdo" ? (
    <PdoSteps />
  ) : (
    <>
      <path d={ACOGNI_A} fill="currentColor" fillRule="evenodd" />
      <path d={ACOGNI_A_ACCENT} fill="var(--primary)" />
    </>
  );
}

/**
 * The mark of the Marque the page was served under. Decorative where it stands
 * (the top bar): an image named after the product, never a control.
 */
export function BrandMark({
  size = 16,
  className,
  decorative = false,
}: {
  size?: number;
  className?: string;
  /** Next to the product's name already written: hidden from assistive technologies. */
  decorative?: boolean;
}) {
  const brand = currentBrand();
  return (
    <svg
      className={["acogni-brand", className].filter(Boolean).join(" ")}
      width={size}
      height={size}
      viewBox={MARK_VIEWBOX[brand.id]}
      {...(decorative ? { "aria-hidden": true } : { role: "img", "aria-label": brand.name })}
      data-testid="brand-mark"
      data-brand={brand.id}
    >
      <MarkDrawing brand={brand.id} />
    </svg>
  );
}
