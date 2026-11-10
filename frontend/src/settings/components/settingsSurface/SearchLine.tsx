import { forwardRef } from "react";
import { CircleDot, Loader2, Search } from "lucide-react";
import { Button, EmptyState, IconButton } from "../../../shared/ui/kit";

/** The search line above the page: a few words, or only the changed settings. */
export const SearchLine = forwardRef<
  HTMLInputElement,
  {
    needle: string;
    onNeedle: (needle: string) => void;
    /** How many rows show; `null` outside a search. */
    count: number | null;
    modifiedOnly: boolean;
    onModifiedOnly: (on: boolean) => void;
  }
>(function SearchLine({ needle, onNeedle, count, modifiedOnly, onModifiedOnly }, ref) {
  return (
    <div className="set-search">
      <label className="set-search__field">
        <Search size={14} strokeWidth={1.5} aria-hidden />
        <input
          ref={ref}
          className="set-search__input"
          placeholder="Search settings"
          aria-label="Search settings"
          data-testid="settings-search"
          value={needle}
          onChange={(e) => onNeedle(e.target.value)}
        />
        {count !== null && (
          <span className="set-search__count" data-testid="settings-search-count">
            {count}
          </span>
        )}
      </label>
      <IconButton
        icon={CircleDot}
        label="Modified only"
        pressed={modifiedOnly}
        onClick={() => onModifiedOnly(!modifiedOnly)}
        data-testid="settings-modified-only"
      />
    </div>
  );
});

/** A search that found nothing, and the way back. */
export function NoSettings({ onClear }: { onClear: () => void }) {
  return (
    <EmptyState
      icon={Search}
      action={
        <Button size="sm" onClick={onClear}>
          Clear
        </Button>
      }
    >
      No settings
    </EmptyState>
  );
}

/** A category whose content waits for `GET /settings`. */
export function SettingsLoading() {
  return (
    <div className="set-extra" data-testid="settings-loading">
      <EmptyState icon={Loader2} spin layout="row">
        Loading…
      </EmptyState>
    </div>
  );
}
