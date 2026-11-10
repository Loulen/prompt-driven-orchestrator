/**
 * **Transient overlays** — the modals that float over the whole app and block it
 * while they are up: the markdown artifact viewer, and anything else built like
 * it (a full-screen backdrop plus a card).
 *
 * They exist for one reader gesture, and they are *in the way* of anything that
 * then needs the app underneath. A guided tour is exactly that: it paints from a
 * layer above every modal (`Projecteur`, `z-[120]`) and points AT the app — so a
 * viewer opened three steps ago dims into the background while its own
 * `fixed inset-0` backdrop goes on swallowing every click the tour asks for.
 *
 * That is not a hypothesis: the Full tour's first handover was frozen by it
 * (#825). *First run* ends on « open `out` »; the artifact modal that opens is
 * what ends the tour, and *First pipeline* then started on an app nobody could
 * click — the Pipelines tab its own first step points at included.
 *
 * Settings closes itself before starting a tour, because it is the surface the
 * click came from. This is the same rule for the surfaces that cannot see it
 * coming: they say how to dismiss themselves, and whoever needs a clear stage
 * asks. Deliberately NOT a tour concept — a modal that registers here knows
 * nothing about tours, which is the standing rule for #823's onboarding work
 * (« aucune logique de tour dans les composants existants »).
 */

type Dismiss = () => void;
/** Where the overlay is on screen, read when asked: `null` once it is gone. */
type Root = () => Element | null;

const registered = new Map<Dismiss, Root | null>();

/**
 * Declare this overlay dismissable while it is mounted. Returns the unregister,
 * so a component's effect can simply `return registerTransientOverlay(onClose)`.
 *
 * `root` says where it is. A picker gives it (the folder explorer, the skills
 * picker): it is then also folded by {@link dismissOverlaysAside} when whoever
 * needs the app next points somewhere outside it. A viewer that gives none (the
 * artifact viewer) is only closed by a clear stage.
 */
export function registerTransientOverlay(dismiss: Dismiss, root: Root | null = null): () => void {
  registered.set(dismiss, root);
  return () => {
    registered.delete(dismiss);
  };
}

/**
 * Close every overlay currently up, and leave the app clear.
 *
 * Iterated over a copy: a dismiss unmounts its own component, whose cleanup
 * mutates the very set we are walking.
 */
export function dismissTransientOverlays(): void {
  for (const dismiss of [...registered.keys()]) dismiss();
}

/**
 * Fold the pickers that hold none of `keep` — what the next gesture is about.
 * A guided tour moving to a step that points at the Pipeline field must not
 * leave the folder explorer over it, its Cancel under the dim (FP of CORE-227):
 * the picker was for the step before. One that holds the new target stays.
 */
export function dismissOverlaysAside(keep: Element[]): void {
  for (const [dismiss, root] of [...registered]) {
    const el = root?.();
    if (!el) continue;
    if (keep.some((k) => el.contains(k))) continue;
    dismiss();
  }
}
