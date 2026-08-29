import { useEffect, type ComponentProps } from "react";
import { ChatHeader } from "./ChatHeader";
import { ChatInput } from "./ChatInput";
import { MessageList } from "./MessageList";
import type { WorkspaceMode } from "../../workspace/types";
import { speechController } from "../speech/speechController";

type ChatWorkspaceProps = {
  mode: WorkspaceMode;
  inputKey: string;
  header: Omit<ComponentProps<typeof ChatHeader>, "compact">;
  messages: ComponentProps<typeof MessageList>;
  input: ComponentProps<typeof ChatInput>;
};

/** Chat 主界面与各学习视图右侧 AI 面板共享的纯展示组合。 */
export function ChatWorkspace({ mode, inputKey, header, messages, input }: ChatWorkspaceProps) {
  useEffect(() => {
    // A speech target belongs to one conversation. Switching the workspace
    // must not leave an old answer speaking in the new conversation.
    speechController.stop();
  }, [inputKey]);

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
