/**
 * Transient messages at the bottom right of the window, one line each: the
 * kind's icon in its status colour, the text, an action, the close. Each goes
 * away after a few seconds (counted again once the pointer leaves it, so a
 * text cut short can be read in its tooltip), or on its ✕. A toast may carry a title and one
 * action button (« Take control »). The extensions post theirs through
 * `toast()` of `@pdo/extension-ui`, into the same stack.
 */

import type { ReactNode } from "react";
import { create } from "zustand";

export type ToastKind = "info" | "success" | "warning" | "error";

/** The one button a toast may offer; clicking it also dismisses the toast. */
export interface ToastAction {
  label: string;
  run: () => void;
}

export interface Toast {
  id: number;
  message: ReactNode;
  kind: ToastKind;
  title?: string;
  action?: ToastAction;
}

interface ToastsState {
  toasts: Toast[];
  dismiss: (id: number) => void;
  /** Keep the toast `id` while the pointer is over it. */
  hold: (id: number) => void;
  /** Count its lifetime again, from the start. */
  release: (id: number) => void;
}

const LIFETIME_MS = 6000;
let nextId = 1;
const timers = new Map<number, ReturnType<typeof setTimeout>>();

function stopTimer(id: number) {
  clearTimeout(timers.get(id));
  timers.delete(id);
}

function startTimer(id: number) {
  stopTimer(id);
  timers.set(id, setTimeout(() => useToasts.getState().dismiss(id), LIFETIME_MS));
}

export const useToasts = create<ToastsState>((set) => ({
  toasts: [],
  dismiss: (id) => {
    stopTimer(id);
    set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) }));
  },
  hold: stopTimer,
  release: (id) => {
    if (useToasts.getState().toasts.some((t) => t.id === id)) startTimer(id);
  },
}));

/** Show `message` for a few seconds. */
export function toast(message: string, kind: ToastKind = "info"): void {
  showToast({ message, kind });
}

/** Show a toast with a title and an action, for a few seconds. */
export function showToast(toast: Omit<Toast, "id">): void {
  const id = nextId++;
  useToasts.setState((s) => ({ toasts: [...s.toasts, { ...toast, id }] }));
  startTimer(id);
}

/** Drop every toast (tests). */
export function resetToasts(): void {
  for (const id of [...timers.keys()]) stopTimer(id);
  useToasts.setState({ toasts: [] });
}
