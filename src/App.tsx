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
import {
  loadApplicationSettings,
  saveApplicationSettings,
} from "./api/appSettings";
import {
  cancelChatStream,
  completeChat,
  normalizeModelError,
  startChatStream,
  type ModelStreamEvent,
} from "./api/chat";
import {
  createInitialAppSettings,
  type AppSettings,
  type ResponseLanguage,
  type SettingsBundle,
} from "./types/appSettings";
import {
  clearStoredConversations,
  listStoredConversations,
  loadStoredConversation,
  persistConversation,
  removeStoredConversation,
} from "./api/conversations";

const DEFAULT_CONVERSATION_TITLE = "新对话";
const MAX_TEMPORARY_TITLE_LENGTH = 24;
const MAX_LOADED_CONVERSATIONS = 8;
const STARTS_IN_TAURI = isTauriRuntime();

type AppView = "chat" | "settings";

type ActiveStreamRun = {
  runId: string;
  conversationId: string;
  messageId: string;
  pendingText: string;
  frameId: number | null;
  terminalReceived: boolean;
};

const RESPONSE_LANGUAGE_PROMPTS: Partial<Record<ResponseLanguage, string>> = {
  zh: "请使用简体中文回答。",
  zhHant: "請使用繁體中文回答。",
  en: "Please answer in English.",
};

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
    providerId: null,
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

/** 按“全局设置 -> 对话设置 -> 回复语言”的顺序组合最终 System Prompt。 */
function composeSystemPrompt(settings: AppSettings, conversationPrompt: string) {
  return [
    settings.systemPrompt.trim(),
    conversationPrompt.trim(),
    RESPONSE_LANGUAGE_PROMPTS[settings.responseLanguage] ?? "",
  ].filter(Boolean).join("\n\n");
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
    providerId: conversation.providerId,
    modelId: conversation.modelId,
    projectId: conversation.projectId,
    collectionId: conversation.collectionId,
    pinned: conversation.pinned,
    createdAt: conversation.createdAt,
    updatedAt: conversation.updatedAt,
  };
}

function sortConversationListItems(items: ConversationListItem[]) {
  return [...items].sort((left, right) => {
    if (left.pinned !== right.pinned) return left.pinned ? -1 : 1;
    return right.updatedAt - left.updatedAt;
  });
}

const initialConversation = createConversation();

