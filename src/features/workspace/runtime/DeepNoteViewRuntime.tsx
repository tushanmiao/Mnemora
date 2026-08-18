import { createContext, useContext, type ReactNode } from "react";
import type {
  DeepNoteRunDetail,
  NotePipelineActivity,
  NotePipelinePhase,
} from "../../chat/api/notePipeline";
import type { DeepNoteReview } from "../../../app/hooks/useNoteActions";

export type DeepNoteViewRuntime = {
  detail: DeepNoteRunDetail | null;
  review: DeepNoteReview | null;
  progress: DeepNoteProgress | null;
  busy: boolean;
  onAdjust: (requirement: string) => void;
  onConfirm: (selectedSectionIds: ReadonlySet<string>) => void;
  onCancel: () => void;
  onOpenNote: () => void;
  onReturn: () => void;
};

export type DeepNoteProgress = {
  runId: string | null;
  phase: NotePipelinePhase | null;
  current: number | null;
  total: number | null;
  message: string;
  updatedAt: number;
  terminal: boolean;
  degraded: boolean;
  activity?: NotePipelineActivity | null;
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
