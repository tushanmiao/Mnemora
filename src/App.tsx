import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import "./styles/app.css";
import { ChatHeader, type ModelSelectorGroup } from "./components/ChatHeader";
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
  type ProviderApiKeyUpdate,
} from "./types/settings";
import {
  isTauriRuntime,
  loadModelSettings,
  persistModelSettings,
} from "./api/settings";
import { completeChat, normalizeModelError } from "./api/chat";

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
  const lastMessage = [...conversation.messages]
    .reverse()
    .find((message) => message.content || message.errorMessage);

  return {
    id: conversation.id,
    title: conversation.title,
    preview: lastMessage?.content || lastMessage?.errorMessage || "暂无消息",
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
  const [modelSettingsError, setModelSettingsError] = useState<string | null>(null);
  const [conversations, setConversations] = useState<Conversation[]>([initialConversation]);
  const [currentConversationId, setCurrentConversationId] = useState<string | null>(
    initialConversation.id,
  );
  const [requestInFlight, setRequestInFlight] = useState(false);
  const requestInFlightRef = useRef(false);

  const currentConversation = useMemo(
    () => conversations.find((conversation) => conversation.id === currentConversationId) ?? null,
    [conversations, currentConversationId],
  );

  const defaultModel = useMemo(
    () => resolveDefaultModel(modelSettings),
    [modelSettings],
  );

  const currentModel = useMemo(() => {
    if (currentConversation?.modelId) {
      for (const provider of modelSettings.providers) {
        if (!provider.enabled) continue;
        const model = provider.models.find(
          (item) => item.enabled && item.id === currentConversation.modelId,
        );
        if (model) return { provider, model };
      }
    }
    return defaultModel;
  }, [currentConversation?.modelId, defaultModel, modelSettings.providers]);

  const modelGroups = useMemo<ModelSelectorGroup[]>(() => (
    modelSettings.providers
      .filter((provider) => provider.enabled)
      .map((provider) => ({
        providerId: provider.id,
        providerName: provider.name,
        models: provider.models
          .filter((model) => model.enabled)
          .map((model) => ({
            id: model.id,
            displayName: model.displayName,
            apiModel: model.apiModel,
            isDefault: modelSettings.defaultProviderId === provider.id
              && modelSettings.defaultModelId === model.id,
          })),
      }))
      .filter((group) => group.models.length > 0)
  ), [modelSettings]);

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
    if (!isTauriRuntime()) return;
    let cancelled = false;

    void loadModelSettings()
      .then((settings) => {
        if (cancelled) return;
        setModelSettings(settings);
        setModelSettingsError(null);
      })
      .catch((error) => {
        if (cancelled) return;
        setModelSettingsError(error instanceof Error ? error.message : String(error));
      });

    return () => {
      cancelled = true;
    };
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
  }, [currentConversationId]);

  const handleClearConversations = useCallback(() => {
    setConversations([]);
    setCurrentConversationId(null);
  }, []);

  const handlePermissionChange = useCallback((permissionMode: AiPermissionMode) => {
    if (!currentConversationId) return;

    setConversations((currentConversations) => currentConversations.map((conversation) =>
      conversation.id === currentConversationId
        ? { ...conversation, permissionMode, updatedAt: Date.now() }
        : conversation,
    ));
  }, [currentConversationId]);

  const handleModelChange = useCallback((providerId: string, modelId: string) => {
    if (!currentConversationId || requestInFlightRef.current) return;
    const provider = modelSettings.providers.find(
      (item) => item.enabled && item.id === providerId,
    );
    const model = provider?.models.find((item) => item.enabled && item.id === modelId);
    if (!provider || !model) return;

    setConversations((currentConversations) => currentConversations.map((conversation) => (
      conversation.id === currentConversationId
        ? { ...conversation, modelId: model.id, updatedAt: Date.now() }
        : conversation
    )));
  }, [currentConversationId, modelSettings.providers]);

  const handleSaveModelSettings = useCallback(async (
    nextSettings: ModelSettings,
    apiKeyUpdates: ProviderApiKeyUpdate[],
  ) => {
    if (!isTauriRuntime()) {
      const updateByProvider = new Map(
        apiKeyUpdates.map((update) => [update.providerId, update] as const),
      );
      const browserSettings = {
        ...nextSettings,
        providers: nextSettings.providers.map((provider) => {
          const update = updateByProvider.get(provider.id);
          if (!update) return provider;
          return { ...provider, hasApiKey: update.action === "set" };
        }),
      };
      setModelSettings(browserSettings);
      setModelSettingsError(null);
      return browserSettings;
    }

    const saved = await persistModelSettings(nextSettings, apiKeyUpdates);
    setModelSettings(saved);
    setModelSettingsError(null);
    return saved;
  }, []);

  const handleSendMessage = useCallback(async (rawContent: string) => {
    const content = rawContent.trim();
    const targetConversation = currentConversation;
    const selectedModel = currentModel;
    if (
      !content
      || !targetConversation
      || !selectedModel
      || requestInFlightRef.current
    ) return;

    const now = Date.now();
    const targetConversationId = targetConversation.id;
    const userMessage: ChatMessage = {
      id: createId(),
      conversationId: targetConversationId,
      role: "user",
      content,
      status: "completed",
      createdAt: now,
      updatedAt: now,
    };
    const assistantMessageId = createId();
    const assistantMessage: ChatMessage = {
      id: assistantMessageId,
      conversationId: targetConversationId,
      role: "assistant",
      content: "",
      status: "pending",
      createdAt: now,
      updatedAt: now,
      modelId: selectedModel.model.id,
      modelSnapshot: {
        id: selectedModel.model.id,
        apiModel: selectedModel.model.apiModel,
        displayName: selectedModel.model.displayName,
        providerId: selectedModel.provider.id,
        providerName: selectedModel.provider.name,
      },
    };
    const modelMessages = [...targetConversation.messages, userMessage]
      .filter((message) => message.content.trim() && message.status === "completed")
      .map((message) => ({ role: message.role, content: message.content }));

    requestInFlightRef.current = true;
    setRequestInFlight(true);

    setConversations((currentConversations) => currentConversations.map((conversation) => {
      if (conversation.id !== targetConversationId) return conversation;

      return {
        ...conversation,
        title: conversation.messages.length === 0
          ? createTemporaryTitle(content)
          : conversation.title,
        messages: [...conversation.messages, userMessage, assistantMessage],
        modelId: selectedModel.model.id,
        updatedAt: now,
      };
    }));

    try {
      const response = await completeChat({
        providerId: selectedModel.provider.id,
        modelId: selectedModel.model.id,
        systemPrompt: targetConversation.systemPrompt,
        messages: modelMessages,
      });
      const completedAt = Date.now();
      setConversations((currentConversations) => currentConversations.map((conversation) =>
        conversation.id === targetConversationId
          ? {
              ...conversation,
              messages: conversation.messages.map((message) => (
                message.id === assistantMessageId
                  ? {
                      ...message,
                      content: response.text,
                      status: "completed",
                      usage: response.usage,
                      updatedAt: completedAt,
                    }
                  : message
              )),
              updatedAt: completedAt,
            }
          : conversation,
      ));
    } catch (error) {
      const modelError = normalizeModelError(error);
      const failedAt = Date.now();
      setConversations((currentConversations) => currentConversations.map((conversation) =>
        conversation.id === targetConversationId
          ? {
              ...conversation,
              messages: conversation.messages.map((message) => (
                message.id === assistantMessageId
                  ? {
                      ...message,
                      status: "error",
                      errorMessage: modelError.message,
                      updatedAt: failedAt,
                    }
                  : message
              )),
              updatedAt: failedAt,
            }
          : conversation,
      ));
    } finally {
      requestInFlightRef.current = false;
      setRequestInFlight(false);
    }
  }, [currentConversation, currentModel]);

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
          initialError={modelSettingsError}
          onBack={() => setActiveView("chat")}
          onSave={handleSaveModelSettings}
        />
      ) : (
        <section className="chat-workspace" aria-label="当前对话">
          <ChatHeader
            title={currentConversation?.title ?? "未选择对话"}
            modelLabel={currentModel
              ? `${currentModel.provider.name} · ${currentModel.model.displayName}`
              : "配置模型"}
            modelTitle={currentModel
              ? `${currentModel.provider.name} / ${currentModel.model.apiModel}`
              : "模型设置"}
            modelConfigured={Boolean(currentModel)}
            modelGroups={modelGroups}
            selectedProviderId={currentModel?.provider.id ?? null}
            selectedModelId={currentModel?.model.id ?? null}
            modelSelectionDisabled={!currentConversation || requestInFlight}
            permission={currentConversation?.permissionMode ?? "askSensitive"}
            permissionDisabled={!currentConversation}
            theme={theme}
            onModelChange={handleModelChange}
            onPermissionChange={handlePermissionChange}
            onToggleTheme={() => setTheme(theme === "light" ? "dark" : "light")}
          />
          <MessageList
            messages={currentConversation?.messages ?? []}
            hasConversation={currentConversation !== null}
            suggestionsDisabled={!currentModel || requestInFlight}
            onCreateConversation={handleCreateConversation}
            onSuggestionSelect={handleSendMessage}
          />
          <ChatInput
            busy={requestInFlight}
            disabled={!currentConversation || !currentModel || requestInFlight}
            key={currentConversation?.id ?? "no-conversation"}
            placeholder={!currentConversation
              ? "请先新建对话"
              : !currentModel
                ? "请先配置默认模型"
                : requestInFlight
                  ? "正在等待模型回复"
                  : "向 Mnemora 提问..."}
            onSend={handleSendMessage}
          />
        </section>
      )}
    </main>
  );
}

export default App;
