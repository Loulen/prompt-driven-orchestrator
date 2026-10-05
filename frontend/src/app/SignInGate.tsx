import { useEffect, useState, type ReactNode } from "react";
import { loadCurrentActor } from "../shared/actors/store";
import { ApiError } from "../shared/http/client";
import { isRedirectingToSignIn } from "../shared/http/signIn";

type GateState = "checking" | "admitted" | "redirecting";

/** How long the gate waits for the daemon's answer before rendering the app
 *  anyway: a hung daemon shows the app's own disconnected state, never a blank
 *  page. A refusal arriving later still redirects (every call follows it). */
export const SIGN_IN_GATE_WAIT_MS = 5000;

/**
 * The sign-in gate in front of the app (ADR-0081, 0.8): it asks who the UI is
 * (`GET /actors/me`) before rendering anything. Refused with the sign-in page
 * of an extension, the UI goes there (with `next=` the deep link it was opened
 * on) and renders nothing of the app. Any other answer, a daemon down or
 * silent ({@link SIGN_IN_GATE_WAIT_MS}) included, lets the app render as it
 * always did: its own status says why.
 */
export default function SignInGate({ children }: { children: ReactNode }) {
  const [state, setState] = useState<GateState>("checking");
  useEffect(() => {
    let alive = true;
    const patience = setTimeout(
      () => setState((current) => (current === "checking" ? "admitted" : current)),
      SIGN_IN_GATE_WAIT_MS,
    );
    loadCurrentActor()
      .then(() => alive && setState("admitted"))
      .catch((error: unknown) => {
        if (!alive) return;
        const refused = error instanceof ApiError && error.status === 401;
        setState(refused && isRedirectingToSignIn() ? "redirecting" : "admitted");
      });
    return () => {
      alive = false;
      clearTimeout(patience);
    };
  }, []);
  if (state === "admitted") return <>{children}</>;
  return (
    <div
      className="grid h-screen w-screen place-items-center bg-bg-1 text-fg-3"
      style={{ fontSize: "12.5px" }}
      data-testid="sign-in-gate"
    >
      {state === "redirecting" ? "Redirecting to sign in…" : null}
    </div>
  );
}
