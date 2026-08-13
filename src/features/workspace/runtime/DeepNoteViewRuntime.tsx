import { createContext, useContext, type ReactNode } from "react";
import type { DeepNoteRunDetail } from "../../chat/api/notePipeline";
import type { DeepNoteReview } from "../../../app/hooks/useNoteActions";

export type DeepNoteViewRuntime = {
  detail: DeepNoteRunDetail | null;
  review: DeepNoteReview | null;
  busy: boolean;
  onAdjust: (requirement: string) => void;
  onConfirm: (selectedSectionIds: ReadonlySet<string>) => void;
  onCancel: () => void;
  onReturn: () => void;
};

const Context = createContext<DeepNoteViewRuntime | null>(null);

export function DeepNoteViewRuntimeProvider({
  value,
  children,
}: {
  value: DeepNoteViewRuntime;
  children: ReactNode;
}) {
  return <Context.Provider value={value}>{children}</Context.Provider>;
}

export function useDeepNoteViewRuntime() {
  const runtime = useContext(Context);
  if (!runtime) throw new Error("深度笔记工作区运行时尚未初始化。");
  return runtime;
}
