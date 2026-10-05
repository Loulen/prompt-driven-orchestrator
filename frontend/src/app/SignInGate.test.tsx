import { act, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { resetActorsForTests, useActorsStore } from "../shared/actors/store";
import { resetSignInForTests } from "../shared/http/signIn";
import SignInGate, { SIGN_IN_GATE_WAIT_MS } from "./SignInGate";

let assign: ReturnType<typeof vi.fn>;

beforeEach(() => {
  resetSignInForTests();
  resetActorsForTests();
  assign = vi.fn();
  vi.stubGlobal("location", { ...window.location, pathname: "/", search: "", hash: "#/runs/r1", assign });
});

afterEach(() => {
  vi.unstubAllGlobals();
});

function respond(status: number, body: unknown) {
  const fetchMock = vi.fn<(url: string) => Promise<Response>>(
    async () => new Response(JSON.stringify(body), { status }),
  );
  vi.stubGlobal("fetch", fetchMock);
  return fetchMock;
}

describe("SignInGate", () => {
  it("renders the app once the daemon says who the UI is", async () => {
    const alice = {
      actor: { kind: "identified", namespace: "test", id: "alice" },
      display: { name: "Alice", avatar_url: null },
    };
    const fetchMock = respond(200, alice);
    render(
      <SignInGate>
        <p>the app</p>
      </SignInGate>,
    );
    expect(screen.queryByText("the app")).toBeNull();
    expect(await screen.findByText("the app")).toBeTruthy();
    expect(fetchMock.mock.calls[0][0]).toBe("/actors/me");
    expect(useActorsStore.getState().current).toEqual(alice);
    expect(assign).not.toHaveBeenCalled();
  });

  it("goes to the extension's sign-in page, back to the deep link after", async () => {
    respond(401, { error: "unauthenticated", message: "authentication required", redirect: "/ext/test/sign-in" });
    render(
      <SignInGate>
        <p>the app</p>
      </SignInGate>,
    );
    expect(await screen.findByText("Redirecting to sign in…")).toBeTruthy();
    expect(assign).toHaveBeenCalledWith("/ext/test/sign-in?next=%2F%23%2Fruns%2Fr1");
    expect(screen.queryByText("the app")).toBeNull();
  });

  it("lets the app render when the daemon cannot say (down, or no sign-in page)", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => Promise.reject(new TypeError("Failed to fetch"))));
    const { unmount } = render(
      <SignInGate>
        <p>the app</p>
      </SignInGate>,
    );
    expect(await screen.findByText("the app")).toBeTruthy();
    unmount();
    respond(401, { error: "unauthenticated", reason: "a token is required" });
    render(
      <SignInGate>
        <p>the app</p>
      </SignInGate>,
    );
    await waitFor(() => expect(screen.getByText("the app")).toBeTruthy());
    expect(assign).not.toHaveBeenCalled();
  });

  it("renders the app when the daemon does not answer in time", async () => {
    vi.useFakeTimers();
    try {
      vi.stubGlobal("fetch", vi.fn(() => new Promise<Response>(() => {})));
      render(
        <SignInGate>
          <p>the app</p>
        </SignInGate>,
      );
      expect(screen.queryByText("the app")).toBeNull();
      await act(async () => {
        vi.advanceTimersByTime(SIGN_IN_GATE_WAIT_MS);
      });
      expect(screen.getByText("the app")).toBeTruthy();
    } finally {
      vi.useRealTimers();
    }
  });
});
