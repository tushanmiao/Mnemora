import { createContext, useContext, type ReactNode } from "react";
import type { OverviewRecentItem } from "../../overview/types";

export type OverviewViewRuntime = {
  onNewChat: () => void;
  onOpenNotes: () => void;
  onOpenWork: () => void;
  onOpenItem: (item: OverviewRecentItem) => void;
};

const OverviewViewRuntimeContext = createContext<OverviewViewRuntime | null>(null);

export function OverviewViewRuntimeProvider({ value, children }: { value: OverviewViewRuntime; children: ReactNode }) {
  return <OverviewViewRuntimeContext.Provider value={value}>{children}</OverviewViewRuntimeContext.Provider>;
}

export function useOverviewViewRuntime() {
  const runtime = useContext(OverviewViewRuntimeContext);
  if (!runtime) throw new Error("Overview view runtime is not initialized.");
  return runtime;
}
