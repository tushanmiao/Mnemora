import { useCallback, useMemo, useState, type CSSProperties } from "react";
import "./styles/app.css";
import { ChatHeader, type ModelSelectorGroup } from "./features/chat/components/ChatHeader";
import { ChatInput } from "./features/chat/components/ChatInput";
import { MessageList } from "./features/chat/components/MessageList";
import { useChatRuntime } from "./features/chat/hooks/useChatRuntime";
import { estimateConversationContext } from "./features/chat/utils/contextUsage";
import { activeContextMessages, contextSummaryPrompt } from "./features/chat/utils/contextCompression";
import { Sidebar } from "./features/conversations/components/Sidebar";
import { useConversations } from "./features/conversations/hooks/useConversations";
import { SettingsPage } from "./features/settings/components/SettingsPage";
import { useAppSettings } from "./features/settings/hooks/useAppSettings";
import type { AiPermissionMode } from "./types/chat";
import { resolveDefaultModel } from "./types/modelSettings";

type AppView = "chat" | "settings";

function App() {
  const [activeView, setActiveView] = useState<AppView>("chat");
  const navigateToChat = useCallback(() => setActiveView("chat"), []);

  const settings = useAppSettings();
  const conversations = useConversations(navigateToChat);

  const currentModel = useMemo(() => {
    const conversation = conversations.currentConversation;
    if (conversation?.providerId && conversation.modelId) {
      const provider = settings.modelSettings.providers.find(
        (item) => item.enabled && item.id === conversation.providerId,
      );
      const model = provider?.models.find(
        (item) => item.enabled && item.id === conversation.modelId,
      );
      if (provider && model) return { provider, model };
    } else if (conversation?.modelId) {
      for (const provider of settings.modelSettings.providers) {
        if (!provider.enabled) continue;
        const model = provider.models.find(
          (item) => item.enabled && item.id === conversation.modelId,
        );
        if (model) return { provider, model };
      }
    }
    return resolveDefaultModel(settings.modelSettings);
  }, [conversations.currentConversation, settings.modelSettings]);

  const modelGroups = useMemo<ModelSelectorGroup[]>(() => (
    settings.modelSettings.providers
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
            isDefault: settings.modelSettings.defaultProviderId === provider.id
              && settings.modelSettings.defaultModelId === model.id,
          })),
      }))
      .filter((group) => group.models.length > 0)
  ), [settings.modelSettings]);

  const contextUsage = useMemo(() => {
    const conversation = conversations.currentConversation;
    return estimateConversationContext(
      conversation ? activeContextMessages(conversation) : [],
      [
        settings.appSettings.systemPrompt,
        conversation?.systemPrompt ?? "",
        conversation ? contextSummaryPrompt(conversation) : "",
      ].filter(Boolean).join("\n\n"),
    );
  }, [conversations.currentConversation, settings.appSettings.systemPrompt]);

  const chatRuntime = useChatRuntime({
    appSettings: settings.appSettings,
    currentConversation: conversations.currentConversation,
    currentModel,
    conversationsRef: conversations.conversationsRef,
    requestInFlightRef: conversations.requestInFlightRef,
    cacheConversation: conversations.cacheConversation,
    saveStableConversation: conversations.saveStableConversation,
  });

  const handlePermissionChange = useCallback((permissionMode: AiPermissionMode) => {
    conversations.updateCurrentConversation((conversation) => ({
      ...conversation,
      permissionMode,
      updatedAt: Date.now(),
    }));
  }, [conversations]);

  const handleModelChange = useCallback((providerId: string, modelId: string) => {
    if (conversations.requestInFlightRef.current) return;
    const provider = settings.modelSettings.providers.find(
      (item) => item.enabled && item.id === providerId,
    );
    const model = provider?.models.find((item) => item.enabled && item.id === modelId);
    if (!provider || !model) return;
    conversations.updateCurrentConversation((conversation) => ({
      ...conversation,
      providerId: provider.id,
      modelId: model.id,
      updatedAt: Date.now(),
    }));
  }, [conversations, settings.modelSettings.providers]);

  return (
    <main
      className="app-shell"
      data-theme={settings.resolvedTheme}
      data-theme-color={settings.appSettings.themeColor}
      style={{ "--app-font-size": `${settings.appSettings.fontSize}px` } as CSSProperties}
      aria-label="Mnemora application"
    >
      <Sidebar
        settingsOpen={activeView === "settings"}
        userDisplayName={settings.appSettings.userDisplayName}
        userAvatar={settings.appSettings.userAvatar}
        conversations={conversations.conversationListItems}
        currentConversationId={conversations.currentConversationId}
        onCreateConversation={conversations.createNewConversation}
        onSelectConversation={conversations.selectConversation}
        onDeleteConversation={conversations.deleteConversation}
        onClearConversations={conversations.clearConversations}
        onOpenSettings={() => setActiveView("settings")}
      />

      {activeView === "settings" ? (
        <SettingsPage
          settings={settings.modelSettings}
          appSettings={settings.appSettings}
          initialError={settings.modelSettingsError}
          appSettingsError={settings.appSettingsError}
          onBack={navigateToChat}
          onSave={settings.saveModelSettings}
          onPreviewAppSettings={settings.previewAppSettings}
          onSaveAppSettings={settings.saveAppSettings}
          onSettingsImported={settings.applyImportedSettings}
          onDefaultModelChange={settings.changeDefaultModel}
        />
      ) : (
        <section className="chat-workspace" aria-label="当前对话">
          <ChatHeader
            title={conversations.currentConversation?.title ?? "未选择对话"}
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
            modelSelectionDisabled={!conversations.currentConversation || chatRuntime.requestInFlight}
            permission={conversations.currentConversation?.permissionMode ?? "askSensitive"}
            permissionDisabled={!conversations.currentConversation}
            theme={settings.resolvedTheme}
            onModelChange={handleModelChange}
            onPermissionChange={handlePermissionChange}
            onToggleTheme={settings.toggleTheme}
          />
          <MessageList
            messages={conversations.currentConversation?.messages ?? []}
            conversationId={conversations.currentConversationId}
            hasConversation={conversations.currentConversation !== null}
            suggestionsDisabled={!currentModel || chatRuntime.requestInFlight}
            onCreateConversation={conversations.createNewConversation}
            onSuggestionSelect={chatRuntime.sendMessage}
          />
          <ChatInput
            busy={chatRuntime.requestInFlight}
            stopDisabled={chatRuntime.stopRequested}
            disabled={!conversations.currentConversation || !currentModel}
            key={conversations.currentConversation?.id ?? "no-conversation"}
            placeholder={!conversations.currentConversation
              ? "请先新建对话"
              : !currentModel
                ? "请先配置默认模型"
                : chatRuntime.requestInFlight
                  ? "正在等待模型回复"
                  : "向 Mnemora 提问..."}
            contextUsage={contextUsage}
            contextWindowTokens={currentModel?.model.contextWindowTokens ?? null}
            contextMessageCount={conversations.currentConversation?.messages.length ?? 0}
            contextCompressionCount={conversations.currentConversation?.contextCompressionCount ?? 0}
            contextDisabled={!conversations.currentConversation || !currentModel}
            onSend={chatRuntime.sendMessage}
            onStop={settings.appSettings.streamEnabled ? chatRuntime.stopGeneration : undefined}
          />
        </section>
      )}
    </main>
  );
}

export default App;
