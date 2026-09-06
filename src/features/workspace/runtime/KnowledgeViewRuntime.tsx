import { createContext, useContext, type ReactNode } from "react";
import type { AppSettings } from "../../../types/appSettings";

export type KnowledgeViewRuntime = {
  knowledgeSettings: AppSettings["knowledge"];
  onOpenWork: () => void;
  onOpenNotes: () => void;
  onOpenSettings: () => void;
};

const KnowledgeViewRuntimeContext = createContext<KnowledgeViewRuntime | null>(null);

export function KnowledgeViewRuntimeProvider({ value, children }: { value: KnowledgeViewRuntime; children: ReactNode }) {
  return <KnowledgeViewRuntimeContext.Provider value={value}>{children}</KnowledgeViewRuntimeContext.Provider>;
}

export function useKnowledgeViewRuntime() {
  const runtime = useContext(KnowledgeViewRuntimeContext);
  if (!runtime) throw new Error("Knowledge view runtime is not initialized.");
  return runtime;
}
