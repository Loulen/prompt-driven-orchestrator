import { Repeat } from "lucide-react";

/**
 * An iteration as the Run inspector names it: the canvas card's iteration
 * icon and the number (« ⇄ 2 », « ⇄ 3/3 » with `of`), never « i2 ».
 */
export default function IterMark({
  iter,
  of,
  title = "Iteration",
}: {
  iter: number;
  of?: number;
  /** `null` inside a control that already names the iteration. */
  title?: string | null;
}) {
  return (
    <span className="ri-itermark" title={title ?? undefined} data-testid="iter-mark">
      <Repeat size={11} strokeWidth={1.5} aria-hidden />
      <span>
        <span className="d-sr">Iteration </span>
        {iter}
        {of != null && <span className="ri-itermark__of">/{of}</span>}
      </span>
    </span>
  );
}
