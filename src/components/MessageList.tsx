import { BookOpenText, Lightbulb, ListTodo, MessageCircleQuestion } from "lucide-react";
import "../styles/message-list.css";

const suggestions = [
  {
    icon: Lightbulb,
    title: "梳理一个想法",
    description: "把零散思路整理成清晰结构",
  },
  {
    icon: BookOpenText,
    title: "阅读一篇文献",
    description: "后续可以从文献库选择 PDF",
  },
  {
    icon: ListTodo,
    title: "制定学习计划",
    description: "将目标拆分为可以执行的步骤",
  },
];

export function MessageList() {
  return (
    <section className="message-list" aria-label="消息列表">
      <div className="empty-chat-state">
        <div className="empty-chat-mark" aria-hidden="true">
          <MessageCircleQuestion size={28} />
        </div>
        <div className="empty-chat-copy">
          <span className="conversation-label">新对话</span>
          <h2>今天想研究什么？</h2>
          <p>提出一个问题，或者从下面的建议开始。</p>
        </div>

        <div className="suggestion-grid" aria-label="提问建议">
          {suggestions.map(({ icon: Icon, title, description }) => (
            <button className="suggestion-item" type="button" key={title}>
              <Icon size={18} />
              <span>
                <strong>{title}</strong>
                <small>{description}</small>
              </span>
            </button>
          ))}
        </div>
      </div>
    </section>
  );
}
