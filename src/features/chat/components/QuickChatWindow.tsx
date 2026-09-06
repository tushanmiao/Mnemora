import { useCallback, useEffect, useMemo, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ChatWorkspace } from "./ChatWorkspace";
import type { ModelSelectorGroup } from "./ModelSelector";
import { buildLocalCommandHelp, type LocalSlashCommand, type SlashCommandExecutionResult } from "../commands/slashCommands";
import { useChatRuntime } from "../hooks/useChatRuntime";
import { estimateConversationContext } from "../utils/contextUsage";
import { activeContextMessages, contextSummaryPrompt } from "../utils/contextCompression";
import { useAppSettings } from "../../settings/hooks/useAppSettings";
import { useSkills } from "../../skills/hooks/useSkills";
import { usePromptTemplates } from "../../prompts/hooks/usePromptTemplates";
import { useConversations } from "../../conversations/hooks/useConversations";
import { resolveConversationModel } from "../../../types/modelSettings";
import { matchModelDefaults, resolveSupportsFunctionCalling, resolveSupportsReasoning, resolveSupportsVision } from "../../../data/modelMatching";
import type { AiPermissionMode } from "../../../types/chat";
import { I18nProvider, useI18n } from "../../../i18n/I18nProvider";
import "../../../styles/tokens.css";
import "../../../styles/app.css";
import "../../../styles/themes.css";
import "../styles/quick-chat.css";

type SettingsRuntime = ReturnType<typeof useAppSettings>;

