import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { backoffDelay, BACKOFF_MAX_MS, openProbedSocket, PROBE_PATH, type ProbedSocket } from "./probedSocket";
import { resetSignInForTests } from "./signIn";

/** A socket the test drives: every instance is recorded, its events fired on demand. */
class FakeSocket {
  static OPEN = 1;
  static instances: FakeSocket[] = [];
  url: string;
  readyState = 0;
  binaryType = "blob";
  sent: unknown[] = [];
  closedWith: number | undefined;
  closed = false;
  listeners = new Map<string, ((event: object) => void)[]>();
  constructor(url: string) {
    this.url = url;
    FakeSocket.instances.push(this);
  }
  addEventListener(type: string, listener: (event: object) => void) {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener]);
  }
  send(data: unknown) {
    this.sent.push(data);
  }
  /** Like a browser: the "close" event comes later, not inside `close()`. */
  close(code?: number) {
    this.closed = true;
    this.closedWith = code;
  }
  fire(type: string, event: object = {}) {
    if (type === "open") this.readyState = 1;
    if (type === "close") this.readyState = 3;
    for (const listener of this.listeners.get(type) ?? []) listener(event);
  }
}

let visibility: DocumentVisibilityState = "visible";
let socket: ProbedSocket | null = null;

function setVisibility(next: DocumentVisibilityState) {
  visibility = next;
  document.dispatchEvent(new Event("visibilitychange"));
}

/** The daemon answers the probe with `status` (`null`: unreachable). */
function daemon(status: number | null, body: unknown = {}) {
  const fetchMock = vi.fn(async () => {
    if (status === null) throw new TypeError("NetworkError when attempting to fetch resource.");
    return new Response(JSON.stringify(body), { status });
  });
  vi.stubGlobal("fetch", fetchMock);
  return fetchMock;
}

const open = (path = "/ws", reconnect?: boolean | "after-drop", silenceMs?: number) => {
  socket = openProbedSocket(path, {
    ...(reconnect === undefined ? {} : { reconnect }),
    ...(silenceMs === undefined ? {} : { silenceMs }),
  });
  return socket;
};

beforeEach(() => {
  vi.useFakeTimers();
  FakeSocket.instances = [];
  vi.stubGlobal("WebSocket", FakeSocket);
  visibility = "visible";
  Object.defineProperty(document, "visibilityState", { configurable: true, get: () => visibility });
  resetSignInForTests();
});

