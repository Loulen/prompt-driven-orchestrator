/**
 * Why the greyed entries of a menu are greyed (CORE-205): said once each, at
 * its foot, or in a tooltip of each entry.
 */
import { Lock } from "lucide-react";

/**
 * The foot of a menu: why its greyed entries are greyed, a line per reason.
 * Inside an element of role `menu`, give it `id` and point the menu's
 * `aria-describedby` at it: it is then `presentation`, read as the menu's
 * description.
 */
export function MenuReasons({ reasons, id }: { reasons: readonly string[]; id?: string }) {
  if (reasons.length === 0) return null;
  const lines = reasons.map((reason) => (
    <div key={reason} className="d-pop__why" role={id ? undefined : "note"} data-testid="menu-disabled-reason">
      <Lock size={12} strokeWidth={1.5} aria-hidden />
      <span>{reason}</span>
    </div>
  ));
  return id ? (
    <div id={id} role="presentation">
      {lines}
    </div>
  ) : (
    <>{lines}</>
  );
}

/**
 * Where a menu says why its greyed entries are greyed: once each, at its
 * foot (the default), or in a tooltip of each entry, shown on hover or on
 * keyboard focus (the arrows then stop on it).
 */
export type MenuReasonsAt = "foot" | "tooltip";
