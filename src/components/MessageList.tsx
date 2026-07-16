import { MessageBubble } from "./MessageBubble";
import "../styles/message-list.css";

export function MessageList() {
  return (
    <section className="message-list" aria-label="消息列表">
      <div className="message-content">
        <div className="conversation-intro">
          <span className="conversation-label">新对话</span>
          <h2>从一个问题开始</h2>
          <p>Mnemora 将帮助你整理思路、阅读资料，并逐步建立自己的知识库。</p>
        </div>

        <MessageBubble role="user">
          <p>你好，请介绍一下这个项目接下来会完成什么。</p>
        </MessageBubble>

        <MessageBubble role="assistant">
          <p>
            Mnemora 的第一阶段会先完成基础 Chat，包括对话管理、模型配置、流式回复和本地保存。
          </p>
          <p>Chat 稳定后，我们再加入 PDF 阅读、批注、笔记和文献检索能力。</p>
        </MessageBubble>
      </div>
    </section>
  );
}
