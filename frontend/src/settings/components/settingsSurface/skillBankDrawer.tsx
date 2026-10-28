import type { ShellDrawer } from "../../../shared/ui/FullWindowShell";
import SkillBankPanel from "../../../skill/components/SkillBankPanel";
import { announceSkillsChanged } from "../../../skill/store/useSkillBank";
import type { SkillBank } from "../../../skill/model/skill";

/**
 * The skill bank in the shell's secondary panel (#690 decision 5, framed by the shell
 * since #944): the rail stays visible, Escape returns to Settings › Skills first, and the
 * Save footer hides under it so its own writes and Save are never both in view.
 */
export function skillBankDrawer({
  open,
  onClose,
  bank,
  loaded,
  home,
  refresh,
}: {
  open: boolean;
  onClose: () => void;
  bank: SkillBank;
  loaded: boolean;
  home: string | null;
  refresh: () => Promise<unknown>;
}): ShellDrawer | null {
  if (!open) return null;
  return {
    title: "Skill bank",
    escHint: "saves as you go · Esc returns to Settings",
    widthClassName: "w-[min(880px,calc(90vw/var(--app-zoom,1)))]",
    testId: "settings-drawer",
    dataAttributes: { "data-drawer": "skills" },
    onClose,
    content: (
      <SkillBankPanel
        bank={bank}
        loaded={loaded}
        home={home}
        onChanged={async () => {
          await refresh();
          announceSkillsChanged();
        }}
      />
    ),
  };
}
