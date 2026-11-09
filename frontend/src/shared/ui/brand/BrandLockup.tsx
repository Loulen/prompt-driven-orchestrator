/**
 * The lockup of the Marque the page was served under (CORE-225), for the welcome
 * modal: PDO's mark and its name in Geist Mono; Acogni Driver's wordmark, rule
 * and tag (`AcogniLockup`). Styles in `app/tour/tour.css`.
 */
import { AcogniLockup } from "./AcogniLockup";
import { MarkDrawing } from "./BrandMark";
import { MARK_VIEWBOX } from "./marks";
import { currentBrand } from "./brand";

export function BrandLockup() {
  const brand = currentBrand();
  if (brand.id === "acogni-driver") return <AcogniLockup />;
  return (
    <p
      className="tr-lockup tr-lockup--pdo"
      role="img"
      aria-label={brand.name}
      data-testid="brand-lockup"
      data-brand={brand.id}
    >
      <svg className="tr-lockup__mark acogni-brand" viewBox={MARK_VIEWBOX.pdo} aria-hidden="true">
        <MarkDrawing brand="pdo" />
      </svg>
      <span className="tr-lockup__word" aria-hidden="true">
        {brand.name}
      </span>
    </p>
  );
}
