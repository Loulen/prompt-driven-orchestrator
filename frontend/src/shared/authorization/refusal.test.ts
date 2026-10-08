import { beforeEach, describe, expect, it } from "vitest";
import { ApiError } from "../http/client";
import { resetToasts, useToasts } from "../ui/toast";
import { toastRefusal } from "./refusal";

const refused = new ApiError("operation refused: the test extension refuses pause_run run", {
  status: 403,
  body: { error: "operation_refused", message: "operation refused: the test extension refuses pause_run run", reason: "…" },
});

beforeEach(() => resetToasts());

describe("the refusal of a gesture (CORE-42)", () => {
  it("is shown with the daemon's sentence", () => {
    toastRefusal(refused);
    expect(useToasts.getState().toasts.map((t) => [t.message, t.kind])).toEqual([
      ["operation refused: the test extension refuses pause_run run", "error"],
    ]);
  });

  it("leaves any other failure silent, as before", () => {
    toastRefusal(new ApiError("conflict", { status: 409, body: { error: "run_not_live" } }));
    toastRefusal(new ApiError("forbidden", { status: 403, body: { error: "origin" } }));
    toastRefusal(new Error("offline"));
    expect(useToasts.getState().toasts).toEqual([]);
  });
});
