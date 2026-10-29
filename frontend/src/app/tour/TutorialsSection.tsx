import { useState } from "react";
import { Check, Play } from "lucide-react";
import { TOUR_CATALOG, fullTourMinutes, fullTourSequence } from "../../lib/tours";
import { loadToursDone, resetTourMemory } from "../../lib/tourMemory";
import { Button } from "../../shared/ui/kit";
import type { SettingsSection } from "../../settings/model/settingsCategories";
import Section, { SectionExtra } from "../../settings/components/settingsSurface/Section";

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
  const hasStartableTour = fullTourSequence().length > 0;

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
      <SectionExtra>
        <table className="set-table" data-testid="settings-tutorials">
          <tbody>
            <tr className="set-tour--full">
              <td>
                <b>Full tour</b>
              </td>
              <td className="set-faint set-mono">~{full} min</td>
              <td />
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
            {TOUR_CATALOG.map((entry) => {
              const finishedAt = done[entry.id];
              return (
                <tr
                  key={entry.id}
                  data-testid={`tutorials-row-${entry.id}`}
                  data-done={finishedAt ? "true" : "false"}
                  className={entry.def ? undefined : "set-faint"}
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
      </SectionExtra>
    </Section>
  );
}
