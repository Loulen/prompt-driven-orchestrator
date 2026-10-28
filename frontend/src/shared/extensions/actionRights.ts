/**
 * The Actions refused to the Actor are absent (API 0.18): an action whose
 * command declares the Operation it makes (`UiCommand.operation`) shows only
 * once the question of authorization (`POST /authorize`) allows it, aimed at
 * the entity of its location's context when it hands one over. An action
 * whose command declares none always shows. A question that fails shows the
 * action: the daemon, or the extension's route, still refuses it.
 */

import { useEffect, useMemo, useRef, useState } from "react";
import { askAuthorization, isAllowed, type OperationQuestion } from "../authorization/api";
import type { ContributionElement } from "./catalogue";
import type { LocationContext } from "./contract";

/** An action, and the context its command would be called with. */
export interface ActionAt {
  element: ContributionElement;
  context: LocationContext;
}

/** How many Operations one question may hold (the daemon's cap). */
const MAX_OPERATIONS = 500;

/** The Operation `action` makes, `null` when its command declares none. */
export function actionQuestion({ element, context }: ActionAt): OperationQuestion | null {
  const operation = element.command?.operation;
  if (!operation) return null;
  const entity = (context as { entity?: { id?: unknown } | null }).entity;
  const target = typeof entity?.id === "string" && entity.id ? entity.id : null;
  return { resource: operation.resource, verb: operation.verb, target };
}

const questionKey = (question: OperationQuestion) => JSON.stringify(question);

/**
 * `actions` without those refused; a guarded action is left out until its
 * answer arrives.
 */
export function useAllowedActions<A extends ActionAt>(actions: readonly A[]): A[] {
  // The questions, by their key: the same questions asked of new contexts
  // (a Run refreshed on screen) ask nothing again.
  const keys = useMemo(() => {
    const byKey = new Map<string, OperationQuestion>();
    for (const action of actions) {
      const question = actionQuestion(action);
      if (question) byKey.set(questionKey(question), question);
    }
    return [...byKey.keys()].sort().join("\n");
  }, [actions]);
  const [answers, setAnswers] = useState<ReadonlyMap<string, boolean>>(() => new Map());
  const asking = useRef(new Set<string>());

  useEffect(() => {
    const missing = (keys ? keys.split("\n") : [])
      .filter((key) => !answers.has(key) && !asking.current.has(key))
      .map((key): [string, OperationQuestion] => [key, JSON.parse(key) as OperationQuestion]);
    if (missing.length === 0) return;
    for (const [key] of missing) asking.current.add(key);
    const parts: [string, OperationQuestion][][] = [];
    for (let i = 0; i < missing.length; i += MAX_OPERATIONS) parts.push(missing.slice(i, i + MAX_OPERATIONS));
    void Promise.all(
      parts.map((part) =>
        askAuthorization(part.map(([, question]) => question)).then(
          (decisions) => part.map(([key], i): [string, boolean] => [key, isAllowed(decisions[i])]),
          // Unanswered: shown, the daemon still decides.
          () => part.map(([key]): [string, boolean] => [key, true]),
        ),
      ),
    ).then((answered) => {
      for (const [key] of missing) asking.current.delete(key);
      setAnswers((before) => new Map([...before, ...answered.flat()]));
    });
  }, [keys, answers]);

  return useMemo(
    () =>
      actions.filter((action) => {
        const question = actionQuestion(action);
        return !question || answers.get(questionKey(question)) === true;
      }),
    [actions, answers],
  );
}
