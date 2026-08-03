import { createContext, useContext, type ReactNode } from "react";
import type { NotesContextPanelProps } from "../../notes/components/NotesContextPanel";
import type { NotesWorkspaceProps } from "../../notes/components/NotesWorkspace";

export type NotesViewRuntime = {
  workspace: NotesWorkspaceProps;
  contextPanel: NotesContextPanelProps | null;
};

const NotesViewRuntimeContext = createContext<NotesViewRuntime | null>(null);

export function NotesViewRuntimeProvider({
  value,
  children,
}: {
  value: NotesViewRuntime;
  children: ReactNode;
}) {
  return (
    <NotesViewRuntimeContext.Provider value={value}>
      {children}
    </NotesViewRuntimeContext.Provider>
  );
}

export function useNotesViewRuntime() {
  const runtime = useContext(NotesViewRuntimeContext);
  if (!runtime) throw new Error("笔记视图运行时尚未初始化。");
  return runtime;
}
