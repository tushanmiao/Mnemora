import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import "./styles/app.css";
import { ChatHeader } from "./components/ChatHeader";
import { ChatInput } from "./components/ChatInput";
import { MessageList } from "./components/MessageList";
import { SettingsPage } from "./components/SettingsPage";
import { Sidebar } from "./components/Sidebar";
import type {
  AiPermissionMode,
  ChatMessage,
  Conversation,
  ConversationListItem,
} from "./types/chat";
import {
  createInitialModelSettings,
  resolveDefaultModel,
  type ModelSettings,
  type ProviderApiKeyDrafts,
} from "./types/settings";

const MOCK_REPLY_DELAY_MS = 700;
const DEFAULT_CONVERSATION_TITLE = "新对话";
const MAX_TEMPORARY_TITLE_LENGTH = 24;

type AppView = "chat" | "settings";

function createId() {
  return crypto.randomUUID();
}

function createConversation(): Conversation {
  const now = Date.now();

  return {
    id: createId(),
    title: DEFAULT_CONVERSATION_TITLE,
    messages: [],
    assistantId: null,
    modelId: null,
    systemPrompt: "",
    permissionMode: "askSensitive",
    projectId: null,
    collectionId: null,
    pinned: false,
    createdAt: now,
    updatedAt: now,
  };
}

function createTemporaryTitle(content: string) {
  const normalizedContent = content.replace(/\s+/g, " ").trim();
  const characters = Array.from(normalizedContent);

  if (characters.length <= MAX_TEMPORARY_TITLE_LENGTH) return normalizedContent;
  return `${characters.slice(0, MAX_TEMPORARY_TITLE_LENGTH).join("")}...`;
}

function toConversationListItem(conversation: Conversation): ConversationListItem {
  const lastMessage = conversation.messages[conversation.messages.length - 1];

  return {
    id: conversation.id,
    title: conversation.title,
    preview: lastMessage?.content ?? "暂无消息",
    messageCount: conversation.messages.length,
    assistantId: conversation.assistantId,
    modelId: conversation.modelId,
    projectId: conversation.projectId,
    collectionId: conversation.collectionId,
    pinned: conversation.pinned,
    createdAt: conversation.createdAt,
    updatedAt: conversation.updatedAt,
  };
}

const initialConversation = createConversation();

