import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as Y from "yjs";
import type { DraftSocketHandlers } from "../api/draftSocket";
import { SYNC_REPLY, SYNC_REQUEST, UPDATE } from "../api/draftSocket";
import type { DraftParticipant } from "../../shared/generated/DraftParticipant";
import { applyObject, applyPrompts, type Json } from "../model/draftDelta";
import { toDraftPipeline } from "../model/draftForm";
import type { EditorPipelineDef } from "../model/pipeline";
import { draftSessionOf, startDraftSessions } from "./draftSessions";
import { useEditStore } from "./editStore";
import { savePipeline } from "../api/pipelines";
import type { OpenPipeline } from "./editState";

/** The sockets the sessions opened, by pipeline id. */
const sockets = new Map<string, { handlers: DraftSocketHandlers; sent: { tag: number; payload: Uint8Array }[]; texts: unknown[] }>();

vi.mock("../api/pipelines", () => ({ savePipeline: vi.fn(), fetchPipeline: vi.fn(), fetchPipelines: vi.fn() }));

vi.mock("../api/draftSocket", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../api/draftSocket")>();
  return {
    ...actual,
    openDraftSocket: (pipelineId: string, handlers: DraftSocketHandlers) => {
      const socket = { handlers, sent: [] as { tag: number; payload: Uint8Array }[], texts: [] as unknown[] };
      sockets.set(pipelineId, socket);
      return {
        sendBinary: (tag: number, payload: Uint8Array) => {
          socket.sent.push({ tag, payload: payload.slice() });
          return true;
        },
        send: (message: unknown) => {
          socket.texts.push(message);
          return true;
        },
        close: () => sockets.delete(pipelineId),
      };
    },
  };
});

const pipeline: EditorPipelineDef = {
  name: "flow",
  version: "1.0",
  variables: {},
  nodes: [
    { id: "a", name: "A", type: "agent", inputs: [], outputs: [], view: { x: 0, y: 0 } } as never,
    { id: "b", name: "B", type: "agent", inputs: [], outputs: [], view: { x: 0, y: 100 } } as never,
  ],
  edges: [],
};

/** The daemon's side: the draft, as the server holds it. */
function serverDoc(): Y.Doc {
  const doc = new Y.Doc();
  doc.transact(() => {
    applyObject(doc.getMap("pipeline"), {}, toDraftPipeline(pipeline));
    applyPrompts(doc.getMap("prompts"), {}, { a: "Read." });
  });
  return doc;
}

function participant(connection: string, color: number): DraftParticipant {
  return {
    connection,
    actor: { kind: "local_operator" },
    display: null,
    color,
    can_write: true,
    selection: null,
    typing: null,
  };
}

function tab(): OpenPipeline {
  return { id: "flow", scope: "instance", pipeline, prompts: {}, diagnostics: [], dirty: false, externalDirty: false };
}

function join(server: Y.Doc, canWrite = true) {
  const socket = sockets.get("flow")!;
  socket.handlers.onState("ouvert");
  socket.handlers.onOpen();
  socket.handlers.onMessage({ type: "welcome", connection: "1", generation: "g1", can_write: canWrite, color: 0 });
  socket.handlers.onMessage({ type: "participants", participants: [participant("1", 0), participant("2", 1)] });
  socket.handlers.onBinary(SYNC_REPLY, Y.encodeStateAsUpdate(server));
  socket.sent.length = 0;
  return socket;
}

function sessionOf() {
  return draftSessionOf("flow")!;
}

function current(): OpenPipeline {
  return useEditStore.getState().openTabs[0];
}

let stop: () => void;

beforeEach(() => {
  useEditStore.setState({ openTabs: [tab()], activeTabId: "flow", selection: { kind: "none", id: null }, history: {} });
  stop = startDraftSessions(useEditStore);
});

afterEach(() => {
  stop();
  useEditStore.setState({ openTabs: [], activeTabId: null });
});

