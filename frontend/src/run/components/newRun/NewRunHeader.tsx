import { CalendarClock, Play } from "lucide-react";
import type { NewRunForm } from "../../store/newRun/useNewRunForm";

/**
 * The [Run | Trigger] switch of the title bar (#160), the kit's segmented
 * look; absent while a Trigger is edited (it stays a Trigger).
 */
export function NewRunModeSwitch({ form }: { form: NewRunForm }) {
  const { mode, setMode, editingTriggerId, setGuardTest, setGuardTestError } = form;
  if (editingTriggerId) return null;
  const choose = (next: "run" | "trigger") => {
    setMode(next);
    // Drop any stale guard verdict when switching (#350).
    setGuardTest(null);
    setGuardTestError(null);
  };
  const items = [
    { value: "run" as const, label: "Run", Icon: Play },
    { value: "trigger" as const, label: "Trigger", Icon: CalendarClock },
  ];
  return (
    <div
      className="d-seg d-seg--sm"
      role="tablist"
      aria-label="Mode"
      onKeyDown={(e) => {
        if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
          e.preventDefault();
          const next = mode === "run" ? "trigger" : "run";
          choose(next);
          e.currentTarget.querySelector<HTMLElement>(`[data-testid="mode-${next}"]`)?.focus();
        }
      }}
    >
      {items.map(({ value, label, Icon }) => (
        <button
          key={value}
          type="button"
          role="tab"
          aria-selected={mode === value}
          tabIndex={mode === value ? 0 : -1}
          className="d-seg__btn"
          onClick={() => choose(value)}
          data-testid={`mode-${value}`}
        >
          <Icon size={12} strokeWidth={1.5} aria-hidden />
          <span>{label}</span>
        </button>
      ))}
    </div>
  );
}
