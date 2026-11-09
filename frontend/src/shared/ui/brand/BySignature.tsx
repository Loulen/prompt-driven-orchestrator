/**
 * « by Acogni »: the publisher's signature, its gear and its name (CORE-225).
 * Text, never a link; in `--text-3` and `--fs-xs`, its gear keeping the ruby
 * under every Theme. Under the welcome lockup of PDO and after the title of
 * Settings › Version & update.
 */
import { AcogniGear } from "./AcogniMark";

export function BySignature({ className }: { className?: string }) {
  return (
    <span className={["by-acogni", className].filter(Boolean).join(" ")} data-testid="by-acogni">
      <AcogniGear size={12} aria-hidden />
      <span>by Acogni</span>
    </span>
  );
}
