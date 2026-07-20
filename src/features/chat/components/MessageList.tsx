import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  BookOpenText,
  Lightbulb,
  ListTodo,
  MessageCircleQuestion,
  MessageSquarePlus,
} from "lucide-react";
import type { ChatMessage } from "../../../types/chat";
import { buildMessageNavigatorNodes, type MessageNavigatorNode } from "../utils/messageNavigator";
import { MessageBubble } from "./MessageBubble";
import { MessageNavigator } from "./MessageNavigator";
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

type MessageListProps = {
  messages: ChatMessage[];
  conversationId: string | null;
  hasConversation: boolean;
  actionsDisabled?: boolean;
  canRegenerate?: boolean;
  suggestionsDisabled?: boolean;
  onCreateConversation: () => void;
  onSuggestionSelect: (prompt: string) => void;
  onEditMessage: (messageId: string, content: string) => void;
  onRegenerateMessage: (messageId: string) => void;
  onDeleteMessage: (messageId: string) => void;
};

export function MessageList({
  messages,
  conversationId,
  hasConversation,
  actionsDisabled = false,
  canRegenerate = false,
  suggestionsDisabled = false,
  onCreateConversation,
  onSuggestionSelect,
  onEditMessage,
  onRegenerateMessage,
  onDeleteMessage,
}: MessageListProps) {
  const listRef = useRef<HTMLElement>(null);
  const threadRef = useRef<HTMLDivElement>(null);
  const isPinnedToBottomRef = useRef(true);
  const scrollFrameRef = useRef<number | null>(null);
  const messageElementsRef = useRef(new Map<string, HTMLDivElement>());
  const [activeNavigatorNodeId, setActiveNavigatorNodeId] = useState<string | null>(null);
  const navigatorNodes = useMemo(() => buildMessageNavigatorNodes(messages), [messages]);
  const showNavigator = navigatorNodes.length >= 4;

  const updateActiveNavigatorNode = useCallback(() => {
    const list = listRef.current;
    if (!list || navigatorNodes.length === 0) {
      setActiveNavigatorNodeId(null);
      return;
    }
    const readingLine = list.getBoundingClientRect().top + list.clientHeight * 0.3;
    let active = navigatorNodes[0];
    for (const node of navigatorNodes) {
      const element = messageElementsRef.current.get(node.targetMessageId);
      if (!element || element.getBoundingClientRect().top > readingLine) break;
      active = node;
    }
    setActiveNavigatorNodeId((current) => current === active.id ? current : active.id);
  }, [navigatorNodes]);

  const requestScrollToBottom = useCallback(() => {
    if (scrollFrameRef.current !== null) return;

    scrollFrameRef.current = requestAnimationFrame(() => {
      scrollFrameRef.current = null;
      const list = listRef.current;
      if (!list || !isPinnedToBottomRef.current) return;
      list.scrollTop = list.scrollHeight;
    });
  }, []);

  const handleScroll = useCallback(() => {
    const list = listRef.current;
    if (!list) return;
    const distanceToBottom = list.scrollHeight - list.scrollTop - list.clientHeight;
    isPinnedToBottomRef.current = distanceToBottom <= 48;
    updateActiveNavigatorNode();
  }, [updateActiveNavigatorNode]);

  useEffect(() => {
    isPinnedToBottomRef.current = true;
    setActiveNavigatorNodeId(navigatorNodes[navigatorNodes.length - 1]?.id ?? null);
    requestScrollToBottom();
    // 只在切换对话时重置；消息内容更新由滚动与 ResizeObserver 处理。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [conversationId, requestScrollToBottom]);

  useEffect(() => {
    const thread = threadRef.current;
    if (!thread || typeof ResizeObserver === "undefined") return;

    const observer = new ResizeObserver(() => {
      requestScrollToBottom();
      updateActiveNavigatorNode();
    });
    observer.observe(thread);
    requestScrollToBottom();
    return () => observer.disconnect();
  }, [hasConversation, messages.length, requestScrollToBottom, updateActiveNavigatorNode]);

  useEffect(() => () => {
    if (scrollFrameRef.current !== null) cancelAnimationFrame(scrollFrameRef.current);
  }, []);

  const navigateToNode = useCallback((node: MessageNavigatorNode) => {
    const list = listRef.current;
    const element = messageElementsRef.current.get(node.targetMessageId);
    if (!list || !element) return;
    isPinnedToBottomRef.current = false;
    setActiveNavigatorNodeId(node.id);
    const listRect = list.getBoundingClientRect();
    const elementRect = element.getBoundingClientRect();
    list.scrollTo({
      top: list.scrollTop + elementRect.top - listRect.top - 24,
      behavior: window.matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth",
    });
  }, []);

  const navigateStep = useCallback((direction: -1 | 1) => {
    if (navigatorNodes.length === 0) return;
    const currentIndex = Math.max(0, navigatorNodes.findIndex((node) => node.id === activeNavigatorNodeId));
    const nextIndex = Math.max(0, Math.min(navigatorNodes.length - 1, currentIndex + direction));
    navigateToNode(navigatorNodes[nextIndex]);
  }, [activeNavigatorNodeId, navigateToNode, navigatorNodes]);

  return (
    <div className={`message-list-shell${showNavigator ? " message-list-shell-has-navigator" : ""}`}>
      <section
        className="message-list"
        aria-label="消息列表"
        ref={listRef}
        onScroll={handleScroll}
      >
        {!hasConversation ? (
        <div className="empty-chat-state">
          <div className="empty-chat-mark" aria-hidden="true">
            <MessageCircleQuestion size={28} />
          </div>
          <div className="empty-chat-copy">
            <span className="conversation-label">对话</span>
            <h2>还没有对话</h2>
          </div>
          <button className="empty-chat-action" type="button" onClick={onCreateConversation}>
            <MessageSquarePlus size={17} />
            <span>新建聊天</span>
          </button>
        </div>
      ) : messages.length === 0 ? (
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
              <button
                className="suggestion-item"
                type="button"
                key={title}
                disabled={suggestionsDisabled}
                onClick={() => onSuggestionSelect(title)}
              >
                <Icon size={18} />
                <span>
                  <strong>{title}</strong>
                  <small>{description}</small>
                </span>
              </button>
            ))}
          </div>
        </div>
        ) : (
          <div className="message-thread" ref={threadRef}>
            {messages.map((message) => (
              <div
                className="message-list-item"
                key={message.id}
                ref={(element) => {
                  if (element) messageElementsRef.current.set(message.id, element);
                  else messageElementsRef.current.delete(message.id);
                }}
              >
                <MessageBubble
                  message={message}
                  actionsDisabled={actionsDisabled}
                  canRegenerate={canRegenerate}
                  onEdit={onEditMessage}
                  onRegenerate={onRegenerateMessage}
                  onDelete={onDeleteMessage}
                />
              </div>
            ))}
          </div>
        )}
      </section>
      {showNavigator ? (
        <MessageNavigator
          nodes={navigatorNodes}
          activeNodeId={activeNavigatorNodeId}
          onNavigate={navigateToNode}
          onNavigateStep={navigateStep}
        />
      ) : null}
    </div>
  );
}
