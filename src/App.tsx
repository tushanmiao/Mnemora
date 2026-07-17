import { useCallback, useEffect, useRef, useState } from "react";
import "./styles/app.css";
import { ChatHeader } from "./components/ChatHeader";
import { ChatInput } from "./components/ChatInput";
import { MessageList } from "./components/MessageList";
import { Sidebar } from "./components/Sidebar";
import type { ChatMessage } from "./types/chat";

const LOCAL_CONVERSATION_ID = "local-conversation";
const MOCK_REPLY_DELAY_MS = 700;

function createMessageId() {
  return crypto.randomUUID();
}

function App() {
  const [theme, setTheme] = useState<"light" | "dark">("light");
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const replyTimersRef = useRef<Set<number>>(new Set());

  useEffect(() => {
    const replyTimers = replyTimersRef.current;

    return () => {
      replyTimers.forEach((timer) => window.clearTimeout(timer));
      replyTimers.clear();
    };
  }, []);

  const handleSendMessage = useCallback((rawContent: string) => {
    const content = rawContent.trim();
    if (!content) return;

    const now = Date.now();
    const userMessage: ChatMessage = {
      id: createMessageId(),
      conversationId: LOCAL_CONVERSATION_ID,
      role: "user",
      content,
      status: "completed",
      createdAt: now,
      updatedAt: now,
    };

    setMessages((currentMessages) => [...currentMessages, userMessage]);

    const replyTimer = window.setTimeout(() => {
      const replyCreatedAt = Date.now();
      const assistantMessage: ChatMessage = {
        id: createMessageId(),
        conversationId: LOCAL_CONVERSATION_ID,
        role: "assistant",
        content: "收到。我们可以继续围绕这个问题展开讨论。",
        status: "completed",
        createdAt: replyCreatedAt,
        updatedAt: replyCreatedAt,
      };

      setMessages((currentMessages) => [...currentMessages, assistantMessage]);
      replyTimersRef.current.delete(replyTimer);
    }, MOCK_REPLY_DELAY_MS);

    replyTimersRef.current.add(replyTimer);
  }, []);

  return (
    <main className="app-shell" data-theme={theme} aria-label="Mnemora application">
      <Sidebar />

      <section className="chat-workspace" aria-label="当前对话">
        <ChatHeader theme={theme} onToggleTheme={() => setTheme(theme === "light" ? "dark" : "light")} />
        <MessageList messages={messages} onSuggestionSelect={handleSendMessage} />
        <ChatInput onSend={handleSendMessage} />
      </section>
    </main>
  );
}

export default App;
