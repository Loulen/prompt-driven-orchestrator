import { afterEach, describe, expect, it } from "vitest";
import { localStorageTransport, TAB_BUS_STORAGE_KEY } from "./tabCoordinatorBrowser";

afterEach(() => localStorage.clear());

describe("localStorage fallback transport", () => {
  it("writes each message to one key and reads the others' off the storage event", () => {
    const transport = localStorageTransport()!;
    const received: unknown[] = [];
    const off = transport.subscribe((m) => received.push(m));
    transport.post({ type: "bye", tabId: "x" });
    expect(JSON.parse(localStorage.getItem(TAB_BUS_STORAGE_KEY)!).message).toEqual({ type: "bye", tabId: "x" });
    // Not to itself (the browser never fires `storage` in the writing tab)…
    expect(received).toEqual([]);
    // …but another tab's write arrives.
    window.dispatchEvent(
      new StorageEvent("storage", {
        key: TAB_BUS_STORAGE_KEY,
        newValue: JSON.stringify({ message: { type: "state", tabId: "y", visible: true }, nonce: 1 }),
      }),
    );
    window.dispatchEvent(new StorageEvent("storage", { key: "other", newValue: "{}" }));
    expect(received).toEqual([{ type: "state", tabId: "y", visible: true }]);
    off();
  });
});
