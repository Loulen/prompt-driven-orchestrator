import { useEffect, type RefObject } from "react";

/**
 * The focus of a modal card (#437): focus the card on mount, give focus back to
 * the opener on unmount, and keep Tab / Shift+Tab inside the card.
 *
 * Mounting IS opening: the modal that calls it is only mounted while open.
 */
export function useDialogFocus(dialogRef: RefObject<HTMLDivElement | null>) {
  // Focus initial + restitution. Montage == ouverture (cf. l'effet de fetch
  // ci-dessus). On capture l'opener (la loupe déclencheuse) AVANT de bouger le focus,
  // on pose le focus sur la carte pour que le lecteur d'écran annonce le dialogue par
  // son nom accessible, et on rend le focus à l'opener au démontage. `[]` volontaire :
  // mêmes sémantiques "ouvrir une fois" que l'effet de fetch. StrictMode double-invoque
  // de façon idempotente (capture -> restore -> capture lit le même opener).
  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null;
    dialogRef.current?.focus();
    return () => {
      if (opener?.isConnected) opener.focus?.();
    };
  }, [dialogRef]);

  // Piège de focus (#437) : garde Tab / Shift+Tab dans la carte. Effet distinct de
  // l'Escape pour ne pas perturber son stopPropagation / ordre d'écouteurs. document +
  // phase bubble, comme l'Escape de ce fichier. Interception AUX BORDS seulement : on ne
  // redirige qu'au premier/dernier focusable, le Tab au milieu reste natif (pas de calcul
  // sur les ~1000 lignes, le scroll-suit-focus marche tout seul). Re-query à chaque Tab
  // pour suivre le churn asynchrone des lignes (navigateTo).
  useEffect(() => {
    function handleTab(e: KeyboardEvent) {
      if (e.key !== "Tab") return;
      const card = dialogRef.current;
      if (!card) return;
      const items = card.querySelectorAll<HTMLElement>(
        'a[href], button:not([disabled]), input:not([disabled]), ' +
          'select:not([disabled]), textarea:not([disabled]), ' +
          '[tabindex]:not([tabindex="-1"])',
      );
      if (items.length === 0) return;
      const first = items[0];
      const last = items[items.length - 1];
      const active = document.activeElement;
      const inside = card.contains(active);
      if (e.shiftKey) {
        if (active === first || !inside) {
          e.preventDefault();
          last.focus();
        }
      } else {
        if (active === last || !inside) {
          e.preventDefault();
          first.focus();
        }
      }
    }
    document.addEventListener("keydown", handleTab);
    return () => document.removeEventListener("keydown", handleTab);
  }, [dialogRef]);
}
