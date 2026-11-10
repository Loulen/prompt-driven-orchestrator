import { useEffect, useState } from "react";
import { KeyValue, type KeyValueRow } from "../../../shared/ui/kit";
import ActorLabel from "../../../shared/actors/ActorLabel";
import { fetchProjects } from "../../../workspace/api/projects";
import { folderLabel, kindLabel } from "../../model/hostedSession";
import { useHostedSession } from "../../store/useHostedSession";
import "./hostedSession.css";

/** The name of the Projet `id`, its id until read (or when it is not listed). */
function useProjectName(id: string | null): string | null {
  const [names, setNames] = useState<Record<string, string>>({});
  useEffect(() => {
    if (!id) return undefined;
    let alive = true;
    fetchProjects().then(
      (projects) => alive && setNames(Object.fromEntries(projects.map((p) => [p.id, p.name]))),
      () => undefined,
    );
    return () => {
      alive = false;
    };
  }, [id]);
  return id ? (names[id] ?? id) : null;
}

/**
 * What a Session hébergée is (CORE-125): its kind, the folder it works in, its
 * Projet, who started it and who drives its terminal now (the Pilote, or the
 * one Poste that shows it).
 */
export default function SessionInfo({ session: id }: { session: string }) {
  const { session } = useHostedSession(id);
  const project = useProjectName(session?.project ?? null);
  if (!session) return null;
  const rows: KeyValueRow[] = [
    { k: "Kind", v: `${kindLabel(session.kind)} (hosted session)` },
    { k: "Folder", v: folderLabel(session), mono: true, tip: session.cwd },
  ];
  if (project) rows.push({ k: "Project", v: project, tip: session.project ?? undefined });
  rows.push(
    {
      k: "Started by",
      v: session.started_by ? <ActorLabel actor={session.started_by} /> : "Unknown",
      tip: "Who opened or last relaunched it: it acts on their behalf",
    },
    {
      k: "Driving",
      v: session.driving ? <ActorLabel actor={session.driving} /> : "Nobody",
      tip: "Whose keystrokes reach its terminal",
    },
  );
  return (
    <section className="hs-block" data-testid="hosted-session-info">
      <div className="hs-block__head">
        <span className="hs-block__title">Session</span>
      </div>
      <KeyValue rows={rows} />
    </section>
  );
}
