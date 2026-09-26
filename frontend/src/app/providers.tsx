import { StrictMode, type ReactNode } from "react";

/** What wraps the whole tree. */
export function AppProviders({ children }: { children: ReactNode }) {
  return <StrictMode>{children}</StrictMode>;
}