function App() {
  const [appSettings, setAppSettings] = useState<AppSettings>(createInitialAppSettings);
  const [appSettingsError, setAppSettingsError] = useState<string | null>(null);
  const [systemTheme, setSystemTheme] = useState<"light" | "dark">(() => (
    window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light"
  ));
  const [activeView, setActiveView] = useState<AppView>("chat");
  const [modelSettings, setModelSettings] = useState<ModelSettings>(createInitialModelSettings);
  const [modelSettingsError, setModelSettingsError] = useState<string | null>(null);
  const [conversations, setConversations] = useState<Conversation[]>(() => (
    STARTS_IN_TAURI ? [] : [initialConversation]
  ));
  const conversationsRef = useRef<Conversation[]>(STARTS_IN_TAURI ? [] : [initialConversation]);
  const [conversationListItems, setConversationListItems] = useState<ConversationListItem[]>(() => (
    STARTS_IN_TAURI ? [] : [toConversationListItem(initialConversation)]
  ));
  const [currentConversationId, setCurrentConversationId] = useState<string | null>(
    STARTS_IN_TAURI ? null : initialConversation.id,
  );
  const [requestInFlight, setRequestInFlight] = useState(false);
  const [stopRequested, setStopRequested] = useState(false);
  const requestInFlightRef = useRef(false);
  const activeStreamRunRef = useRef<ActiveStreamRun | null>(null);
  const selectionVersionRef = useRef(0);
  const conversationSaveChainsRef = useRef(new Map<string, Promise<void>>());

  const resolvedTheme = appSettings.theme === "system" ? systemTheme : appSettings.theme;

  const currentConversation = useMemo(
    () => conversations.find((conversation) => conversation.id === currentConversationId) ?? null,
    [conversations, currentConversationId],
  );

  const defaultModel = useMemo(
    () => resolveDefaultModel(modelSettings),
    [modelSettings],
  );

  const currentModel = useMemo(() => {
    if (currentConversation?.providerId && currentConversation.modelId) {
      const provider = modelSettings.providers.find(
        (item) => item.enabled && item.id === currentConversation.providerId,
      );
      const model = provider?.models.find(
        (item) => item.enabled && item.id === currentConversation.modelId,
      );
      if (provider && model) return { provider, model };
    } else if (currentConversation?.modelId) {
      // 兼容早期只保存 modelId 的本地会话；下次发送或选择模型时会补齐 providerId。
      for (const provider of modelSettings.providers) {
        if (!provider.enabled) continue;
        const model = provider.models.find(
          (item) => item.enabled && item.id === currentConversation.modelId,
        );
        if (model) return { provider, model };
      }
    }
    return defaultModel;
  }, [
    currentConversation?.modelId,
    currentConversation?.providerId,
    defaultModel,
    modelSettings.providers,
  ]);

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

  const cacheConversation = useCallback((
    conversation: Conversation,
    updateSummary = true,
  ) => {
    const nextCache = [
      conversation,
      ...conversationsRef.current.filter((item) => item.id !== conversation.id),
    ].slice(0, MAX_LOADED_CONVERSATIONS);
    conversationsRef.current = nextCache;
    setConversations(nextCache);

    if (updateSummary) {
      const summary = toConversationListItem(conversation);
      setConversationListItems((current) => sortConversationListItems([
        summary,
        ...current.filter((item) => item.id !== conversation.id),
      ]));
    }
  }, []);

  const saveStableConversation = useCallback((conversation: Conversation) => {
    if (!STARTS_IN_TAURI) return;
    const previous = conversationSaveChainsRef.current.get(conversation.id) ?? Promise.resolve();
    const saveOperation = previous
      .catch(() => undefined)
      .then(() => persistConversation(conversation))
      .then((summary) => {
        if (!summary) return;
        setConversationListItems((current) => sortConversationListItems([
          summary,
          ...current.filter((item) => item.id !== summary.id),
        ]));
      })
      .catch((error) => {
        console.error("保存会话失败", error);
      });
    conversationSaveChainsRef.current.set(conversation.id, saveOperation);
    void saveOperation.finally(() => {
      if (conversationSaveChainsRef.current.get(conversation.id) === saveOperation) {
        conversationSaveChainsRef.current.delete(conversation.id);
      }
    });
  }, []);

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const updateSystemTheme = (event: MediaQueryListEvent) => {
      setSystemTheme(event.matches ? "dark" : "light");
    };
    media.addEventListener("change", updateSystemTheme);
    return () => media.removeEventListener("change", updateSystemTheme);
  }, []);

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

    void loadApplicationSettings()
      .then((settings) => {
        if (cancelled) return;
        setAppSettings(settings);
        setAppSettingsError(null);
      })
      .catch((error) => {
        if (cancelled) return;
        setAppSettingsError(error instanceof Error ? error.message : String(error));
      });

    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!STARTS_IN_TAURI) return;
    let cancelled = false;

    void (async () => {
      try {
        const items = await listStoredConversations();
        if (cancelled) return;
        setConversationListItems(items);

        if (items.length > 0) {
          const conversation = await loadStoredConversation(items[0].id);
          if (cancelled) return;
          cacheConversation(conversation, false);
          setCurrentConversationId(conversation.id);
          return;
        }

        const conversation = createConversation();
        cacheConversation(conversation);
        setCurrentConversationId(conversation.id);
        saveStableConversation(conversation);
      } catch (error) {
        if (cancelled) return;
        console.error("加载本地会话失败", error);
        const conversation = createConversation();
        cacheConversation(conversation);
        setCurrentConversationId(conversation.id);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [cacheConversation, saveStableConversation]);

  const handleCreateConversation = useCallback(() => {
    const conversation = createConversation();
    cacheConversation(conversation);
    setCurrentConversationId(conversation.id);
    setActiveView("chat");
    saveStableConversation(conversation);
  }, [cacheConversation, saveStableConversation]);

  const handleSelectConversation = useCallback((conversationId: string) => {
    setCurrentConversationId(conversationId);
    setActiveView("chat");
    const cached = conversationsRef.current.find((conversation) => conversation.id === conversationId);
    if (cached || !STARTS_IN_TAURI) return;

    const selectionVersion = ++selectionVersionRef.current;
    void loadStoredConversation(conversationId)
      .then((conversation) => {
        if (selectionVersion !== selectionVersionRef.current) return;
        cacheConversation(conversation, false);
      })
      .catch((error) => {
        console.error("加载会话失败", error);
      });
  }, [cacheConversation]);

  const handleDeleteConversation = useCallback((conversationId: string) => {
    if (requestInFlightRef.current && currentConversationId === conversationId) return;
    const deletedIndex = conversationListItems.findIndex((item) => item.id === conversationId);
    const remainingItems = conversationListItems.filter((item) => item.id !== conversationId);
    setConversationListItems(remainingItems);
    const nextCache = conversationsRef.current.filter((conversation) => conversation.id !== conversationId);
    conversationsRef.current = nextCache;
    setConversations(nextCache);

    if (currentConversationId === conversationId) {
      const nextItem = remainingItems[deletedIndex] ?? remainingItems[deletedIndex - 1] ?? null;
      if (nextItem) handleSelectConversation(nextItem.id);
      else setCurrentConversationId(null);
    }
    if (STARTS_IN_TAURI) {
      const previous = conversationSaveChainsRef.current.get(conversationId) ?? Promise.resolve();
      const deleteOperation = previous
        .catch(() => undefined)
        .then(() => removeStoredConversation(conversationId))
        .then(() => undefined)
        .catch((error) => {
          console.error("删除会话失败", error);
        });
      conversationSaveChainsRef.current.set(conversationId, deleteOperation);
      void deleteOperation.finally(() => {
        if (conversationSaveChainsRef.current.get(conversationId) === deleteOperation) {
          conversationSaveChainsRef.current.delete(conversationId);
        }
      });
    }
  }, [conversationListItems, currentConversationId, handleSelectConversation]);

  const handleClearConversations = useCallback(() => {
    if (requestInFlightRef.current) return;
    conversationsRef.current = [];
    setConversations([]);
    setConversationListItems([]);
    setCurrentConversationId(null);
    if (STARTS_IN_TAURI) {
      const pendingWrites = [...conversationSaveChainsRef.current.values()];
      void Promise.allSettled(pendingWrites)
        .then(() => clearStoredConversations())
        .catch((error) => {
          console.error("清空会话失败", error);
        });
    }
  }, []);

  const handlePermissionChange = useCallback((permissionMode: AiPermissionMode) => {
    if (!currentConversation) return;
    const nextConversation = { ...currentConversation, permissionMode, updatedAt: Date.now() };
    cacheConversation(nextConversation);
    saveStableConversation(nextConversation);
  }, [cacheConversation, currentConversation, saveStableConversation]);

  const handleModelChange = useCallback((providerId: string, modelId: string) => {
    if (!currentConversation || requestInFlightRef.current) return;
    const provider = modelSettings.providers.find(
      (item) => item.enabled && item.id === providerId,
    );
    const model = provider?.models.find((item) => item.enabled && item.id === modelId);
    if (!provider || !model) return;

    const nextConversation = {
      ...currentConversation,
      providerId: provider.id,
      modelId: model.id,
      updatedAt: Date.now(),
    };
    cacheConversation(nextConversation);
    saveStableConversation(nextConversation);
  }, [cacheConversation, currentConversation, modelSettings.providers, saveStableConversation]);

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

  const handleSaveAppSettings = useCallback(async (nextSettings: AppSettings) => {
    if (!isTauriRuntime()) {
      setAppSettings(nextSettings);
      setAppSettingsError(null);
      return nextSettings;
    }

    const saved = await saveApplicationSettings(nextSettings);
    setAppSettings(saved);
    setAppSettingsError(null);
    return saved;
  }, []);

  const handleDefaultModelChange = useCallback(async (
    providerId: string,
    modelId: string,
  ) => {
    await handleSaveModelSettings({
      ...modelSettings,
      defaultProviderId: providerId,
      defaultModelId: modelId,
    }, []);
  }, [handleSaveModelSettings, modelSettings]);

  const handleSettingsImported = useCallback((bundle: SettingsBundle) => {
    setAppSettings(bundle.appSettings);
    setModelSettings(bundle.modelSettings);
    setAppSettingsError(null);
    setModelSettingsError(null);
  }, []);

  const handleToggleTheme = useCallback(() => {
    const nextSettings: AppSettings = {
      ...appSettings,
      theme: resolvedTheme === "light" ? "dark" : "light",
    };
    void handleSaveAppSettings(nextSettings).catch((error) => {
      setAppSettingsError(error instanceof Error ? error.message : String(error));
    });
  }, [appSettings, handleSaveAppSettings, resolvedTheme]);

  const flushStreamRun = useCallback((
    run: ActiveStreamRun,
    terminal?: {
      status: "completed" | "stopped" | "error";
      usage?: ChatMessage["usage"];
      errorMessage?: string;
    },
  ) => {
    if (run.frameId !== null) {
      cancelAnimationFrame(run.frameId);
      run.frameId = null;
    }
    const pendingText = run.pendingText;
    run.pendingText = "";
    if (!pendingText && !terminal) return;

    const conversation = conversationsRef.current.find((item) => item.id === run.conversationId);
    if (!conversation) return;
    const updatedAt = Date.now();
    const nextConversation: Conversation = {
      ...conversation,
      messages: conversation.messages.map((message) => (
        message.id === run.messageId
          ? {
              ...message,
              content: message.content + pendingText,
              status: terminal?.status ?? "streaming",
              usage: terminal?.usage ?? message.usage,
              errorMessage: terminal?.errorMessage,
              updatedAt,
            }
          : message
      )),
      updatedAt,
    };
    cacheConversation(nextConversation);
    if (terminal) saveStableConversation(nextConversation);
  }, [cacheConversation, saveStableConversation]);

  const handleStreamEvent = useCallback((event: ModelStreamEvent) => {
    const run = activeStreamRunRef.current;
    if (
      !run
      || event.runId !== run.runId
      || event.conversationId !== run.conversationId
      || event.messageId !== run.messageId
    ) return;

    switch (event.type) {
      case "started":
        return;
      case "textDelta":
        run.pendingText += event.delta;
        if (run.frameId === null) {
          run.frameId = requestAnimationFrame(() => {
            run.frameId = null;
            flushStreamRun(run);
          });
        }
        return;
      case "completed":
        run.terminalReceived = true;
        flushStreamRun(run, { status: "completed", usage: event.usage });
        return;
      case "stopped":
        run.terminalReceived = true;
        flushStreamRun(run, { status: "stopped" });
        return;
      case "error":
        run.terminalReceived = true;
        flushStreamRun(run, {
          status: "error",
          errorMessage: event.error.message,
        });
    }
  }, [flushStreamRun]);

  const handleStopGeneration = useCallback(() => {
    const run = activeStreamRunRef.current;
    if (!run || stopRequested) return;
    setStopRequested(true);
    void cancelChatStream(run.runId).catch(() => {
      setStopRequested(false);
    });
  }, [stopRequested]);

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
    setStopRequested(false);

    const title = targetConversation.messages.length === 0
      ? createTemporaryTitle(content)
      : targetConversation.title;
    const runningConversation: Conversation = {
      ...targetConversation,
      title,
      messages: [...targetConversation.messages, userMessage, assistantMessage],
      providerId: selectedModel.provider.id,
      modelId: selectedModel.model.id,
      updatedAt: now,
    };
    cacheConversation(runningConversation);
    saveStableConversation({
      ...runningConversation,
      messages: [...targetConversation.messages, userMessage],
    });

    let streamRun: ActiveStreamRun | null = null;
    try {
      const completionRequest = {
        providerId: selectedModel.provider.id,
        modelId: selectedModel.model.id,
        systemPrompt: composeSystemPrompt(appSettings, targetConversation.systemPrompt),
        messages: modelMessages,
        options: {
          maxOutputTokens: appSettings.maxOutputTokens,
        },
      };

      if (appSettings.streamEnabled) {
        streamRun = {
          runId: createId(),
          conversationId: targetConversationId,
          messageId: assistantMessageId,
          pendingText: "",
          frameId: null,
          terminalReceived: false,
        };
        activeStreamRunRef.current = streamRun;
        await startChatStream({
          runId: streamRun.runId,
          conversationId: streamRun.conversationId,
          messageId: streamRun.messageId,
          completion: completionRequest,
        }, handleStreamEvent);
        if (!streamRun.terminalReceived) {
          throw new Error("流式请求结束，但没有收到完成、停止或错误事件。");
        }
        return;
      }

      const response = await completeChat(completionRequest);
      const completedAt = Date.now();
      const conversation = conversationsRef.current.find((item) => item.id === targetConversationId);
      if (conversation) {
        const completedConversation: Conversation = {
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
        };
        cacheConversation(completedConversation);
        saveStableConversation(completedConversation);
      }
    } catch (error) {
      const modelError = normalizeModelError(error);
      if (streamRun) {
        if (!streamRun.terminalReceived) {
          streamRun.terminalReceived = true;
          flushStreamRun(streamRun, {
            status: "error",
            errorMessage: modelError.message,
          });
        }
        return;
      }
      const failedAt = Date.now();
      const conversation = conversationsRef.current.find((item) => item.id === targetConversationId);
      if (conversation) {
        const failedConversation: Conversation = {
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
        };
        cacheConversation(failedConversation);
        saveStableConversation(failedConversation);
      }
    } finally {
      if (streamRun && streamRun.frameId !== null) {
        cancelAnimationFrame(streamRun.frameId);
        streamRun.frameId = null;
      }
      if (activeStreamRunRef.current?.runId === streamRun?.runId) {
        activeStreamRunRef.current = null;
      }
      requestInFlightRef.current = false;
      setRequestInFlight(false);
      setStopRequested(false);
    }
  }, [
    appSettings,
    cacheConversation,
    currentConversation,
    currentModel,
    flushStreamRun,
    handleStreamEvent,
    saveStableConversation,
  ]);

  return (
    <main
      className="app-shell"
      data-theme={resolvedTheme}
      data-theme-color={appSettings.themeColor}
      aria-label="Mnemora application"
    >
      <Sidebar
        settingsOpen={activeView === "settings"}
        userDisplayName={appSettings.userDisplayName}
        userAvatar={appSettings.userAvatar}
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
          appSettings={appSettings}
          initialError={modelSettingsError}
          appSettingsError={appSettingsError}
          onBack={() => setActiveView("chat")}
          onSave={handleSaveModelSettings}
          onPreviewAppSettings={setAppSettings}
          onSaveAppSettings={handleSaveAppSettings}
          onSettingsImported={handleSettingsImported}
          onDefaultModelChange={handleDefaultModelChange}
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
            theme={resolvedTheme}
            onModelChange={handleModelChange}
            onPermissionChange={handlePermissionChange}
            onToggleTheme={handleToggleTheme}
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
            stopDisabled={stopRequested}
            disabled={!currentConversation || !currentModel}
            key={currentConversation?.id ?? "no-conversation"}
            placeholder={!currentConversation
              ? "请先新建对话"
              : !currentModel
                ? "请先配置默认模型"
                : requestInFlight
                  ? "正在等待模型回复"
                  : "向 Mnemora 提问..."}
            onSend={handleSendMessage}
            onStop={appSettings.streamEnabled ? handleStopGeneration : undefined}
          />
        </section>
      )}
    </main>
  );
}

export default App;