describe("a pipeline tab's shared draft", () => {
  it("opens with the tab, and takes no edit before the draft arrives", () => {
    const socket = sockets.get("flow")!;
    expect(socket).toBeDefined();
    expect(current().draft?.synced).toBe(false);
    useEditStore.getState().updateNode("a", { name: "too soon" });
    expect(current().pipeline.nodes[0].name).toBe("A");
    socket.handlers.onOpen();
    expect(socket.sent[0].tag).toBe(SYNC_REQUEST);
  });

  it("shows the draft once it arrives, with its participants", () => {
    join(serverDoc());
    expect(current().draft).toMatchObject({ synced: true, canWrite: true, connection: "1", status: "live" });
    expect(current().draft?.participants).toHaveLength(2);
    expect(current().prompts).toEqual({ a: "Read." });
  });

  it("sends my change, field by field, to the daemon", () => {
    const server = serverDoc();
    const socket = join(server);
    useEditStore.getState().updateNode("a", { name: "Designer" });
    const update = socket.sent.find((m) => m.tag === UPDATE);
    expect(update).toBeDefined();
    Y.applyUpdate(server, update!.payload);
    const nodes = server.getMap("pipeline").toJSON().nodes as { name: string }[];
    expect(nodes[0].name).toBe("Designer");
  });

  it("shows a change from another participant without reloading", () => {
    const server = serverDoc();
    const socket = join(server);
    const vector = Y.encodeStateVector(server);
    const before = toDraftPipeline(pipeline);
    const after = JSON.parse(JSON.stringify(before)) as { [key: string]: Json };
    (after.nodes as { [key: string]: Json }[])[1].view = { x: 400, y: 100 };
    server.transact(() => applyObject(server.getMap("pipeline"), before, after));
    socket.handlers.onBinary(UPDATE, Y.encodeStateAsUpdate(server, vector));
    expect(current().pipeline.nodes[1].view).toEqual({ x: 400, y: 100 });
  });

  it("undoes my last change only, and names it", () => {
    const server = serverDoc();
    const socket = join(server);
    useEditStore.getState().updateNode("a", { name: "Designer" });
    expect(current().draft?.undoLabel).toBe("rename A to Designer");
    Y.applyUpdate(server, socket.sent.find((m) => m.tag === UPDATE)!.payload);
    // Someone else moves node b.
    const vector = Y.encodeStateVector(server);
    const before = server.getMap("pipeline").toJSON() as { [key: string]: Json };
    const after = JSON.parse(JSON.stringify(before)) as { [key: string]: Json };
    (after.nodes as { [key: string]: Json }[])[1].view = { x: 400, y: 100 };
    server.transact(() => applyObject(server.getMap("pipeline"), before, after));
    socket.handlers.onBinary(UPDATE, Y.encodeStateAsUpdate(server, vector));

    useEditStore.getState().undo();
    expect(current().pipeline.nodes[0].name).toBe("A");
    expect(current().pipeline.nodes[1].view).toEqual({ x: 400, y: 100 });
    expect(current().draft?.canRedo).toBe(true);
  });

  it("reads when it may not update the pipeline: no edit, nothing sent", () => {
    const socket = join(serverDoc(), false);
    useEditStore.getState().updateNode("a", { name: "Designer" });
    expect(current().pipeline.nodes[0].name).toBe("A");
    expect(socket.sent.filter((m) => m.tag === UPDATE)).toHaveLength(0);
  });

  it("marks the tab unsaved from the draft's changes, whoever made them", () => {
    const socket = join(serverDoc());
    socket.handlers.onMessage({ type: "status", changes: 3 });
    expect(current().dirty).toBe(true);
    socket.handlers.onMessage({ type: "status", changes: 0 });
    expect(current().dirty).toBe(false);
  });

  it("tells the others what I selected", () => {
    const socket = join(serverDoc());
    useEditStore.getState().setSelection({ kind: "node", id: "b" });
    expect(socket.texts.at(-1)).toEqual({
      type: "presence",
      presence: { selection: { kind: "node", id: "b" }, typing: null },
    });
  });

  it("stops, and reads, once the pipeline is deleted", () => {
    const socket = join(serverDoc());
    socket.handlers.onMessage({ type: "removed" });
    expect(sockets.has("flow")).toBe(false);
    expect(current().draft).toMatchObject({ status: "offline", canWrite: false });
  });

  it("edits without a draft when the draft cannot open", () => {
    const socket = sockets.get("flow")!;
    socket.handlers.onMessage({ type: "unavailable", message: "the file no longer reads" });
    expect(current().draft).toBeUndefined();
    expect(sockets.has("flow")).toBe(false);
    useEditStore.getState().updateNode("a", { name: "Designer" });
    expect(current().pipeline.nodes[0].name).toBe("Designer");
  });

  it("opens a prompt for typing without showing an empty one", () => {
    const server = serverDoc();
    const socket = join(server);
    useEditStore.getState().setSelection({ kind: "node", id: "b" });
    sessionOf().openPrompt("b");
    const opened = socket.sent.find((m) => m.tag === UPDATE);
    expect(opened).toBeDefined();
    Y.applyUpdate(server, opened!.payload);
    expect(server.getMap("prompts").get("b")).toBeInstanceOf(Y.Text);
    // A change from elsewhere reads the draft back: still no prompt for b.
    socket.handlers.onBinary(UPDATE, Y.encodeStateAsUpdate(server));
    expect(current().prompts).toEqual({ a: "Read." });
  });

  it("saves with the draft as the tab holds it", async () => {
    join(serverDoc());
    const content = sessionOf().content();
    expect(content.prompts).toEqual({ a: "Read." });
    expect((content.pipeline as { name: string }).name).toBe("flow");
    vi.mocked(savePipeline).mockResolvedValueOnce({ ok: true });
    await useEditStore.getState().save("flow");
    expect(savePipeline).toHaveBeenCalledWith("flow", expect.any(String), { a: "Read." }, "instance", content, undefined);
    // ADR-0088: as the « Save… » window says.
    const replace = { new_version: false, keep_old: true };
    vi.mocked(savePipeline).mockResolvedValueOnce({ ok: true, version: { number: 1, title: "v1" } });
    await useEditStore.getState().save("flow", replace);
    expect(vi.mocked(savePipeline).mock.calls.at(-1)?.[5]).toEqual(replace);
  });

  it("follows a rename to the new id", () => {
    const socket = join(serverDoc());
    socket.handlers.onMessage({ type: "moved", to: "shipping" });
    expect(current().id).toBe("shipping");
    expect(sockets.has("shipping")).toBe(true);
  });
});
