import type { Conversation } from "../../../types/conversation";

/**
 * 找出正在等待用户处理审批或反问的会话。
 *
 * 存在的理由是一个具体的失败：审批等待有 300 秒的后端超时（`TOOL_APPROVAL_TIMEOUT`），
 * 而弹窗只在对应消息可见时才渲染。用户切到别的视图、或者把副窗最小化之后，那次中断
 * 就会静默超时 —— 从用户视角看是"Agent 自己放弃了"，而且没有任何提示。
 *
 * 判据与 `AgentWorkflow` 的渲染条件保持一致（`status === "awaitingApproval"` 且带
 * `approvalId`）：等待结束时后端会用同一个 callId 再发一个 toolTrace，前端归约器会把
 * `approvalId` 清掉，所以这里不会把已经结束的中断算成待处理。
 */
export function pendingApprovalConversationIds(
  conversations: readonly Conversation[],
): string[] {
  const ids: string[] = [];
  for (const conversation of conversations) {
    const pending = conversation.messages.some((message) => (
      message.toolTraces?.some((trace) => (
        trace.status === "awaitingApproval" && Boolean(trace.approvalId)
      )) ?? false
    ));
    if (pending) ids.push(conversation.id);
  }
  return ids;
}
