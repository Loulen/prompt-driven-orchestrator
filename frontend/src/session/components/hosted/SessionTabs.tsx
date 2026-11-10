import { useMemo, useState, type ReactNode } from "react";
import { Tabs } from "../../../shared/ui/kit";
import { useContributions, useLocationContext } from "../../../shared/extensions/catalogue";
import type { TabContext, TabProps } from "../../../shared/extensions/contract";
import ExtensionDetailTab from "../../../shared/extensions/ExtensionDetailTab";
import { useHostedSession } from "../../store/useHostedSession";
import SessionEvents from "./SessionEvents";
import "./hostedSession.css";

/** `session/detail` (0.19): the Session hébergée its tab details, as `entity`
 *  (`kind: "session"`, its `id`, its key as `name`) and as `session`. */
export interface SessionTabContext extends TabContext {
  session: { id: string; extension: string; key: string; kind: string; state: string };
}

const DETAILS = "details";
const EVENTS = "events";

/**
 * The tabs of a Session hébergée's detail (CORE-125): Details (what the host
 * composes there, `details`), Events (its journal), then the
 * tabs the extensions contribute to `session/detail`, each handed the session.
 */
export default function SessionTabs({ session: id, details }: { session: string; details?: ReactNode }) {
  const { session, events } = useHostedSession(id);
  const extra = useMemo(
    () => ({
      entity: { kind: "session", id, name: session?.key ?? null, attributes: {} },
      session: {
        id,
        extension: session?.extension ?? "",
        key: session?.key ?? "",
        kind: session?.kind ?? "",
        state: session?.state ?? "",
      },
    }),
    [id, session?.extension, session?.key, session?.kind, session?.state],
  );
  const context = useLocationContext<SessionTabContext>("session/detail", extra);
  const contributed = useContributions<TabProps>("session/detail", context);
  const tabs = session ? contributed : [];
  const [chosen, setChosen] = useState<string>(DETAILS);
  const extensionTab = tabs.find((t) => t.key === chosen) ?? null;
  const value = extensionTab || chosen === EVENTS ? chosen : DETAILS;
  const items = [
    { value: DETAILS, label: "Details" },
    { value: EVENTS, label: "Events", count: events.length },
    ...tabs.map((t) => ({ value: t.key, label: t.contribution.label })),
  ];
  return (
    <div className="hs-tabs" data-testid="hosted-session-tabs">
      <div className="hs-tabs__bar">
        <Tabs items={items} value={value} onChange={setChosen} label="Session details" />
      </div>
      <div className="hs-tabs__body" role="tabpanel">
        {extensionTab ? (
          <ExtensionDetailTab key={`${extensionTab.key}:${id}`} tab={extensionTab} context={context} />
        ) : value === EVENTS ? (
          <SessionEvents session={id} />
        ) : (
          details
        )}
      </div>
    </div>
  );
}