afterEach(() => {
  socket?.close();
  socket = null;
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

describe("the backoff of the probe", () => {
  it("grows from about a second up to about thirty, with a random half taken off", () => {
    expect(backoffDelay(0, () => 1)).toBe(1_000);
    expect(backoffDelay(0, () => 0)).toBe(500);
    expect(backoffDelay(1, () => 1)).toBe(2_000);
    expect(backoffDelay(3, () => 0.5)).toBe(6_000);
    expect(backoffDelay(20, () => 1)).toBe(BACKOFF_MAX_MS);
    expect(backoffDelay(20, () => 0)).toBe(BACKOFF_MAX_MS / 2);
  });
});

describe("openProbedSocket", () => {
  it("opens no WebSocket while the daemon does not answer, and probes again ever later", async () => {
    vi.spyOn(Math, "random").mockReturnValue(1);
    const fetchMock = daemon(null);
    const s = open();
    await vi.advanceTimersByTimeAsync(0);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(fetchMock).toHaveBeenCalledWith(PROBE_PATH, expect.anything());
    expect(s.state).toBe("hors_ligne");

    // 1 s, then 2 s, then 4 s: the probe repeats, never the socket.
    await vi.advanceTimersByTimeAsync(999);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(fetchMock).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(2_000);
    expect(fetchMock).toHaveBeenCalledTimes(3);
    await vi.advanceTimersByTimeAsync(3_999);
    expect(fetchMock).toHaveBeenCalledTimes(3);
    await vi.advanceTimersByTimeAsync(1);
    expect(fetchMock).toHaveBeenCalledTimes(4);
    expect(FakeSocket.instances).toHaveLength(0);
  });

  it("spaces the probes of two tabs apart (jitter)", async () => {
    const random = vi.spyOn(Math, "random").mockReturnValue(0);
    const fetchMock = daemon(null);
    open();
    await vi.advanceTimersByTimeAsync(0);
    random.mockReturnValue(0.5);
    // With nothing taken off, the second probe would come at 1 s; with half of
    // the ceiling taken off at random, it comes at 500 ms.
    await vi.advanceTimersByTimeAsync(500);
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it("counts a daemon behind a proxy answering 5xx as unreachable", async () => {
    daemon(502);
    const s = open();
    await vi.advanceTimersByTimeAsync(0);
    expect(FakeSocket.instances).toHaveLength(0);
    expect(s.state).toBe("hors_ligne");
  });

  it("opens the socket once the probe succeeded, and relays what it receives", async () => {
    daemon(200);
    const s = open("/ext/test/stream");
    const states: string[] = [];
    const received: unknown[] = [];
    s.onState((state) => states.push(state));
    s.onMessage((data) => received.push(data));
    expect(s.state).toBe("connexion");
    await vi.advanceTimersByTimeAsync(0);
    expect(FakeSocket.instances).toHaveLength(1);
    const ws = FakeSocket.instances[0];
    expect(ws.url).toBe(`ws://${window.location.host}/ext/test/stream`);
    expect(ws.binaryType).toBe("arraybuffer");

    expect(s.send("too early")).toBe(false);
    ws.fire("open");
    expect(s.state).toBe("ouvert");
    expect(s.send("hello")).toBe(true);
    const bytes = new Uint8Array([1, 2]).buffer;
    ws.fire("message", { data: "text" });
    ws.fire("message", { data: bytes });
    expect(ws.sent).toEqual(["hello"]);
    expect(received).toEqual(["text", bytes]);
    expect(states).toEqual(["ouvert"]);
  });

  it("probes again after the socket dropped, and reopens once the daemon answers", async () => {
    vi.spyOn(Math, "random").mockReturnValue(1);
    const fetchMock = daemon(200);
    const s = open();
    await vi.advanceTimersByTimeAsync(0);
    FakeSocket.instances[0].fire("open");

    daemon(null);
    FakeSocket.instances[0].fire("close", { code: 1006 });
    expect(s.state).toBe("hors_ligne");
    await vi.advanceTimersByTimeAsync(1_000 + 2_000);
    expect(FakeSocket.instances).toHaveLength(1);

    const back = daemon(200);
    await vi.advanceTimersByTimeAsync(4_000);
    expect(back).toHaveBeenCalledTimes(1);
    expect(FakeSocket.instances).toHaveLength(2);
    FakeSocket.instances[1].fire("open");
    expect(s.state).toBe("ouvert");
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("does not probe while the tab is hidden, and probes at once when it shows", async () => {
    visibility = "hidden";
    const fetchMock = daemon(200);
    open();
    await vi.advanceTimersByTimeAsync(60_000);
    expect(fetchMock).not.toHaveBeenCalled();
    expect(FakeSocket.instances).toHaveLength(0);

    setVisibility("visible");
    await vi.advanceTimersByTimeAsync(0);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(FakeSocket.instances).toHaveLength(1);
  });

  it("stops probing a stopped daemon while hidden, and resumes at once when shown", async () => {
    vi.spyOn(Math, "random").mockReturnValue(1);
    const fetchMock = daemon(null);
    open();
    await vi.advanceTimersByTimeAsync(0);
    setVisibility("hidden");
    await vi.advanceTimersByTimeAsync(120_000);
    // The probe due 1 s later found the tab hidden: none since.
    expect(fetchMock).toHaveBeenCalledTimes(1);

    daemon(200);
    setVisibility("visible");
    await vi.advanceTimersByTimeAsync(0);
    expect(FakeSocket.instances).toHaveLength(1);
  });

  it("keeps an open socket open when the tab hides", async () => {
    daemon(200);
    const s = open();
    await vi.advanceTimersByTimeAsync(0);
    FakeSocket.instances[0].fire("open");
    setVisibility("hidden");
    await vi.advanceTimersByTimeAsync(60_000);
    expect(FakeSocket.instances[0].closedWith).toBeUndefined();
    expect(s.state).toBe("ouvert");
  });

  it("goes to the sign-in page on a refused session, without any socket", async () => {
    const assign = vi.fn();
    vi.stubGlobal("location", { ...window.location, host: window.location.host, assign });
    const fetchMock = daemon(401, { error: "unauthenticated", redirect: "/ext/test/sign-in" });
    const s = open();
    await vi.advanceTimersByTimeAsync(60_000);
    expect(assign).toHaveBeenCalledTimes(1);
    expect(assign.mock.calls[0][0]).toMatch(/^\/ext\/test\/sign-in\?next=/);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(FakeSocket.instances).toHaveLength(0);
    expect(s.state).toBe("hors_ligne");
  });

  it("closes with 1000 and does nothing more", async () => {
    vi.spyOn(Math, "random").mockReturnValue(1);
    const fetchMock = daemon(200);
    const s = open();
    await vi.advanceTimersByTimeAsync(0);
    const ws = FakeSocket.instances[0];
    ws.fire("open");
    s.close();
    expect(ws.closedWith).toBe(1000);
    ws.fire("close", { code: 1000 });
    await vi.advanceTimersByTimeAsync(60_000);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(FakeSocket.instances).toHaveLength(1);
  });

  it("closed while probing, it opens nothing", async () => {
    daemon(200);
    const s = open();
    s.close();
    await vi.advanceTimersByTimeAsync(0);
    expect(FakeSocket.instances).toHaveLength(0);
  });

  it("without reconnection, probes before its one opening and stays down once it ends", async () => {
    vi.spyOn(Math, "random").mockReturnValue(1);
    daemon(null);
    const s = open("/sessions/pdo-x/pty", false);
    await vi.advanceTimersByTimeAsync(0);
    expect(FakeSocket.instances).toHaveLength(0);
    const fetchMock = daemon(200);
    await vi.advanceTimersByTimeAsync(1_000);
    expect(FakeSocket.instances).toHaveLength(1);
    FakeSocket.instances[0].fire("open");
    FakeSocket.instances[0].fire("close", { code: 1000 });
    expect(s.state).toBe("hors_ligne");
    await vi.advanceTimersByTimeAsync(60_000);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(FakeSocket.instances).toHaveLength(1);
  });

  it("without reconnection, a socket refused at its upgrade asks whether to sign in", async () => {
    const assign = vi.fn();
    vi.stubGlobal("location", { ...window.location, host: window.location.host, assign });
    daemon(200);
    open("/sessions/pdo-x/pty", false);
    await vi.advanceTimersByTimeAsync(0);
    daemon(401, { error: "unauthenticated", redirect: "/ext/test/sign-in" });
    FakeSocket.instances[0].fire("close", { code: 1006 });
    await vi.advanceTimersByTimeAsync(0);
    expect(assign).toHaveBeenCalledTimes(1);
    expect(FakeSocket.instances).toHaveLength(1);
  });

  it("reconnecting after a drop, reopens once the daemon is back when the socket dropped without a Close", async () => {
    vi.spyOn(Math, "random").mockReturnValue(1);
    daemon(200);
    const s = open("/sessions/pdo-x/pty", "after-drop");
    await vi.advanceTimersByTimeAsync(0);
    FakeSocket.instances[0].fire("open");

    // The daemon restarts: the socket drops, the probe repeats meanwhile.
    daemon(null);
    FakeSocket.instances[0].fire("close", { code: 1006 });
    expect(s.state).toBe("hors_ligne");
    await vi.advanceTimersByTimeAsync(1_000);
    expect(FakeSocket.instances).toHaveLength(1);

    daemon(200);
    await vi.advanceTimersByTimeAsync(2_000);
    expect(FakeSocket.instances).toHaveLength(2);
    FakeSocket.instances[1].fire("open");
    expect(s.state).toBe("ouvert");
  });

  it("reconnecting after a drop, stays down once the daemon closed it", async () => {
    daemon(200);
    const s = open("/sessions/pdo-x/pty", "after-drop");
    await vi.advanceTimersByTimeAsync(0);
    FakeSocket.instances[0].fire("open");
    // The session ended: the daemon sends its Close (no status code).
    const fetchMock = daemon(200);
    FakeSocket.instances[0].fire("close", { code: 1005 });
    expect(s.state).toBe("hors_ligne");
    await vi.advanceTimersByTimeAsync(60_000);
    expect(fetchMock).not.toHaveBeenCalled();
    expect(FakeSocket.instances).toHaveLength(1);
  });

  it("reconnecting after a drop, stays down when refused at its upgrade, and asks whether to sign in", async () => {
    daemon(200);
    const s = open("/sessions/pdo-x/pty", "after-drop");
    await vi.advanceTimersByTimeAsync(0);
    const fetchMock = daemon(200);
    FakeSocket.instances[0].fire("close", { code: 1006 });
    await vi.advanceTimersByTimeAsync(60_000);
    expect(s.state).toBe("hors_ligne");
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(FakeSocket.instances).toHaveLength(1);
  });

  it("reconnecting after a drop, reopens after a daemon going away (1001), and stays down when the reopening is refused", async () => {
    vi.spyOn(Math, "random").mockReturnValue(1);
    daemon(200);
    const s = open("/sessions/pdo-x/pty", "after-drop");
    await vi.advanceTimersByTimeAsync(0);
    FakeSocket.instances[0].fire("open");
    FakeSocket.instances[0].fire("close", { code: 1001 });
    await vi.advanceTimersByTimeAsync(1_000);
    expect(FakeSocket.instances).toHaveLength(2);

    // The session ended meanwhile: the reopening is refused at its upgrade.
    const fetchMock = daemon(200);
    FakeSocket.instances[1].fire("close", { code: 1006 });
    await vi.advanceTimersByTimeAsync(60_000);
    expect(s.state).toBe("hors_ligne");
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(FakeSocket.instances).toHaveLength(2);
  });
});

describe("the delay of silence", () => {
  const SILENCE = 30_000;

  it("closes a socket that heard nothing for the delay, and probes again before reopening", async () => {
    vi.spyOn(Math, "random").mockReturnValue(1);
    const fetchMock = daemon(200);
    const s = open("/ws", true, SILENCE);
    await vi.advanceTimersByTimeAsync(0);
    const ws = FakeSocket.instances[0];
    ws.fire("open");

    await vi.advanceTimersByTimeAsync(SILENCE - 1);
    expect(ws.closed).toBe(false);
    expect(s.state).toBe("ouvert");
    await vi.advanceTimersByTimeAsync(1);
    expect(ws.closed).toBe(true);
    expect(s.state).toBe("hors_ligne");
    expect(fetchMock).toHaveBeenCalledTimes(1);

    // Its late `close` changes nothing; the probe comes after the backoff.
    ws.fire("close", { code: 1006 });
    await vi.advanceTimersByTimeAsync(1_000);
    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(FakeSocket.instances).toHaveLength(2);
    FakeSocket.instances[1].fire("open");
    expect(s.state).toBe("ouvert");
  });

  it("starts again on every message, the daemon's beat included", async () => {
    daemon(200);
    const s = open("/ws", true, SILENCE);
    await vi.advanceTimersByTimeAsync(0);
    const ws = FakeSocket.instances[0];
    ws.fire("open");
    for (let i = 0; i < 5; i++) {
      await vi.advanceTimersByTimeAsync(SILENCE - 1_000);
      ws.fire("message", { data: '{"type":"heartbeat"}' });
    }
    expect(s.state).toBe("ouvert");
    expect(FakeSocket.instances).toHaveLength(1);
  });

  it("counts as a drop for a terminal that reopens only after a drop", async () => {
    vi.spyOn(Math, "random").mockReturnValue(1);
    daemon(200);
    const s = open("/sessions/pdo-x/pty", "after-drop", SILENCE);
    await vi.advanceTimersByTimeAsync(0);
    FakeSocket.instances[0].fire("open");
    await vi.advanceTimersByTimeAsync(SILENCE);
    expect(s.state).toBe("hors_ligne");
    // A drop, not the daemon's Close: the terminal comes back on its own.
    FakeSocket.instances[0].fire("close", { code: 1000 });
    await vi.advanceTimersByTimeAsync(1_000);
    expect(FakeSocket.instances).toHaveLength(2);
    FakeSocket.instances[1].fire("open");
    expect(s.state).toBe("ouvert");
  });

  it("without reconnection, stays down once silent", async () => {
    const fetchMock = daemon(200);
    const s = open("/sessions/pdo-x/pty", false, SILENCE);
    await vi.advanceTimersByTimeAsync(0);
    FakeSocket.instances[0].fire("open");
    await vi.advanceTimersByTimeAsync(SILENCE);
    expect(s.state).toBe("hors_ligne");
    await vi.advanceTimersByTimeAsync(120_000);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(FakeSocket.instances).toHaveLength(1);
  });

  it("drops the socket at once when the browser goes offline, and probes at once when it is back", async () => {
    vi.spyOn(Math, "random").mockReturnValue(1);
    daemon(200);
    const s = open("/ws", true, SILENCE);
    await vi.advanceTimersByTimeAsync(0);
    FakeSocket.instances[0].fire("open");

    const offline = daemon(null);
    window.dispatchEvent(new Event("offline"));
    expect(s.state).toBe("hors_ligne");
    // Probes fail while offline, ever later (1 s, 2 s, 4 s, 8 s…).
    await vi.advanceTimersByTimeAsync(10_000);
    const probesOffline = offline.mock.calls.length;
    expect(probesOffline).toBeGreaterThan(0);
    expect(FakeSocket.instances).toHaveLength(1);

    const back = daemon(200);
    window.dispatchEvent(new Event("online"));
    await vi.advanceTimersByTimeAsync(0);
    expect(back).toHaveBeenCalledTimes(1);
    expect(FakeSocket.instances).toHaveLength(2);
  });

  it("judges the silence again when a hidden tab shows, its timers being throttled", async () => {
    vi.spyOn(Math, "random").mockReturnValue(1);
    daemon(200);
    const s = open("/ws", true, SILENCE);
    await vi.advanceTimersByTimeAsync(0);
    FakeSocket.instances[0].fire("open");
    setVisibility("hidden");
    // The browser held the timer back: the clock moved, the timer did not fire.
    vi.setSystemTime(Date.now() + SILENCE + 5_000);
    setVisibility("visible");
    expect(s.state).toBe("hors_ligne");
    await vi.advanceTimersByTimeAsync(0);
    expect(FakeSocket.instances).toHaveLength(2);
  });

  it("without the option, a silent socket stays open, offline or not", async () => {
    daemon(200);
    const s = open("/ws");
    await vi.advanceTimersByTimeAsync(0);
    const ws = FakeSocket.instances[0];
    ws.fire("open");
    await vi.advanceTimersByTimeAsync(10 * 60_000);
    window.dispatchEvent(new Event("offline"));
    expect(s.state).toBe("ouvert");
    expect(ws.closed).toBe(false);
    expect(FakeSocket.instances).toHaveLength(1);
  });
});
