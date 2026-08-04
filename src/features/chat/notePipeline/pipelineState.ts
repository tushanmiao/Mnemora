import type { DeepNoteOutline } from "./outlineSchema";

export type DeepNotePhase =
  | "idle"
  | "preflight"
  | "analyzing"
  | "outlineReview"
  | "drafting"
  | "assembling"
  | "persisting"
  | "done"
  | "cancelled"
  | "error";

export interface DeepNotePipelineState {
  phase: DeepNotePhase;
  outline: DeepNoteOutline | null;
  currentSection: number;
  totalSections: number;
  message: string;
}

export type DeepNotePipelineEvent =
  | { type: "start" }
  | { type: "analyze" }
  | { type: "outlineReady"; outline: DeepNoteOutline }
  | { type: "draft"; total: number }
  | { type: "sectionCompleted"; current: number }
  | { type: "assemble" }
  | { type: "persist" }
  | { type: "complete" }
  | { type: "cancel" }
  | { type: "fail"; message: string };

export const INITIAL_DEEP_NOTE_STATE: DeepNotePipelineState = {
  phase: "idle",
  outline: null,
  currentSection: 0,
  totalSections: 0,
  message: "",
};

export function reduceDeepNotePipeline(
  state: DeepNotePipelineState,
  event: DeepNotePipelineEvent,
): DeepNotePipelineState {
  if (event.type === "cancel") return { ...state, phase: "cancelled", message: "已取消" };
  if (event.type === "fail") return { ...state, phase: "error", message: event.message };
  switch (event.type) {
    case "start":
      if (state.phase !== "idle") throw new Error("管线只能从 idle 启动。");
      return { ...state, phase: "preflight", message: "正在检查对话…" };
    case "analyze":
      if (state.phase !== "preflight" && state.phase !== "outlineReview") throw new Error("当前阶段不能分析提纲。");
      return { ...state, phase: "analyzing", message: "正在分析知识结构…" };
    case "outlineReady":
      if (state.phase !== "analyzing") throw new Error("当前阶段不能确认提纲。");
      return { ...state, phase: "outlineReview", outline: event.outline, message: "请确认提纲" };
    case "draft":
      if (state.phase !== "outlineReview") throw new Error("当前阶段不能开始扩写。");
      return { ...state, phase: "drafting", currentSection: 0, totalSections: event.total, message: `正在扩写 0/${event.total}` };
    case "sectionCompleted":
      if (state.phase !== "drafting") throw new Error("当前阶段不能记录章节进度。");
      return { ...state, currentSection: event.current, message: `正在扩写 ${event.current}/${state.totalSections}` };
    case "assemble":
      if (state.phase !== "drafting") throw new Error("当前阶段不能组装笔记。");
      return { ...state, phase: "assembling", message: "正在组装与检查笔记…" };
    case "persist":
      if (state.phase !== "assembling") throw new Error("当前阶段不能保存笔记。");
      return { ...state, phase: "persisting", message: "正在保存笔记与来源…" };
    case "complete":
      if (state.phase !== "persisting") throw new Error("当前阶段不能完成管线。");
      return { ...state, phase: "done", message: "深度笔记已生成" };
    default:
      return state;
  }
}
