import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, beforeEach } from "vitest";
import NoteInspector from "./NoteInspector";
import { useEditStore } from "../stores/editStore";
import type { PipelineDef, NoteDef } from "../types";

function makePipeline(notes: NoteDef[]): PipelineDef {
  return {
    name: "test-pipeline",
    variables: {},
    nodes: [],
    edges: [],
    notes,
  };
}

function selectNote(notes: NoteDef[], noteId: string) {
  useEditStore.setState({
    openTabs: [
      {
        id: "tab1",
        scope: "repo",
        pipeline: makePipeline(notes),
        prompts: {},
        diagnostics: [],
        dirty: false,
        externalDirty: false,
      },
    ],
    activeTabId: "tab1",
    selection: { kind: "note", id: null, noteId },
  });
}

const note: NoteDef = { id: "n1", content: "hello note", view: { x: 10, y: 20 } };

describe("NoteInspector (#307)", () => {
  beforeEach(() => {
    useEditStore.setState({
      openTabs: [],
      activeTabId: null,
      selection: { kind: "none", id: null },
    });
  });

  it("renders nothing when the selection is not a note", () => {
    useEditStore.setState({
      openTabs: [
        {
          id: "tab1",
          scope: "repo",
          pipeline: makePipeline([note]),
          prompts: {},
          diagnostics: [],
          dirty: false,
          externalDirty: false,
        },
      ],
      activeTabId: "tab1",
      selection: { kind: "none", id: null },
    });
    const { container } = render(<NoteInspector />);
    expect(container.firstChild).toBeNull();
  });

  it("shows a stripped inspector: a Note header and a single content textarea", () => {
    selectNote([note], "n1");
    render(<NoteInspector />);
    expect(screen.getByTestId("note-inspector")).toBeInTheDocument();
    expect(screen.getByText("Note")).toBeInTheDocument();
    const textarea = screen.getByTestId("note-content");
    expect(textarea).toHaveValue("hello note");
    // No node-style affordances: no name/type/ports/model, no star button.
    expect(screen.queryByTestId("star-button")).toBeNull();
    expect(screen.queryByText(/model/i)).toBeNull();
  });

  it("commits content edits to the note via updateNote", () => {
    selectNote([note], "n1");
    render(<NoteInspector />);
    fireEvent.change(screen.getByTestId("note-content"), {
      target: { value: "edited body" },
    });
    const tab = useEditStore.getState().openTabs[0];
    expect(tab.pipeline.notes!.find((n) => n.id === "n1")!.content).toBe("edited body");
  });

  it("renders nothing when the selected note id is missing", () => {
    selectNote([note], "does-not-exist");
    const { container } = render(<NoteInspector />);
    expect(container.firstChild).toBeNull();
  });
});