function App() {
  const [theme, setTheme] = useState<"light" | "dark">("light");
  const [activeView, setActiveView] = useState<AppView>("chat");
  const [modelSettings, setModelSettings] = useState<ModelSettings>(createInitialModelSettings);
  const [providerApiKeyDrafts, setProviderApiKeyDrafts] = useState<ProviderApiKeyDrafts>({});
  const [conversations, setConversations] = useState<Conversation[]>([initialConversation]);
  const [currentConversationId, setCurrentConversationId] = useState<string | null>(
    initialConversation.id,
  );
  const replyTimersRef = useRef<Map<number, string>>(new Map());

  const currentConversation = useMemo(
    () => conversations.find((conversation) => conversation.id === currentConversationId) ?? null,
    [conversations, currentConversationId],
  );

  const defaultModel = useMemo(
    () => resolveDefaultModel(modelSettings),
    [modelSettings],
  );

  const conversationListItems = useMemo(
    () => conversations
      .map(toConversationListItem)
      .sort((left, right) => {
        if (left.pinned !== right.pinned) return left.pinned ? -1 : 1;
        return right.updatedAt - left.updatedAt;
      }),
    [conversations],
  );

  useEffect(() => {
    const replyTimers = replyTimersRef.current;

    return () => {
      replyTimers.forEach((_, timer) => window.clearTimeout(timer));
      replyTimers.clear();
    };
  }, []);

  const cancelReplyTimers = useCallback((conversationId?: string) => {
    replyTimersRef.current.forEach((timerConversationId, timer) => {
      if (conversationId && timerConversationId !== conversationId) return;

      window.clearTimeout(timer);
      replyTimersRef.current.delete(timer);
    });
  }, []);

  const handleCreateConversation = useCallback(() => {
    const conversation = createConversation();
    setConversations((currentConversations) => [conversation, ...currentConversations]);
    setCurrentConversationId(conversation.id);
    setActiveView("chat");
  }, []);

  const handleSelectConversation = useCallback((conversationId: string) => {
    setCurrentConversationId(conversationId);
    setActiveView("chat");
  }, []);

  const handleDeleteConversation = useCallback((conversationId: string) => {
    cancelReplyTimers(conversationId);

    setConversations((currentConversations) => {
      const deletedIndex = currentConversations.findIndex(
        (conversation) => conversation.id === conversationId,
      );
      if (deletedIndex === -1) return currentConversations;

      const remainingConversations = currentConversations.filter(
        (conversation) => conversation.id !== conversationId,
      );

      if (currentConversationId === conversationId) {
        const nextConversation =
          remainingConversations[deletedIndex] ?? remainingConversations[deletedIndex - 1] ?? null;
        setCurrentConversationId(nextConversation?.id ?? null);
      }

      return remainingConversations;
    });
  }, [cancelReplyTimers, currentConversationId]);

  const handleClearConversations = useCallback(() => {
    cancelReplyTimers();
    setConversations([]);
    setCurrentConversationId(null);
  }, [cancelReplyTimers]);

  const handlePermissionChange = useCallback((permissionMode: AiPermissionMode) => {
    if (!currentConversationId) return;

    setConversations((currentConversations) => currentConversations.map((conversation) =>
      conversation.id === currentConversationId
        ? { ...conversation, permissionMode, updatedAt: Date.now() }
        : conversation,
    ));
  }, [currentConversationId]);

  const handleSaveModelSettings = useCallback((
    nextSettings: ModelSettings,
    nextApiKeyDrafts: ProviderApiKeyDrafts,
  ) => {
    setModelSettings(nextSettings);
    setProviderApiKeyDrafts(nextApiKeyDrafts);
  }, []);

  const handleSendMessage = useCallback((rawContent: string) => {
    const content = rawContent.trim();
    const targetConversationId = currentConversationId;
    if (!content || !targetConversationId) return;

    const now = Date.now();
    const userMessage: ChatMessage = {
      id: createId(),
      conversationId: targetConversationId,
      role: "user",
      content,
      status: "completed",
      createdAt: now,
      updatedAt: now,
    };

    setConversations((currentConversations) => currentConversations.map((conversation) => {
      if (conversation.id !== targetConversationId) return conversation;

      return {
        ...conversation,
        title: conversation.messages.length === 0
          ? createTemporaryTitle(content)
          : conversation.title,
        messages: [...conversation.messages, userMessage],
        modelId: conversation.modelId ?? defaultModel?.model.id ?? null,
        updatedAt: now,
      };
    }));

    const replyTimer = window.setTimeout(() => {
      const replyCreatedAt = Date.now();
      const assistantMessage: ChatMessage = {
        id: createId(),
        conversationId: targetConversationId,
        role: "assistant",
        content: "收到。我们可以继续围绕这个问题展开讨论。",
        status: "completed",
        createdAt: replyCreatedAt,
        updatedAt: replyCreatedAt,
        modelId: defaultModel?.model.id,
        modelSnapshot: defaultModel ? {
          id: defaultModel.model.id,
          apiModel: defaultModel.model.apiModel,
          displayName: defaultModel.model.displayName,
          providerId: defaultModel.provider.id,
          providerName: defaultModel.provider.name,
        } : undefined,
      };

      setConversations((currentConversations) => currentConversations.map((conversation) =>
        conversation.id === targetConversationId
          ? {
              ...conversation,
              messages: [...conversation.messages, assistantMessage],
              updatedAt: replyCreatedAt,
            }
          : conversation,
      ));
      replyTimersRef.current.delete(replyTimer);
    }, MOCK_REPLY_DELAY_MS);

    replyTimersRef.current.set(replyTimer, targetConversationId);
  }, [currentConversationId, defaultModel]);

  return (
    <main className="app-shell" data-theme={theme} aria-label="Mnemora application">
      <Sidebar
        settingsOpen={activeView === "settings"}
        conversations={conversationListItems}
        currentConversationId={currentConversationId}
        onCreateConversation={handleCreateConversation}
        onSelectConversation={handleSelectConversation}
        onDeleteConversation={handleDeleteConversation}
        onClearConversations={handleClearConversations}
        onOpenSettings={() => setActiveView("settings")}
      />

      {activeView === "settings" ? (
        <SettingsPage
          settings={modelSettings}
          apiKeyDrafts={providerApiKeyDrafts}
          onBack={() => setActiveView("chat")}
          onSave={handleSaveModelSettings}
        />
      ) : (
        <section className="chat-workspace" aria-label="当前对话">
          <ChatHeader
            title={currentConversation?.title ?? "未选择对话"}
            modelLabel={defaultModel
              ? `${defaultModel.provider.name} · ${defaultModel.model.displayName}`
              : "配置模型"}
            modelTitle={defaultModel
              ? `${defaultModel.provider.name} / ${defaultModel.model.apiModel}`
              : "模型设置"}
            modelConfigured={Boolean(defaultModel)}
            permission={currentConversation?.permissionMode ?? "askSensitive"}
            permissionDisabled={!currentConversation}
            theme={theme}
            onOpenSettings={() => setActiveView("settings")}
            onPermissionChange={handlePermissionChange}
            onToggleTheme={() => setTheme(theme === "light" ? "dark" : "light")}
          />
          <MessageList
            messages={currentConversation?.messages ?? []}
            hasConversation={currentConversation !== null}
            onCreateConversation={handleCreateConversation}
            onSuggestionSelect={handleSendMessage}
          />
          <ChatInput
            disabled={!currentConversation}
            key={currentConversation?.id ?? "no-conversation"}
            onSend={handleSendMessage}
          />
        </section>
      )}
    </main>
  );
}

export default App;