function QuickChatContent({ settings }: { settings: SettingsRuntime }) {
  const { t } = useI18n();
  const skills = useSkills();
  const prompts = usePromptTemplates();
  const conversations = useConversations(() => undefined, { startFresh: true });
  const [modelMenuRequest, setModelMenuRequest] = useState(0);
  const [composerFocusRequest] = useState(1);

  const currentConversation = conversations.currentConversation;
  const currentModel = useMemo(() => resolveConversationModel(
    settings.modelSettings,
    currentConversation?.providerId ?? null,
    currentConversation?.modelId ?? null,
  ), [currentConversation, settings.modelSettings]);

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
            reasoningEfforts: matchModelDefaults(model.apiModel)?.reasoningEfforts,
            isDefault: settings.modelSettings.defaultProviderId === provider.id
              && settings.modelSettings.defaultModelId === model.id,
          })),
      }))
      .filter((group) => group.models.length > 0)
  ), [settings.modelSettings]);

  const contextUsage = useMemo(() => estimateConversationContext(
    currentConversation ? activeContextMessages(currentConversation) : [],
    [
      settings.appSettings.systemPrompt,
      currentConversation?.systemPrompt ?? "",
      currentConversation ? contextSummaryPrompt(currentConversation) : "",
    ].filter(Boolean).join("\n\n"),
  ), [currentConversation, settings.appSettings.systemPrompt]);

  const chatRuntime = useChatRuntime({
    appSettings: settings.appSettings,
    workspaceMode: "chat",
    skills: skills.skills,
    currentConversation,
    currentModel,
    conversationsRef: conversations.conversationsRef,
    chatBusy: conversations.chatBusy,
    cacheConversation: conversations.cacheConversation,
    saveStableConversation: conversations.saveStableConversation,
    protectConversation: conversations.protectConversation,
    releaseConversation: conversations.releaseConversation,
  });

  const handlePermissionChange = useCallback((permissionMode: AiPermissionMode) => {
    conversations.updateCurrentConversation((conversation) => ({
      ...conversation,
      permissionMode,
      updatedAt: Date.now(),
    }));
  }, [conversations]);

  const handleModelChange = useCallback((providerId: string, modelId: string) => {
    if (conversations.chatBusy.isBusy(conversations.currentConversationId)) return;
    const provider = settings.modelSettings.providers.find(
      (item) => item.enabled && item.id === providerId,
    );
    const model = provider?.models.find((item) => item.enabled && item.id === modelId);
    if (!provider || !model) return;
    conversations.updateCurrentConversation((conversation) => {
      const efforts = matchModelDefaults(model.apiModel)?.reasoningEfforts ?? [];
      return {
        ...conversation,
        providerId: provider.id,
        modelId: model.id,
        reasoningEffort: conversation.reasoningEffort && efforts.includes(conversation.reasoningEffort)
          ? conversation.reasoningEffort
          : null,
        updatedAt: Date.now(),
      };
    });
  }, [conversations, settings.modelSettings.providers]);

  const handleSlashCommand = useCallback(async (
    command: LocalSlashCommand,
    argumentsValue: string,
  ): Promise<SlashCommandExecutionResult> => {
    switch (command) {
      case "help":
        return { executed: true, message: buildLocalCommandHelp() };
      case "new":
        conversations.createNewConversation();
        return { executed: true };
      case "clear": {
        if (!window.confirm("确定永久删除当前对话及其附件吗？关联笔记不会删除，但部分来源跳转将失效。此操作无法撤销。")) {
          return { executed: false, message: "已取消清除当前对话。" };
        }
        const deleted = await conversations.deleteCurrentConversationPermanently();
        if (!deleted) return { executed: false, message: "当前对话正在使用，暂时无法清除。" };
        conversations.createNewConversation();
        return { executed: true, message: "当前对话已清除。" };
      }
      case "compact":
        return chatRuntime.compactConversation(argumentsValue);
      case "model":
        setModelMenuRequest((request) => request + 1);
        return { executed: true };
      case "attach":
        return { executed: false, message: "附件命令由输入框处理。" };
      case "settings":
      case "skills":
      case "memory":
      case "install":
        return { executed: false, message: "快速聊天窗口不支持打开主窗口设置，请在主窗口中操作。" };
    }
  }, [chatRuntime, conversations]);

  const closeWindow = useCallback(() => {
    void getCurrentWindow().close();
  }, []);

  return (
    <main
      className="app-shell quick-chat-window"
      data-theme={settings.resolvedTheme}
      data-theme-preset={settings.appSettings.themePreset}
      data-theme-color={settings.appSettings.themeColor}
      aria-label="Mnemora 快速聊天"
    >
      <ChatWorkspace
        mode="chat"
        inputKey={currentConversation?.id ?? "quick-chat-loading"}
        header={{
          title: `${t("chat.quickChatTitle")} · ${currentConversation?.title ?? t("chat.newConversation")}`,
          permission: currentConversation?.permissionMode ?? "askSensitive",
          permissionDisabled: !currentConversation,
          theme: settings.resolvedTheme,
          onPermissionChange: handlePermissionChange,
          onToggleTheme: settings.toggleTheme,
          onCloseWindow: closeWindow,
          showTaskProgress: false,
        }}
        messages={{
          messages: currentConversation?.messages ?? [],
          conversationId: conversations.currentConversationId,
          hasConversation: currentConversation !== null,
          conversationLoading: conversations.currentConversationLoading,
          actionsDisabled: chatRuntime.requestInFlight,
          canRegenerate: Boolean(currentModel),
          suggestionsDisabled: !currentModel || chatRuntime.requestInFlight,
          onCreateConversation: conversations.createNewConversation,
          onSuggestionSelect: chatRuntime.sendMessage,
          onEditMessage: chatRuntime.editMessage,
          onRegenerateMessage: chatRuntime.regenerateMessage,
          onDeleteMessage: chatRuntime.deleteMessage,
        }}
        input={{
          conversationId: conversations.currentConversationId,
          busy: chatRuntime.requestInFlight,
          stopDisabled: chatRuntime.stopRequested,
          disabled: !currentConversation || !currentModel,
          placeholder: !currentConversation
            ? "正在创建新对话"
            : !currentModel
              ? "请先在主窗口配置模型"
              : chatRuntime.requestInFlight
                ? "正在等待模型回复"
                : "向 Mnemora 提问...",
          focusRequest: composerFocusRequest,
          contextUsage,
          contextWindowTokens: currentModel?.model.contextWindowTokens ?? null,
          maxOutputTokens: settings.appSettings.maxOutputTokens,
          supportsReasoning: currentModel
            ? resolveSupportsReasoning(currentModel.model.apiModel, currentModel.model.capabilities?.reasoning) ?? null
            : null,
          reasoningEfforts: currentModel ? (matchModelDefaults(currentModel.model.apiModel)?.reasoningEfforts ?? []) : [],
          thinkingEnabled: currentConversation?.thinkingEnabled ?? settings.appSettings.thinkingEnabled,
          reasoningEffort: currentConversation?.reasoningEffort ?? null,
          modelLabel: currentModel ? `${currentModel.provider.name} · ${currentModel.model.displayName}` : "配置模型",
          modelTitle: currentModel ? `${currentModel.provider.name} / ${currentModel.model.apiModel}` : "模型设置",
          modelConfigured: Boolean(currentModel),
          modelGroups,
          selectedProviderId: currentModel?.provider.id ?? null,
          selectedModelId: currentModel?.model.id ?? null,
          modelMenuRequest,
          modelSelectionDisabled: !currentConversation || chatRuntime.requestInFlight,
          onModelChange: handleModelChange,
          onThinkingChange: (enabled) => conversations.updateCurrentConversation((conversation) => ({ ...conversation, thinkingEnabled: enabled, updatedAt: Date.now() })),
          onReasoningEffortChange: (effort) => conversations.updateCurrentConversation((conversation) => ({ ...conversation, reasoningEffort: effort, updatedAt: Date.now() })),
          hasMessages: (currentConversation?.messages.length ?? 0) > 0,
          supportsVision: currentModel
            ? resolveSupportsVision(currentModel.model.apiModel, currentModel.model.capabilities?.vision) ?? null
            : null,
          supportsTools: currentModel
            ? resolveSupportsFunctionCalling(currentModel.model.apiModel, currentModel.model.capabilities?.functionCalling)
            : null,
          contextMessageCount: currentConversation?.messages.length ?? 0,
          contextCompressionCount: currentConversation?.contextCompressionCount ?? 0,
          contextDisabled: !currentConversation || !currentModel,
          contextMessages: currentConversation?.messages ?? [],
          contextSystemPrompt: settings.appSettings.systemPrompt,
          skills: skills.skills,
          promptTemplates: prompts.templates,
          onOpenPromptSettings: () => undefined,
          onSend: chatRuntime.sendMessage,
          onStop: settings.appSettings.streamEnabled ? chatRuntime.stopGeneration : undefined,
          onSlashCommand: handleSlashCommand,
        }}
      />
    </main>
  );
}

export default function QuickChatWindow() {
  const settings = useAppSettings();
  useEffect(() => {
    document.documentElement.classList.add("quick-chat-route");
    document.body.classList.add("quick-chat-route");
    return () => {
      document.documentElement.classList.remove("quick-chat-route");
      document.body.classList.remove("quick-chat-route");
    };
  }, []);
  return (
    <I18nProvider language={settings.appSettings.interfaceLanguage}>
      <QuickChatContent settings={settings} />
    </I18nProvider>
  );
}
