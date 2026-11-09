/**
 * The Marque of the binary (ADR draft « la Marque est déclarée par le binaire à
 * la compilation »): the name the interface gives the product. The daemon
 * declares it in the page it serves (`<html data-brand>`, written before the
 * bundle loads, so the first render already has it); the Vite dev server serves
 * the page as shipped, PDO's.
 *
 * The one place of the front that names the product: everything else reads
 * `brandName()` (`designSystem.guards.test.ts` holds it).
 */

/** The Marques the core knows; the ids the daemon writes in `data-brand`. */
export type BrandId = "pdo" | "acogni-driver";

export interface Brand {
  id: BrandId;
  /** The window title, the product in a sentence. */
  name: string;
}

/**
 * The name of the core, whatever the Marque: the vanilla binary's release
 * channel, the folder of the built-in skills, the « core PDO 2.76.0 » of a
 * distribution.
 */
export const CORE_NAME = "PDO";

/** The name of each Marque. */
export const BRAND_NAMES: Record<BrandId, string> = {
  pdo: CORE_NAME,
  "acogni-driver": "Acogni Driver",
};

const BRANDS: Record<BrandId, Brand> = {
  pdo: { id: "pdo", name: BRAND_NAMES.pdo },
  "acogni-driver": { id: "acogni-driver", name: BRAND_NAMES["acogni-driver"] },
};

/** The Marque the page was served under; PDO when it names none (or an unknown one). */
export function currentBrand(): Brand {
  const id = typeof document === "undefined" ? undefined : document.documentElement.dataset.brand;
  return (id && Object.hasOwn(BRANDS, id) ? BRANDS[id as BrandId] : null) ?? BRANDS.pdo;
}

/** The product's name, as the interface writes it. */
export function brandName(): string {
  return currentBrand().name;
}
