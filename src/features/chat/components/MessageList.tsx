import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  BookOpenText,
  Lightbulb,
  ListTodo,
  MessageCircleQuestion,
  MessageSquarePlus,
} from "lucide-react";
import { Virtualizer, type VirtualizerHandle } from "virtua";
import type { ChatMessage } from "../../../types/chat";
import {
  activeMessageNavigatorNodeId,
  buildMessageNavigatorNodes,
  type MessageNavigatorNode,
} from "../utils/messageNavigator";
import { MessageBubble, type MessageBubbleUiState } from "./MessageBubble";
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

const BOTTOM_LEAVE_THRESHOLD_PX = 32;
const BOTTOM_REENTER_THRESHOLD_PX = 16;

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

function prefersReducedMotion() {
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

function defaultMessageUiState(message: ChatMessage): MessageBubbleUiState {
  return {
    reasoningOpen: false,
    userExpanded: false,
    editing: false,
    editDraft: message.content,
  };
}

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
  const virtualizerRef = useRef<VirtualizerHandle>(null);
  const isPinnedToBottomRef = useRef(true);
  const atBottomRef = useRef(true);
  const lastScrollOffsetRef = useRef(0);
  const scrollFrameRef = useRef<number | null>(null);
  const previousMessageCountRef = useRef(0);
  const navigatorNodesRef = useRef<MessageNavigatorNode[]>([]);
  const activeNavigatorNodeIdRef = useRef<string | null>(null);
  const messagesRef = useRef(messages);
  messagesRef.current = messages;

  const [activeNavigatorNodeId, setActiveNavigatorNodeId] = useState<string | null>(null);
  const [messageUiState, setMessageUiState] = useState<Record<string, MessageBubbleUiState>>({});
  const navigatorNodes = useMemo(() => buildMessageNavigatorNodes(messages), [messages]);
  navigatorNodesRef.current = navigatorNodes;
  const showNavigator = navigatorNodes.length >= 4;

  const updateActiveNavigatorNode = useCallback((nodeId: string | null) => {
    if (activeNavigatorNodeIdRef.current === nodeId) return;
    activeNavigatorNodeIdRef.current = nodeId;
    setActiveNavigatorNodeId(nodeId);
  }, []);

  const updateMessageUiState = useCallback((
    messageId: string,
    patch: Partial<MessageBubbleUiState>,
  ) => {
    const message = messagesRef.current.find((item) => item.id === messageId);
    if (!message) return;
    setMessageUiState((current) => {
      const previous = current[messageId] ?? defaultMessageUiState(message);
      const next = { ...previous, ...patch };
      if (
        previous.reasoningOpen === next.reasoningOpen
        && previous.userExpanded === next.userExpanded
        && previous.editing === next.editing
        && previous.editDraft === next.editDraft
      ) return current;
      return { ...current, [messageId]: next };
    });
  }, []);

  const scrollToBottom = useCallback((smooth = false) => {
    const lastIndex = messages.length - 1;
    if (lastIndex < 0) return;
    const handle = virtualizerRef.current;
    if (handle) {
      handle.scrollToIndex(lastIndex, {
        align: "end",
        smooth: smooth && !prefersReducedMotion(),
      });
      lastScrollOffsetRef.current = handle.scrollOffset;
      return;
    }
    const list = listRef.current;
    if (!list) return;
    list.scrollTo({
      top: list.scrollHeight,
      behavior: smooth && !prefersReducedMotion() ? "smooth" : "auto",
    });
    lastScrollOffsetRef.current = list.scrollTop;
  }, [messages.length]);

  const requestScrollToBottom = useCallback((smooth = false) => {
    if (scrollFrameRef.current !== null) return;
    scrollFrameRef.current = requestAnimationFrame(() => {
      scrollFrameRef.current = null;
      if (isPinnedToBottomRef.current) scrollToBottom(smooth);
    });
  }, [scrollToBottom]);

  const handleScroll = useCallback((nextOffset: number) => {
    const list = listRef.current;
    const handle = virtualizerRef.current;
    const offset = handle?.scrollOffset ?? nextOffset;
    const scrollSize = handle?.scrollSize ?? list?.scrollHeight ?? 0;
    const viewportSize = handle?.viewportSize ?? list?.clientHeight ?? 0;
    const distanceToBottom = scrollSize - offset - viewportSize;
    const atBottom = atBottomRef.current
      ? distanceToBottom <= BOTTOM_LEAVE_THRESHOLD_PX
      : distanceToBottom <= BOTTOM_REENTER_THRESHOLD_PX;

    if (offset < lastScrollOffsetRef.current - 1) {
      isPinnedToBottomRef.current = false;
    } else if (atBottom) {
      isPinnedToBottomRef.current = true;
    }
    atBottomRef.current = atBottom;
    lastScrollOffsetRef.current = offset;

    if (handle && navigatorNodesRef.current.length > 0) {
      const readingOffset = Math.min(
        Math.max(0, scrollSize - 1),
        offset + viewportSize * 0.3,
      );
      const renderIndex = handle.findItemIndex(readingOffset);
      updateActiveNavigatorNode(
        activeMessageNavigatorNodeId(navigatorNodesRef.current, renderIndex),
      );
    }
  }, [updateActiveNavigatorNode]);

  const navigateToNode = useCallback((node: MessageNavigatorNode) => {
    const handle = virtualizerRef.current;
    if (!handle) return;
    isPinnedToBottomRef.current = false;
    atBottomRef.current = false;
    updateActiveNavigatorNode(node.id);
    handle.scrollToIndex(node.targetRenderIndex, {
      align: "start",
      smooth: !prefersReducedMotion(),
    });
  }, [updateActiveNavigatorNode]);

  const navigateStep = useCallback((direction: -1 | 1) => {
    const nodes = navigatorNodesRef.current;
    if (nodes.length === 0) return;
    const currentIndex = Math.max(
      0,
      nodes.findIndex((node) => node.id === activeNavigatorNodeIdRef.current),
    );
    const nextIndex = Math.max(0, Math.min(nodes.length - 1, currentIndex + direction));
    navigateToNode(nodes[nextIndex]);
  }, [navigateToNode]);

  const renderMessage = useCallback((message: ChatMessage, index: number) => (
    <div
      className={`message-list-item${index === 0 ? " message-list-item-first" : ""}${
        index === messages.length - 1 ? " message-list-item-last" : ""
      }`}
      key={message.id}
    >
      <MessageBubble
        message={message}
        uiState={messageUiState[message.id] ?? defaultMessageUiState(message)}
        actionsDisabled={actionsDisabled}
        canRegenerate={canRegenerate}
        onUiStateChange={updateMessageUiState}
        onEdit={onEditMessage}
        onRegenerate={onRegenerateMessage}
        onDelete={onDeleteMessage}
      />
    </div>
  ), [
    actionsDisabled,
    canRegenerate,
    messageUiState,
    messages.length,
    onDeleteMessage,
    onEditMessage,
    onRegenerateMessage,
    updateMessageUiState,
  ]);

  useLayoutEffect(() => {
    isPinnedToBottomRef.current = true;
    atBottomRef.current = true;
    lastScrollOffsetRef.current = 0;
    setMessageUiState({});
    const lastNode = navigatorNodesRef.current[navigatorNodesRef.current.length - 1];
    updateActiveNavigatorNode(lastNode?.id ?? null);
    requestScrollToBottom();
    // 只在切换会话时重置界面状态和滚动意图。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [conversationId, updateActiveNavigatorNode]);

  useLayoutEffect(() => {
    const previousCount = previousMessageCountRef.current;
    if (
      messages.length > previousCount
      && messages.slice(previousCount).some((message) => message.role === "user")
    ) {
      isPinnedToBottomRef.current = true;
      atBottomRef.current = true;
    }
    previousMessageCountRef.current = messages.length;
    if (isPinnedToBottomRef.current) requestScrollToBottom();
  }, [messages, requestScrollToBottom]);

  useEffect(() => {
    const messageIds = new Set(messages.map((message) => message.id));
    setMessageUiState((current) => {
      const entries = Object.entries(current).filter(([messageId]) => messageIds.has(messageId));
      if (entries.length === Object.keys(current).length) return current;
      return Object.fromEntries(entries);
    });
  }, [messages]);

  useEffect(() => {
    const thread = threadRef.current;
    if (!thread || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => {
      if (isPinnedToBottomRef.current) requestScrollToBottom();
    });
    observer.observe(thread);
    return () => observer.disconnect();
  }, [hasConversation, requestScrollToBottom]);

  useLayoutEffect(() => () => {
    if (scrollFrameRef.current !== null) cancelAnimationFrame(scrollFrameRef.current);
  }, []);

  return (
    <div className={`message-list-shell${showNavigator ? " message-list-shell-has-navigator" : ""}`}>
      <section
        className="message-list"
        aria-label="消息列表"
        ref={listRef}
        onWheel={(event) => {
          if (event.deltaY < 0) isPinnedToBottomRef.current = false;
        }}
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
            <Virtualizer
              ref={virtualizerRef}
              scrollRef={listRef}
              onScroll={handleScroll}
              data={messages}
            >
              {renderMessage}
            </Virtualizer>
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
