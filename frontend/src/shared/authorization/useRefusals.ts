/**
 * Which of a list of Operations the caller may not make, and why (CORE-205):
 * a surface asks once, with the question of authorization (`POST /authorize`),
 * and greys each refused gesture with the refusal's sentence instead of
 * letting it go and showing the daemon's refusal afterwards.
 *
 * Asked again whenever the Operations change, and when the window comes back
 * (a right changed elsewhere). Before the answer, and when the question
 * fails, nothing is refused: the daemon still decides at the gesture.
 */

import { useEffect, useMemo, useState } from "react";
import { askAuthorization, refusalOf, type OperationQuestion } from "./api";

/** How many Operations one question may hold (the daemon's cap). */
const MAX_OPERATIONS = 500;

/**
 * The sentence of each refusal among `operations`, in order: `undefined`
 * where the Operation is allowed (or not answered yet). `active: false`
 * asks nothing (a surface not on screen).
 */
export function useRefusals(operations: readonly OperationQuestion[], active = true): (string | undefined)[] {
  const key = useMemo(() => JSON.stringify(operations), [operations]);
  const [answer, setAnswer] = useState<{ key: string; refusals: (string | undefined)[] } | null>(null);
  const [asked, setAsked] = useState(0);

  useEffect(() => {
    if (!active) return;
    const again = () => {
      if (document.visibilityState === "visible") setAsked((n) => n + 1);
    };
    window.addEventListener("focus", again);
    document.addEventListener("visibilitychange", again);
    return () => {
      window.removeEventListener("focus", again);
      document.removeEventListener("visibilitychange", again);
    };
  }, [active]);

  useEffect(() => {
    const questions = JSON.parse(key) as OperationQuestion[];
    if (!active || questions.length === 0) return;
    let current = true;
    const parts: OperationQuestion[][] = [];
    for (let i = 0; i < questions.length; i += MAX_OPERATIONS) parts.push(questions.slice(i, i + MAX_OPERATIONS));
    Promise.all(parts.map((part) => askAuthorization(part)))
      .then((decisions) => {
        if (current) setAnswer({ key, refusals: decisions.flat().map(refusalOf) });
      })
      .catch(() => {
        // keep the last answer: the daemon decides at the gesture
      });
    return () => {
      current = false;
    };
  }, [key, active, asked]);

  return useMemo(
    () => (answer?.key === key ? answer.refusals : operations.map(() => undefined)),
    [answer, key, operations],
  );
}

/** The sentence of the refusal of `operation`, `undefined` when allowed (or not answered yet). */
export function useRefusal(operation: OperationQuestion, active = true): string | undefined {
  const key = JSON.stringify(operation);
  // One stable list per Operation: the question is asked once.
  const operations = useMemo(() => [JSON.parse(key) as OperationQuestion], [key]);
  return useRefusals(operations, active)[0];
}

/**
 * The refusal of each item of a list, by its key, for the Operations
 * `perItem` asks about it (in order), after the `global` ones: one question
 * for the whole list. `of(key)[i]` is the refusal of the i-th Operation of
 * that item, nothing for a key not in the list; `global[i]` that of the i-th
 * global Operation. `keyOf`, `perItem` and `global` must be stable (module
 * constants): the question is asked again when they or `items` change.
 */
export function useListRefusals<T>(
  items: readonly T[],
  keyOf: (item: T) => string,
  perItem: (item: T) => OperationQuestion[],
  global: readonly OperationQuestion[] = NO_OPERATION,
  active = true,
): { global: (string | undefined)[]; of: (key: string) => (string | undefined)[] } {
  const questions = useMemo(() => [...global, ...items.flatMap(perItem)], [items, perItem, global]);
  const refusals = useRefusals(questions, active);
  return useMemo(() => {
    const byKey = new Map<string, (string | undefined)[]>();
    let at = global.length;
    for (const item of items) {
      const n = perItem(item).length;
      byKey.set(keyOf(item), refusals.slice(at, at + n));
      at += n;
    }
    return { global: refusals.slice(0, global.length), of: (key: string) => byKey.get(key) ?? [] };
  }, [refusals, items, perItem, global, keyOf]);
}

const NO_OPERATION: readonly OperationQuestion[] = [];
