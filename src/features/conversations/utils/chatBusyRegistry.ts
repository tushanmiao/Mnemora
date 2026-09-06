/**
 * 按会话记录「有请求在跑」。
 *
 * 原来这是一个全局 boolean（`requestInFlightRef`）。它在只有一个 Chat 运行时的时候够用，
 * 但一旦允许主对话与副窗对话并发生成就会误伤：主窗在生成时，副窗发送会被主窗的
 * 进行中状态挡住，而两者其实互不相干。
 *
 * 所以这里区分两种问法，调用点必须按语义选对：
 *
 * - `isBusy(id)`：**这一个会话**忙不忙。用于发送/重生成/编辑的守卫、以及释放或删除
 *   某一个会话前的保护 —— 别的会话在跑不应该影响它。
 * - `isAnyBusy()`：**任意会话**忙不忙。只用于会影响全部会话的动作（清空全部会话、
 *   退出前确认），这类动作没有"只针对一个会话"的说法。
 */
export type ChatBusyRegistry = {
  isBusy: (conversationId: string | null | undefined) => boolean;
  isAnyBusy: () => boolean;
  /** 当前正在生成的会话 ID 快照，顺序不保证。 */
  busyConversationIds: () => readonly string[];
  begin: (conversationId: string) => void;
  end: (conversationId: string) => void;
};

export function createChatBusyRegistry(): ChatBusyRegistry {
  const busy = new Set<string>();
  return {
    isBusy: (conversationId) => Boolean(conversationId) && busy.has(conversationId as string),
    isAnyBusy: () => busy.size > 0,
    busyConversationIds: () => [...busy],
    begin: (conversationId) => {
      if (conversationId) busy.add(conversationId);
    },
    end: (conversationId) => {
      busy.delete(conversationId);
    },
  };
}
