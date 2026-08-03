import { useChatViewRuntime } from "../runtime/ChatViewRuntime";

/** Chat 是主视图，复用 App 层唯一的 Chat Runtime，不创建第二份运行状态。 */
export default function ChatView() {
  return useChatViewRuntime();
}
