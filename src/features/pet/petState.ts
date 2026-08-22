import type { ChatMessage } from "../../types/chat";
import type { DeepNoteRunDetail } from "../chat/api/notePipeline";
import type { DeepNoteProgress } from "../workspace/runtime/DeepNoteViewRuntime";
import type { PetStatePayload } from "./types";

const RECENT_SUCCESS_MS = 8_000;

export function projectPetState(
  chatMessage: ChatMessage | null,
  deepNoteDetail: DeepNoteRunDetail | null,
  deepNoteProgress: DeepNoteProgress | null,
  now = Date.now(),
): PetStatePayload {
  const phase = deepNoteProgress?.phase ?? deepNoteDetail?.run.phase ?? null;
  if (phase) {
    const updatedAt = deepNoteProgress?.updatedAt ?? deepNoteDetail?.run.updatedAt ?? now;
    if (phase === "awaitingOutline") return payload("waiting", "等你确认", "提纲已经准备好", updatedAt);
    if (phase === "paused" || phase === "cancelled") return payload("waiting", "任务已停靠", "可以从检查点继续", updatedAt);
    if (phase === "error" || phase === "blocked") return payload("error", "遇到阻碍", "打开任务进度查看原因", updatedAt);
    if (phase === "done") {
      return now - updatedAt <= RECENT_SUCCESS_MS
        ? payload("success", "笔记完成", "知识已经收拢好了", updatedAt)
        : payload("idle", "陪你学习", "需要时我会告诉你进度", updatedAt);
    }
    const events = deepNoteDetail?.events ?? [];
    const latestEvent = events.length > 0 ? events[events.length - 1].eventType : "";
    if (latestEvent === "toolStarted") return payload("tooling", "正在读来源", "只处理已授权的附件", updatedAt);
    if (latestEvent === "modelRetryScheduled") return payload("waiting", "稍等一下", "模型请求正在重试", updatedAt);
    return payload("thinking", "正在整理知识", deepNoteProgress?.message ?? "深度笔记正在运行", updatedAt);
  }

  if (chatMessage) {
    const updatedAt = chatMessage.updatedAt;
    const activeTool = [...(chatMessage.toolTraces ?? [])].reverse().find((trace) => (
      trace.status === "awaitingApproval" || trace.status === "running"
    ));
    if (activeTool?.status === "awaitingApproval") return payload("waiting", "需要你确认", "有一项工具操作等待处理", updatedAt);
    if (activeTool?.status === "running") return payload("tooling", "正在使用工具", "只显示状态，不展示你的内容", updatedAt);
    if (chatMessage.status === "pending" || chatMessage.status === "streaming") {
      return payload("thinking", "正在思考", "答案正在形成", updatedAt);
    }
    if (chatMessage.status === "error") return payload("error", "回复失败", "回到 Mnemora 可以重试", updatedAt);
    if (chatMessage.status === "completed" && now - updatedAt <= RECENT_SUCCESS_MS) {
      return payload("success", "回答完成", "可以继续追问", updatedAt);
    }
  }
  return payload("idle", "陪你学习", "需要时我会告诉你进度", now);
}

function payload(
  state: PetStatePayload["state"],
  label: string,
  detail: string,
  updatedAt: number,
): PetStatePayload {
  return { state, label, detail, updatedAt };
}
