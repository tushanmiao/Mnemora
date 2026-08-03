import type { ComponentProps } from "react";
import { ChatHeader } from "./ChatHeader";
import { ChatInput } from "./ChatInput";
import { MessageList } from "./MessageList";
import type { WorkspaceMode } from "../../workspace/types";

type ChatWorkspaceProps = {
  mode: WorkspaceMode;
  inputKey: string;
  header: Omit<ComponentProps<typeof ChatHeader>, "compact">;
  messages: ComponentProps<typeof MessageList>;
  input: ComponentProps<typeof ChatInput>;
};

/** Chat 主界面与各学习视图右侧 AI 面板共享的纯展示组合。 */
export function ChatWorkspace({ mode, inputKey, header, messages, input }: ChatWorkspaceProps) {
  return (
    <section
      className={`chat-workspace${mode !== "chat" ? " chat-workspace-panel" : ""}`}
      aria-label={mode === "work" ? "文献 AI 对话" : mode === "notes" ? "笔记 AI 对话" : "当前对话"}
    >
      <ChatHeader {...header} compact={mode !== "chat"} />
      <MessageList {...messages} />
      <ChatInput key={inputKey} {...input} />
    </section>
  );
}
