import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { request } from "./client";
import {
  followSignIn,
  isRedirectingToSignIn,
  probeSignIn,
  resetSignInForTests,
  signInRedirect,
  signInUrl,
} from "./signIn";

const REFUSED = { error: "unauthenticated", message: "authentication required: sign in first", redirect: "/ext/test/sign-in" };

let assign: ReturnType<typeof vi.fn>;

beforeEach(() => {
  resetSignInForTests();
  assign = vi.fn();
  vi.stubGlobal("location", { ...window.location, pathname: "/", search: "", hash: "#/runs/r1", assign });
});

afterEach(() => {
  vi.unstubAllGlobals();
});

function respond(status: number, body: unknown) {
  vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify(body), { status })));
}

describe("the sign-in redirection", () => {
  it("reads the page a refusal names, and nothing else", () => {
    expect(signInRedirect(REFUSED)).toBe("/ext/test/sign-in");
    expect(signInRedirect({ error: "unauthenticated", reason: "token" })).toBeNull();
    expect(signInRedirect({ error: "operation_refused", redirect: "/x" })).toBeNull();
    expect(signInRedirect(null)).toBeNull();
  });

  it("appends where to come back, deep link included", () => {
    expect(signInUrl("/ext/test/sign-in", "/#/runs/r1")).toBe("/ext/test/sign-in?next=%2F%23%2Fruns%2Fr1");
    expect(signInUrl("/sign-in?x=1#top", "/")).toBe("/sign-in?x=1&next=%2F#top");
  });

  it("goes there once", () => {
    followSignIn("/ext/test/sign-in");
    followSignIn("/ext/test/sign-in");
    expect(assign).toHaveBeenCalledTimes(1);
    expect(assign).toHaveBeenCalledWith("/ext/test/sign-in?next=%2F%23%2Fruns%2Fr1");
    expect(isRedirectingToSignIn()).toBe(true);
  });

  it("is followed by every call of the client, which still fails", async () => {
    respond(401, REFUSED);
    await expect(request("GET", "/runs")).rejects.toMatchObject({ status: 401 });
    expect(assign).toHaveBeenCalledWith("/ext/test/sign-in?next=%2F%23%2Fruns%2Fr1");
  });

  it("is not followed for a bare refusal, nor for another error", async () => {
    respond(401, { error: "unauthenticated", reason: "a token is required" });
    await expect(request("GET", "/runs")).rejects.toMatchObject({ status: 401 });
    respond(403, { ...REFUSED, error: "operation_refused" });
    await expect(request("GET", "/runs")).rejects.toMatchObject({ status: 403 });
    expect(assign).not.toHaveBeenCalled();
  });

  it("a socket's probe follows it, and an admitted UI stays", async () => {
    respond(200, { actor: { kind: "local_operator" }, display: null });
    await probeSignIn();
    expect(assign).not.toHaveBeenCalled();
    respond(401, REFUSED);
    await probeSignIn();
    expect(assign).toHaveBeenCalledTimes(1);
  });
});
