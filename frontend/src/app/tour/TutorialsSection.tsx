import { useContext, useState, type ReactNode } from "react";
import { Check, Play } from "lucide-react";
import { TOUR_CATALOG, fullTourMinutes, fullTourSequence } from "../../lib/tours";
import { loadToursDone, resetTourMemory } from "../../lib/tourMemory";
import { Button } from "../../shared/ui/kit";
import type { SettingsSection } from "../../settings/model/settingsCategories";
import Section from "../../settings/components/settingsSurface/Section";
import {
  rowVisible,
  SEARCH_HIT,
  SearchContext,
  searching,
  SectionMatchContext,
} from "../../settings/components/settingsSurface/search";

/**
 * Settings › General › **Tutorials** (#823, spec #821 stories 5–6).
 *
 * Presentation only: nothing here belongs to the instance form, so no field of it
 * ever enters the dirty set and Save does not apply to it — the checkmarks and the
 * welcome prompt are this browser's memory, like the theme. Starting a tour closes
 * the Settings surface first, because every tour points at the app underneath it.
 */

interface Props {
  section: SettingsSection;
  /** Close Settings, then run this tour. */
  onStartTour: (tourId: string) => void;
  /** Close Settings, then run the full tour. */
  onStartFullTour: () => void;
}

function shortDate(iso: string): string {
  const at = new Date(iso);
  if (Number.isNaN(at.getTime())) return "";
  return at.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

export default function TutorialsSection({ section, onStartTour, onStartFullTour }: Props) {
  const [done, setDone] = useState(loadToursDone);
  const full = fullTourMinutes();
  const sequence = fullTourSequence();
  const hasStartableTour = sequence.length > 0;
  // The full tour is done once each of its tours is (CORE-232): its mark carries the
  // date the last of them was finished.
  const fullFinishedAt =
    hasStartableTour && sequence.every((tour) => done[tour.id])
      ? sequence.map((tour) => done[tour.id]).sort().at(-1)
      : undefined;

  return (
    <Section
      section={section}
      tools={
        <Button
          size="sm"
          tone="ghost"
          title="Forget the checkmarks and the welcome prompt of this browser"
          data-testid="tutorials-reset"
          onClick={() => {
            resetTourMemory();
            setDone({});
          }}
        >
          Reset
        </Button>
      }
    >
      <TourTable>
        {(search) => (
          <table className="set-table" data-testid="settings-tutorials">
            <tbody>
              {search.finds(FULL_TOUR_TEXT) && (
                <tr
                  className="set-tour--full"
                  data-testid="tutorials-full"
                  data-done={fullFinishedAt ? "true" : "false"}
                  {...hit(search)}
                >
                  <td>
                    <b>Full tour</b>
                  </td>
                  <td className="set-faint set-mono">~{full} min</td>
                  <td>
                    {fullFinishedAt && (
                      <span className="set-ok" title={`Completed ${shortDate(fullFinishedAt)}`}>
                        <Check
                          size={13}
                          strokeWidth={1.5}
                          aria-label="Full tour completed"
                          data-testid="tutorials-done-full"
                        />
                      </span>
                    )}
                  </td>
                  <td className="set-table__act">
                    <Button
                      size="sm"
                      tone="primary"
                      icon={Play}
                      onClick={onStartFullTour}
                      disabled={!hasStartableTour}
                      data-testid="tutorials-start-full"
                    >
                      Start
                    </Button>
                  </td>
                </tr>
              )}
              {TOUR_CATALOG.map((entry) => {
                const finishedAt = done[entry.id];
                if (!search.finds(tourText(entry))) return null;
                return (
                  <tr
                    key={entry.id}
                    data-testid={`tutorials-row-${entry.id}`}
                    data-done={finishedAt ? "true" : "false"}
                    className={entry.def ? undefined : "set-faint"}
                    {...hit(search)}
                  >
                    <td title={entry.def ? `${entry.blurb} ${entry.def.steps.length} steps.` : entry.blurb}>
                      {entry.title}
                    </td>
                    <td className="set-faint set-mono">{entry.def ? `~${entry.minutes} min` : "soon"}</td>
                    <td>
                      {finishedAt && (
                        <span className="set-ok" title={`Completed ${shortDate(finishedAt)}`}>
                          <Check
                            size={13}
                            strokeWidth={1.5}
                            aria-label={`${entry.title} completed`}
                            data-testid={`tutorials-done-${entry.id}`}
                          />
                        </span>
                      )}
                    </td>
                    <td className="set-table__act">
                      <Button
                        size="sm"
                        disabled={entry.def == null}
                        onClick={() => onStartTour(entry.id)}
                        data-testid={`tutorials-start-${entry.id}`}
                      >
                        {finishedAt ? "Replay" : "Start"}
                      </Button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </TourTable>
    </Section>
  );
}

/** What the search reads of the full tour's row, and of a tour's: its name, never its blurb. */
const FULL_TOUR_TEXT = "Full tour tutorial";
const tourText = (entry: { title: string }) => `${entry.title} tour tutorial`;

/** The search, as the tours' rows read it. */
interface TourSearch {
  /** Whether a row of `text` shows; outside a search, every row shows. */
  finds: (text: string) => boolean;
  searching: boolean;
}

/** Under a search, a shown row is one of its hits: counted, and its section shows for it. */
function hit(search: TourSearch) {
  return search.searching ? { [SEARCH_HIT]: "" } : {};
}

/**
 * The tours' table, its rows found by a search like setting rows (CORE-232): « tour »,
 * « Full tour » or « Overview » finds them, and the search line counts them. A search
 * that finds none of them, nor the section, leaves the table out.
 */
function TourTable({ children }: { children: (search: TourSearch) => ReactNode }) {
  const query = useContext(SearchContext);
  const sectionMatch = useContext(SectionMatchContext);
  const search: TourSearch = {
    finds: (text) => rowVisible(query, sectionMatch, text, false),
    searching: searching(query),
  };
  const any = search.finds(FULL_TOUR_TEXT) || TOUR_CATALOG.some((entry) => search.finds(tourText(entry)));
  return any ? <div className="set-extra">{children(search)}</div> : null;
}
