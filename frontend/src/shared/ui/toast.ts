/**
 * Transient messages at the bottom of the window, styled like App's run-now
 * error: an `error` toast has the failed border, an `info` one the strong line.
 * Each goes away after a few seconds, or on its ✕.
 */

import { create } from "zustand";

export type ToastKind = "info" | "error";

interface Toast {
  id: number;
  message: string;
  kind: ToastKind;
}

interface ToastsState {
  toasts: Toast[];
  dismiss: (id: number) => void;
}

const LIFETIME_MS = 6000;
let nextId = 1;

export const useToasts = create<ToastsState>((set) => ({
  toasts: [],
  dismiss: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
}));

/** Show `message` for a few seconds. */
export function toast(message: string, kind: ToastKind = "info"): void {
  const id = nextId++;
  useToasts.setState((s) => ({ toasts: [...s.toasts, { id, message, kind }] }));
  setTimeout(() => useToasts.getState().dismiss(id), LIFETIME_MS);
}

/** Drop every toast (tests). */
export function resetToasts(): void {
  useToasts.setState({ toasts: [] });
}
