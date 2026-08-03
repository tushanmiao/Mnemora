import { createContext, useContext, type ReactNode } from "react";
import type { WorkContextPanelProps } from "../components/WorkContextPanel";
import type { WorkWorkspaceProps } from "../components/WorkWorkspace";

export type WorkViewRuntime = {
  workspace: WorkWorkspaceProps;
  contextPanel: WorkContextPanelProps | null;
};

const WorkViewRuntimeContext = createContext<WorkViewRuntime | null>(null);

export function WorkViewRuntimeProvider({
  value,
  children,
}: {
  value: WorkViewRuntime;
  children: ReactNode;
}) {
  return (
    <WorkViewRuntimeContext.Provider value={value}>
      {children}
    </WorkViewRuntimeContext.Provider>
  );
}

export function useWorkViewRuntime() {
  const runtime = useContext(WorkViewRuntimeContext);
  if (!runtime) throw new Error("PDF 学习视图运行时尚未初始化。");
  return runtime;
}
