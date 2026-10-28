/**
 * Transient messages at the bottom right of the window, one line each: the
 * kind's icon in its status colour, the text, an action, the close. Each goes
 * away after a few seconds, or on its ✕. A toast may carry a title and one
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
}

const LIFETIME_MS = 6000;
let nextId = 1;

export const useToasts = create<ToastsState>((set) => ({
  toasts: [],
  dismiss: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
}));

/** Show `message` for a few seconds. */
export function toast(message: string, kind: ToastKind = "info"): void {
  showToast({ message, kind });
}

/** Show a toast with a title and an action, for a few seconds. */
export function showToast(toast: Omit<Toast, "id">): void {
  const id = nextId++;
  useToasts.setState((s) => ({ toasts: [...s.toasts, { ...toast, id }] }));
  setTimeout(() => useToasts.getState().dismiss(id), LIFETIME_MS);
}

/** Drop every toast (tests). */
export function resetToasts(): void {
  useToasts.setState({ toasts: [] });
}
