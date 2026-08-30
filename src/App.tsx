import {
  Suspense,
  lazy,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { isTauri } from "@tauri-apps/api/core";
import { emitTo, listen } from "@tauri-apps/api/event";
import { LoaderCircle } from "lucide-react";
import "./styles/tokens.css";
import "./styles/app.css";
import "./styles/themes.css";
import type { ModelSelectorGroup } from "./features/chat/components/ModelSelector";
import { ChatWorkspace } from "./features/chat/components/ChatWorkspace";
import type { LocalSlashCommand, SlashCommandExecutionResult } from "./features/chat/commands/slashCommands";
import {
  buildInstallUsage,
  buildLocalCommandHelp,
  parseInstallTarget,
} from "./features/chat/commands/slashCommands";
import {
  pickAndInstallPet,
  pickAndInstallPlugin,
  pickAndInstallSkill,
} from "./features/settings/api/installFlows";
import type { RemotePackageKind } from "./features/settings/api/remotePackages";

// 只在真正要从 GitHub 安装时才加载：这条路径不常走，不该进主包。
const RemoteInstallDialog = lazy(() => import("./features/settings/components/RemoteInstallDialog"));
import { useChatRuntime } from "./features/chat/hooks/useChatRuntime";
import { estimateConversationContext } from "./features/chat/utils/contextUsage";
import { activeContextMessages, contextSummaryPrompt } from "./features/chat/utils/contextCompression";
import { Sidebar } from "./features/conversations/components/Sidebar";
import { useConversations } from "./features/conversations/hooks/useConversations";
import { exportStoredConversation } from "./features/conversations/api/conversations";
import { SettingsPage } from "./features/settings/components/SettingsPage";
import { useAppSettings } from "./features/settings/hooks/useAppSettings";
import { useSkills } from "./features/skills/hooks/useSkills";
import { useLibrary } from "./features/library/hooks/useLibrary";
import {
  DEFAULT_LAYOUT_PREFERENCES,
  LAYOUT_PANEL_LIMITS,
} from "./features/layout/hooks/useLayoutPreferences";
import type { AiPermissionMode } from "./types/chat";
import { resolveConversationModel } from "./types/modelSettings";
import { matchModelDefaults, resolveSupportsFunctionCalling, resolveSupportsReasoning, resolveSupportsVision } from "./data/modelMatching";
import type { ActiveWorkNoteContext, WorkLibraryView } from "./features/workspace/types";
import { findWorkspaceView } from "./features/workspace/viewRegistry";
import { ActivityBar } from "./features/workspace/components/ActivityBar";
import { WorkspaceViewHost } from "./features/workspace/components/WorkspaceViewHost";
import { ChatViewRuntimeProvider } from "./features/workspace/runtime/ChatViewRuntime";
import { NotesViewRuntimeProvider } from "./features/workspace/runtime/NotesViewRuntime";
import { WorkViewRuntimeProvider } from "./features/workspace/runtime/WorkViewRuntime";
import { OverviewViewRuntimeProvider } from "./features/workspace/runtime/OverviewViewRuntime";
import { DeepNoteViewRuntimeProvider } from "./features/workspace/runtime/DeepNoteViewRuntime";
import type { OverviewRecentItem } from "./features/overview/types";
import { I18nProvider } from "./i18n/I18nProvider";
import { ImageViewerProvider } from "./features/chat/image-viewer/ImageViewerContext";
import { useWorkspaceNavigation } from "./app/hooks/useWorkspaceNavigation";
import { useWorkspaceLayout } from "./app/hooks/useWorkspaceLayout";
import { useChatReferences } from "./app/hooks/useChatReferences";
import { useNoteActions } from "./app/hooks/useNoteActions";
import { NoteEditDialog } from "./features/chat/notePipeline/NoteEditDialog";
import { TaskCenter } from "./features/tasks/components/TaskCenter";
import { clearAttachmentPreviewCache } from "./features/chat/api/attachments";
import { releaseBackgroundResources } from "./runtime/resources/ResourceRegistry";
import { initializeWorkspaceLifecycle, subscribeWorkspaceLifecycle } from "./runtime/resources/WorkspaceLifecycle";
import { nextPetStateExpiry, projectPetState } from "./features/pet/petState";
import { speechController } from "./features/chat/speech/speechController";
function App() {
  const appShellRef = useRef<HTMLElement>(null);
  const navigation = useWorkspaceNavigation();
  const {
    activeView,
    workspaceMode,
    setWorkspaceMode,
    workLibraryView,
    workSearchQuery,
    setWorkSearchQuery,
    workCollectionId,
    setWorkCollectionId,
    workLibrarySort,
    setWorkLibrarySort,
    workContextPanelOpen,
    setWorkContextPanelOpen,
    workContextView,
    setWorkContextView,
    notesContextPanelOpen,
    setNotesContextPanelOpen,
    settingsCategory,
    setSettingsCategory,
    sidebarCollapsed,
    setSidebarCollapsed,
    navigateToWorkspace,
    changeWorkspaceMode,
    changeWorkLibraryView,
    changeWorkCollection,
    openSettings,
  } = navigation;
  const [modelMenuRequest, setModelMenuRequest] = useState(0);
  /** 远端安装对话框；null 表示未打开。 */
  const [remoteInstall, setRemoteInstall] = useState<{ kind: RemotePackageKind; query: string } | null>(null);
  const [remoteInstallResult, setRemoteInstallResult] = useState<string | null>(null);
  const [composerFocusRequest, setComposerFocusRequest] = useState(0);
  const [activeWorkNoteContext, setActiveWorkNoteContext] = useState<ActiveWorkNoteContext | null>(null);
  const requestComposerFocus = useCallback((delayMs = 0) => {
    window.setTimeout(() => {
      window.requestAnimationFrame(() => {
        setComposerFocusRequest((request) => request + 1);
      });
    }, delayMs);
  }, []);

  useEffect(() => {
    const disposeLifecycle = initializeWorkspaceLifecycle();
    const unsubscribe = subscribeWorkspaceLifecycle((state) => {
      if (state === "disposed") speechController.stop();
      if (state === "active") return;
      clearAttachmentPreviewCache();
      releaseBackgroundResources();
    });
    return () => {
      unsubscribe();
      disposeLifecycle();
    };
  }, []);

  const settings = useAppSettings();
  const skills = useSkills();
  const conversations = useConversations(navigateToWorkspace);
  const library = useLibrary({
    enabled: activeView === "workspace" && workspaceMode === "work",
    view: workLibraryView,
    searchQuery: workSearchQuery,
    collectionId: workCollectionId,
    sort: workLibrarySort,
  });
  const references = useChatReferences({
    conversations,
    workspaceMode,
    workContextPanelOpen,
    workContextView,
    setWorkspaceMode: changeWorkspaceMode,
    setWorkContextPanelOpen,
    setWorkContextView,
    setNotesContextPanelOpen,
    requestComposerFocus,
  });

  const currentModel = useMemo(() => {
    const conversation = conversations.currentConversation;
    return resolveConversationModel(
      settings.modelSettings,
      conversation?.providerId ?? null,
      conversation?.modelId ?? null,
    );
  }, [conversations.currentConversation, settings.modelSettings]);

  const noteActions = useNoteActions({
    currentConversation: conversations.currentConversation,
    currentConversationId: conversations.currentConversationId,
    modelSettings: settings.modelSettings,
    appSettings: settings.appSettings,
  });

  const latestAssistantMessage = useMemo(() => (
    [...(conversations.currentConversation?.messages ?? [])]
      .reverse()
      .find((message) => message.role === "assistant") ?? null
  ), [conversations.currentConversation?.messages]);
  const [petClock, setPetClock] = useState(Date.now());

  useEffect(() => {
    if (!settings.appSettings.pet.enabled || !settings.appSettings.pet.taskEvents) return undefined;
    const now = Date.now();
    setPetClock(now);
    const delay = nextPetStateExpiry(
      latestAssistantMessage,
      noteActions.deepNoteDetail,
      noteActions.deepNoteProgress,
      now,
    );
    if (delay === null) return undefined;
    const timer = window.setTimeout(() => setPetClock(Date.now()), delay + 25);
    return () => window.clearTimeout(timer);
  }, [
    latestAssistantMessage,
    noteActions.deepNoteDetail,
    noteActions.deepNoteProgress,
    settings.appSettings.pet.enabled,
    settings.appSettings.pet.taskEvents,
  ]);

  const petState = useMemo(() => (
    settings.appSettings.pet.taskEvents
      ? projectPetState(
          latestAssistantMessage,
          noteActions.deepNoteDetail,
          noteActions.deepNoteProgress,
          petClock,
        )
      : {
          state: "idle" as const,
          label: "陪你学习",
          detail: "任务状态跟随已关闭",
          updatedAt: petClock,
        }
  ), [
    latestAssistantMessage,
    noteActions.deepNoteDetail,
    noteActions.deepNoteProgress,
    petClock,
    settings.appSettings.pet.taskEvents,
  ]);

  useEffect(() => {
    if (!isTauri() || !settings.appSettings.pet.enabled) return undefined;
    const sendState = () => {
      void emitTo("pet", "mnemora://pet-state", petState).catch(() => undefined);
    };
    sendState();
    let unlisten: (() => void) | undefined;
    void listen("mnemora://pet-ready", sendState, { target: { kind: "WebviewWindow", label: "main" } })
      .then((dispose) => { unlisten = dispose; });
    return () => unlisten?.();
  }, [petState, settings.appSettings.pet.enabled]);

  useEffect(() => {
    if (noteActions.deepNoteActive || noteActions.deepNoteReview) {
      changeWorkspaceMode("deepNote");
    }
  }, [changeWorkspaceMode, noteActions.deepNoteActive, noteActions.deepNoteReview]);

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

  const deepNoteModelOptions = useMemo(() => settings.modelSettings.providers
    .filter((provider) => provider.enabled)
    .flatMap((provider) => provider.models
      .filter((model) => model.enabled)
      .map((model) => ({
        providerId: provider.id,
        providerName: provider.name,
        modelId: model.id,
        displayName: model.displayName,
        apiModel: model.apiModel,
        hasApiKey: provider.hasApiKey,
      }))), [settings.modelSettings.providers]);

  const handleDeepNoteModelSwitch = useCallback(async (providerId: string, modelId: string) => {
    const conversationId = noteActions.deepNoteDetail?.run.conversationId
      ?? conversations.currentConversationId;
    if (!conversationId) return;
    await settings.changeNoteModel(providerId, modelId);
    if (noteActions.deepNoteDetail?.run.id) {
      await noteActions.restartDeepNote();
    } else {
      await noteActions.startDeepNote(conversationId);
    }
  }, [conversations.currentConversationId, noteActions.deepNoteDetail?.run.conversationId, noteActions.deepNoteDetail?.run.id, noteActions.restartDeepNote, noteActions.startDeepNote, settings.changeNoteModel]);

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
    workspaceMode,
    skills: skills.skills,
    currentConversation: conversations.currentConversation,
    currentModel,
    conversationsRef: conversations.conversationsRef,
    requestInFlightRef: conversations.requestInFlightRef,
    cacheConversation: conversations.cacheConversation,
    saveStableConversation: conversations.saveStableConversation,
    protectConversation: conversations.protectConversation,
    releaseConversation: conversations.releaseConversation,
    activeWorkNoteContext,
  });

  const chatSurfaceVisible = activeView === "workspace" && (
    workspaceMode === "chat"
    || (workspaceMode === "work" && workContextPanelOpen && workContextView === "chat")
    || (workspaceMode === "notes" && notesContextPanelOpen)
  );

  useEffect(() => {
    if (chatSurfaceVisible) {
      void conversations.ensureCurrentConversationLoaded();
      return;
    }
    if (chatRuntime.requestInFlight) return;
    // 给视图切换留出短暂回退窗口；用户快速返回时不触发读盘和布局重建。
    const releaseTimer = window.setTimeout(() => {
      conversations.releaseCurrentConversation();
    }, 900);
    return () => window.clearTimeout(releaseTimer);
  }, [
    activeView,
    chatRuntime.requestInFlight,
    chatSurfaceVisible,
    conversations.currentConversationId,
    conversations.ensureCurrentConversationLoaded,
    conversations.releaseCurrentConversation,
  ]);

  const handleSlashCommand = useCallback(async (
    command: LocalSlashCommand,
    argumentsValue: string,
  ): Promise<SlashCommandExecutionResult> => {
    switch (command) {
      case "help":
        // 清单由命令表推导，新增命令自动出现在这里，不需要手动同步。
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
      case "settings":
        openSettings("general");
        return { executed: true };
      case "skills":
        openSettings("skills");
        return { executed: true };
      case "memory":
        openSettings("memory");
        return { executed: true };
      case "attach":
        return { executed: false, message: "附件命令由输入框处理。" };
      case "install": {
        const target = parseInstallTarget(argumentsValue);
        if (target.kind === null) {
          return { executed: false, message: buildInstallUsage(target) };
        }
        if (target.source === "github") {
          // 远端安装在对话框里自动推进到确认页，因此这里只负责打开它。
          setRemoteInstall({ kind: target.kind, query: target.query });
          return { executed: true };
        }
        const outcome = target.kind === "skill"
          ? await pickAndInstallSkill(target.mode)
          : target.kind === "plugin"
            ? await pickAndInstallPlugin(target.mode)
            : await pickAndInstallPet(target.mode);
        // Skill 与插件都可能带来新的 Slash 触发词，不刷新当前会话里用不到。
        if (outcome.ok && target.kind !== "pet") await skills.refresh();
        return { executed: outcome.ok, message: outcome.message };
      }
    }
  }, [chatRuntime, conversations, openSettings, skills]);

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
      reasoningEffort: (() => {
        const efforts = matchModelDefaults(model.apiModel)?.reasoningEfforts ?? [];
        return conversation.reasoningEffort && efforts.includes(conversation.reasoningEffort)
          ? conversation.reasoningEffort
          : null;
      })(),
      updatedAt: Date.now(),
    }));
  }, [conversations, settings.modelSettings.providers]);

  const handleConversationSkillsChange = useCallback((enabledSkillIds: string[]) => {
    if (conversations.requestInFlightRef.current) return;
    const available = new Set(
      skills.skills.filter((skill) => skill.enabled).map((skill) => skill.id),
    );
    conversations.updateCurrentConversation((conversation) => ({
      ...conversation,
      enabledSkillIds: [...new Set(enabledSkillIds.filter((id) => available.has(id)))].slice(0, 12),
      updatedAt: Date.now(),
    }));
  }, [conversations, skills.skills]);

  const layout = useWorkspaceLayout(appShellRef, workspaceMode, settings.appSettings);

  const workResourceLabel = {
    all: "全部文献",
    recent: "最近阅读",
    favorites: "收藏",
    unfiled: "未分类",
    notes: "笔记",
    trash: "回收站",
  } satisfies Record<WorkLibraryView, string>;
  const selectedWorkCollection = library.collections.find(
    (collection) => collection.id === workCollectionId,
  ) ?? null;
  const activeWorkResourceLabel = library.selectedItem?.title
    ?? selectedWorkCollection?.name
    ?? workResourceLabel[workLibraryView];

  const handleToggleWorkContextPanel = useCallback(() => {
    if (activeWorkNoteContext) {
      if (workContextPanelOpen && workContextView === "chat") {
        setWorkContextPanelOpen(false);
        return;
      }
      if (!conversations.currentConversation) {
        if (conversations.currentConversationId) {
          void conversations.ensureCurrentConversationLoaded();
        } else {
          conversations.createNewConversation();
        }
      }
      setWorkContextView("chat");
      setWorkContextPanelOpen(true);
      return;
    }
    setWorkContextPanelOpen((open) => !open);
  }, [
    activeWorkNoteContext,
    conversations,
    setWorkContextPanelOpen,
    setWorkContextView,
    workContextPanelOpen,
    workContextView,
  ]);

  const chatWorkspace = (
    <ChatWorkspace
      mode={workspaceMode}
      inputKey={conversations.currentConversation?.id ?? "no-conversation"}
      header={{
        title: conversations.currentConversation?.title ?? "未选择对话",
        permission: conversations.currentConversation?.permissionMode ?? "askSensitive",
        permissionDisabled: !conversations.currentConversation,
        theme: settings.resolvedTheme,
        onPermissionChange: handlePermissionChange,
        onToggleTheme: settings.toggleTheme,
        showTaskProgress: settings.appSettings.showChatTaskProgress,
        onToggleTaskProgress: (enabled) => {
          void settings.saveAppSettings({ ...settings.appSettings, showChatTaskProgress: enabled });
        },
      }}
      messages={{
        messages: conversations.currentConversation?.messages ?? [],
        conversationId: conversations.currentConversationId,
        hasConversation: conversations.currentConversation !== null,
        conversationLoading: conversations.currentConversationLoading,
        actionsDisabled: chatRuntime.requestInFlight,
        canRegenerate: Boolean(currentModel),
        suggestionsDisabled: !currentModel || chatRuntime.requestInFlight,
        onCreateConversation: conversations.createNewConversation,
        onSuggestionSelect: chatRuntime.sendMessage,
        onEditMessage: chatRuntime.editMessage,
        onRegenerateMessage: chatRuntime.regenerateMessage,
        onDeleteMessage: chatRuntime.deleteMessage,
        onQuoteMessage: references.appendQuote,
        onSaveMessageAsNote: noteActions.saveMessage,
        onLiteratureReferenceOpen: references.openLiteratureReference,
      }}
      input={{
        conversationId: conversations.currentConversationId,
        busy: chatRuntime.requestInFlight,
        stopDisabled: chatRuntime.stopRequested,
        disabled: !conversations.currentConversation || !currentModel,
        placeholder: !conversations.currentConversation
          ? "请先新建对话"
          : !currentModel
            ? "请先配置默认模型"
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
        thinkingEnabled: conversations.currentConversation?.thinkingEnabled ?? settings.appSettings.thinkingEnabled,
        reasoningEffort: conversations.currentConversation?.reasoningEffort ?? null,
        modelLabel: currentModel ? `${currentModel.provider.name} · ${currentModel.model.displayName}` : "配置模型",
        modelTitle: currentModel ? `${currentModel.provider.name} / ${currentModel.model.apiModel}` : "模型设置",
        modelConfigured: Boolean(currentModel),
        modelGroups,
        selectedProviderId: currentModel?.provider.id ?? null,
        selectedModelId: currentModel?.model.id ?? null,
        modelMenuRequest,
        modelSelectionDisabled: !conversations.currentConversation || chatRuntime.requestInFlight,
        onModelChange: handleModelChange,
        onThinkingChange: (enabled) => conversations.updateCurrentConversation((conversation) => ({ ...conversation, thinkingEnabled: enabled, updatedAt: Date.now() })),
        onReasoningEffortChange: (effort) => conversations.updateCurrentConversation((conversation) => ({ ...conversation, reasoningEffort: effort, updatedAt: Date.now() })),
        hasMessages: (conversations.currentConversation?.messages.length ?? 0) > 0,
        onSaveConversationAsNote: conversations.currentConversationId ? () => noteActions.saveConversationAsNote(conversations.currentConversationId!) : undefined,
        onSummarizeConversationToNote: conversations.currentConversationId ? () => { void noteActions.summarizeConversationAsNote(conversations.currentConversationId!); } : undefined,
        onGenerateDeepNote: conversations.currentConversationId ? () => { void noteActions.startDeepNote(conversations.currentConversationId!); } : undefined,
        onUpdateExistingNote: conversations.currentConversationId ? () => { void noteActions.openConversationNoteEdit(conversations.currentConversationId!); } : undefined,
        onExportConversation: conversations.currentConversationId ? (format) => { void exportStoredConversation(conversations.currentConversationId!, conversations.currentConversation?.title ?? "Mnemora 会话", format); } : undefined,
        supportsVision: currentModel
          ? resolveSupportsVision(
              currentModel.model.apiModel,
              currentModel.model.capabilities?.vision,
            ) ?? null
          : null,
        supportsTools: currentModel
          ? resolveSupportsFunctionCalling(
              currentModel.model.apiModel,
              currentModel.model.capabilities?.functionCalling,
            )
          : null,
        showLiteraturePicker: workspaceMode === "work",
        quotes: references.quotes,
        onQuoteRemove: references.removeQuote,
        onQuotesClear: references.clearQuotes,
        literatureReferences: references.literatureReferences,
        onLiteratureReferenceRemove: references.removeLiteratureReference,
        onLiteratureReferencesClear: references.clearLiteratureReferences,
        noteReferences: references.noteReferences,
        onNoteReferenceRemove: references.removeNoteReference,
        onNoteReferencesClear: references.clearNoteReferences,
        contextMessageCount: conversations.currentConversation?.messages.length ?? 0,
        contextCompressionCount: conversations.currentConversation?.contextCompressionCount ?? 0,
        contextDisabled: !conversations.currentConversation || !currentModel,
        contextMessages: conversations.currentConversation?.messages ?? [],
        contextSystemPrompt: settings.appSettings.systemPrompt,
        skills: skills.skills,
        selectedSkillIds: conversations.currentConversation?.enabledSkillIds ?? [],
        onSelectedSkillsChange: handleConversationSkillsChange,
        onSend: chatRuntime.sendMessage,
        onStop: settings.appSettings.streamEnabled ? chatRuntime.stopGeneration : undefined,
        onSlashCommand: handleSlashCommand,
      }}
    />
  );

  const notesViewRuntime = {
    workspace: {
            chatOpen: notesContextPanelOpen,
            chatBusy: chatRuntime.requestInFlight,
            userDisplayName: settings.appSettings.userDisplayName,
            onBack: () => {
              setNotesContextPanelOpen(false);
              changeWorkspaceMode("chat");
            },
            onToggleChat: () => {
              if (!conversations.currentConversation) {
                if (conversations.currentConversationId) {
                  void conversations.ensureCurrentConversationLoaded();
                } else {
                  conversations.createNewConversation();
                }
              }
              setNotesContextPanelOpen((open) => !open);
            },
            onAskSelection: references.addNoteReference,
            onEditSelection: noteActions.openSelectionNoteEdit,
            onGenerateFromLocalFiles: async (paths: string[]) => {
              if (await noteActions.startLocalFilesDeepNote(paths)) changeWorkspaceMode("deepNote");
            },
            onOpenSourceConversation: (conversationId: string) => {
              conversations.selectConversation(conversationId);
              changeWorkspaceMode("chat");
            },
          },
    contextPanel: notesContextPanelOpen
            ? {
                chatPanel: chatWorkspace,
                onClose: () => setNotesContextPanelOpen(false),
                resize: {
                  value: layout.preferences.notesContextWidth,
                  defaultValue: DEFAULT_LAYOUT_PREFERENCES.notesContextWidth,
                  minValue: LAYOUT_PANEL_LIMITS.notesContext.min,
                  maxValue: LAYOUT_PANEL_LIMITS.notesContext.max,
                  getMaxValue: layout.getContextMaxWidth,
                  onPreview: layout.previewNotesContextWidth,
                  onCommit: layout.commitNotesContextWidth,
                },
              }
            : null,
  };
  const workViewRuntime = {
    workspace: {
            libraryView: workLibraryView,
            searchQuery: workSearchQuery,
            collectionName: selectedWorkCollection?.name ?? null,
            items: library.items,
            collections: library.collections,
            total: library.total,
            loading: library.loading,
            error: library.error,
            notice: library.notice,
            actionPending: library.actionPending,
            selectedItem: library.selectedItem,
            selectedItemLoading: library.selectedItemLoading,
            selectionError: library.selectionError,
            sort: workLibrarySort,
            contextPanelOpen: workContextPanelOpen,
            noteChatOpen: workContextPanelOpen && workContextView === "chat",
            chatBusy: chatRuntime.requestInFlight,
            literatureNavigationRequest: references.literatureNavigationRequest,
            noteRefreshVersion: noteActions.noteEditRefresh?.noteId === activeWorkNoteContext?.noteId
              ? (noteActions.noteEditRefresh?.version ?? 0)
              : 0,
            onToggleContextPanel: handleToggleWorkContextPanel,
            onAskSelection: references.addLiteratureReference,
            onAskNoteSelection: references.addNoteReference,
            onEditNoteSelection: noteActions.openSelectionNoteEdit,
            onActiveNoteContextChange: setActiveWorkNoteContext,
            onPdfDocumentsChange: references.setWorkPdfDocuments,
            onLiteratureNavigationHandled: references.handleLiteratureNavigationHandled,
            onImport: library.importPdfs,
            onRefresh: library.refresh,
            onDismissNotice: library.clearNotice,
            onSortChange: setWorkLibrarySort,
            onSelectItem: library.selectItem,
            onMarkOpened: library.markOpened,
            onOpenExternal: library.openExternal,
            onSetFavorite: library.setFavorite,
            onSaveItem: library.saveItem,
            onMoveToTrash: library.moveToTrash,
            onRestoreItem: library.restoreItem,
            onDeletePermanently: library.deletePermanently,
          },
    contextPanel: workContextPanelOpen
            ? {
                activeView: workContextView,
                resourceLabel: activeWorkResourceLabel,
                resourceCount: library.total,
                searchQuery: workSearchQuery,
                chatBusy: chatRuntime.requestInFlight,
                chatPanel: chatWorkspace,
                pdfDocuments: references.workPdfDocuments,
                linkedLibraryItemIds: conversations.currentConversation?.linkedLibraryItemIds ?? [],
                literatureReferenceError: references.literatureReferenceError,
                conversationAvailable: conversations.currentConversation !== null,
                conversations: conversations.conversationListItems,
                currentConversationId: conversations.currentConversationId,
                activeNoteContext: activeWorkNoteContext,
                libraryItem: library.selectedItem,
                collections: library.collections,
                itemSaving: library.actionPending,
                onViewChange: setWorkContextView,
                onClose: () => setWorkContextPanelOpen(false),
                onLinkedLibraryItemIdsChange: references.updateLinkedLibraryItemIds,
                onAddLiteratureReference: references.addLiteratureReference,
                onClearLiteratureReferenceError: references.clearLiteratureReferenceError,
                onConversationChange: conversations.selectConversation,
                onCreateConversation: conversations.createNewConversation,
                onSaveLibraryItem: library.saveItem,
                resize: {
                  value: layout.preferences.workContextWidth,
                  defaultValue: DEFAULT_LAYOUT_PREFERENCES.workContextWidth,
                  minValue: LAYOUT_PANEL_LIMITS.workContext.min,
                  maxValue: LAYOUT_PANEL_LIMITS.workContext.max,
                  getMaxValue: layout.getContextMaxWidth,
                  onPreview: layout.previewWorkContextWidth,
                  onCommit: layout.commitWorkContextWidth,
                },
              }
            : null,
  };

  const overviewViewRuntime = {
    onNewChat: () => {
      conversations.createNewConversation();
      changeWorkspaceMode("chat");
    },
    onOpenNotes: () => changeWorkspaceMode("notes"),
    onOpenWork: () => changeWorkspaceMode("work"),
    onOpenItem: (item: OverviewRecentItem) => {
      if (item.destination === "chat") conversations.selectConversation(item.id);
      changeWorkspaceMode(item.destination);
    },
  };

  return (
    <I18nProvider language={settings.appSettings.interfaceLanguage}>
    <main
      ref={appShellRef}
      className="app-shell"
      data-theme={settings.resolvedTheme}
      data-theme-preset={settings.appSettings.themePreset}
      data-theme-color={settings.appSettings.themeColor}
      data-active-workspace={activeView === "settings" ? "settings" : workspaceMode}
      data-custom-background={layout.hasCustomBackground ? "true" : "false"}
      style={layout.appThemeStyle}
      aria-label="Mnemora application"
    >
      <ImageViewerProvider>
      <ActivityBar
        activeView={workspaceMode}
        settingsOpen={activeView === "settings"}
        onSelectView={changeWorkspaceMode}
        onOpenSettings={() => openSettings("general")}
      />
      <TaskCenter
        chatMessage={latestAssistantMessage}
        chatConversationId={conversations.currentConversationId}
        chatConversationLoaded={conversations.currentConversation !== null && !conversations.currentConversationLoading}
        showChatTask={settings.appSettings.showChatTaskProgress}
        deepNoteDetail={noteActions.deepNoteDetail}
        deepNoteProgress={noteActions.deepNoteProgress}
        deepNoteReviewTitle={noteActions.deepNoteReview?.outline.title}
        deepNoteControlBusy={noteActions.deepNoteControlBusy}
        onStopChatTask={chatRuntime.stopGeneration}
        onOpenDeepNoteTask={() => changeWorkspaceMode("deepNote")}
        onPauseDeepNoteTask={() => { void noteActions.pauseDeepNote(); }}
        onResumeDeepNoteTask={() => { void noteActions.resumeDeepNote(); }}
        onRetryDeepNoteTask={() => { void noteActions.retryDeepNote(); }}
        onRestartDeepNoteTask={() => { void noteActions.restartDeepNote(); }}
        onStopDeepNoteTask={() => { void noteActions.cancelDeepNote(); }}
        onAbandonDeepNoteTask={() => { void noteActions.abandonDeepNote(); }}
        onOpenChatTask={(messageId) => {
          changeWorkspaceMode("chat");
          requestAnimationFrame(() => {
            document.getElementById(`message-${messageId}`)?.scrollIntoView({
              block: "center",
              behavior: "smooth",
            });
          });
        }}
      />
      {/* 上下文侧栏由视图清单决定：笔记视图自带左栏，不渲染共享 Sidebar。 */}
      {activeView === "workspace" && findWorkspaceView(workspaceMode)?.contextSidebar !== false ? <Sidebar
        mode={workspaceMode}
        workLibraryView={workLibraryView}
        workSearchQuery={workSearchQuery}
        workCollections={library.collections}
        workSelectedCollectionId={workCollectionId}
        workLibraryBusy={library.actionPending}
        workLibraryRuntimeAvailable={library.runtimeAvailable}
        collapsed={sidebarCollapsed}
        userDisplayName={settings.appSettings.userDisplayName}
        userAvatar={settings.appSettings.userAvatar}
        conversations={conversations.conversationListItems}
        conversationListLoading={conversations.conversationListLoading}
        conversationListError={conversations.conversationListError}
        conversationListHasMore={conversations.conversationListHasMore}
        currentConversationId={conversations.currentConversationId}
        onCreateConversation={() => {
          conversations.createNewConversation();
          setWorkspaceMode("chat");
        }}
        onSelectConversation={(conversationId) => {
          conversations.selectConversation(conversationId);
          setWorkspaceMode("chat");
        }}
        onDeleteConversation={(conversationId) => {
          if (!window.confirm("确定删除这个对话吗？未完成的深度笔记任务将被遗弃终止且不能恢复；已生成笔记不会删除，但部分来源跳转将失效。")) return;
          conversations.deleteConversation(conversationId);
        }}
        onRenameConversation={conversations.renameConversation}
        onExportConversation={(conversationId, format) => {
          const item = conversations.conversationListItems.find((conversation) => conversation.id === conversationId);
          void exportStoredConversation(conversationId, item?.title ?? "Mnemora 会话", format)
            .catch((error) => {
              const message = error instanceof Error ? error.message : String(error);
              window.alert(`导出失败：${message}`);
            });
        }}
        onSaveConversationAsNote={noteActions.saveConversationAsNote}
        onSummarizeConversationToNote={(conversationId) => {
          void noteActions.summarizeConversationAsNote(conversationId);
        }}
        onGenerateDeepNote={(conversationId) => {
          void noteActions.startDeepNote(conversationId);
        }}
        onUpdateExistingNote={(conversationId) => {
          void noteActions.openConversationNoteEdit(conversationId);
        }}
        onClearConversations={() => {
          if (!window.confirm("确定清空全部对话吗？相关的未完成深度笔记任务将被遗弃终止且不能恢复；已生成笔记不会删除，但对话来源跳转将全部失效。")) return;
          conversations.clearConversations();
        }}
        onLoadMoreConversations={conversations.loadMoreConversations}
        onOpenSkills={() => openSettings("skills")}
        onOpenPlugins={() => openSettings("plugins")}
        onWorkLibraryViewChange={changeWorkLibraryView}
        onWorkSearchQueryChange={setWorkSearchQuery}
        onWorkCollectionSelect={changeWorkCollection}
        onWorkImport={library.importPdfs}
        onWorkCreateCollection={library.createCollection}
        onWorkRenameCollection={library.renameCollection}
        onWorkDeleteCollection={async (collectionId) => {
          const removed = await library.removeCollection(collectionId);
          if (removed && workCollectionId === collectionId) setWorkCollectionId(null);
          return removed;
        }}
        onToggleCollapse={() => setSidebarCollapsed((collapsed) => !collapsed)}
        resize={{
          value: layout.sidebarWidth,
          defaultValue: layout.sidebarDefaultWidth,
          minValue: LAYOUT_PANEL_LIMITS.chatSidebar.min,
          maxValue: LAYOUT_PANEL_LIMITS.chatSidebar.max,
          getMaxValue: layout.getSidebarMaxWidth,
          onPreview: layout.previewSidebarWidth,
          onCommit: layout.commitSidebarWidth,
        }}
      /> : null}

      {activeView === "settings" ? (
        <SettingsPage
          settings={settings.modelSettings}
          appSettings={settings.appSettings}
          activeCategory={settingsCategory}
          skillState={skills}
          initialError={settings.modelSettingsError}
          appSettingsError={settings.appSettingsError}
          onBack={navigateToWorkspace}
          onCategoryChange={setSettingsCategory}
          onSave={settings.saveModelSettings}
          onPreviewAppSettings={settings.previewAppSettings}
          onSaveAppSettings={settings.saveAppSettings}
          onSettingsImported={settings.applyImportedSettings}
          onDefaultModelChange={settings.changeDefaultModel}
          onNoteModelChange={settings.changeNoteModel}
        />
      ) : (
        <ChatViewRuntimeProvider chatPanel={chatWorkspace}>
          <NotesViewRuntimeProvider value={notesViewRuntime}>
            <WorkViewRuntimeProvider value={workViewRuntime}>
              <OverviewViewRuntimeProvider value={overviewViewRuntime}>
                <DeepNoteViewRuntimeProvider value={{
                  detail: noteActions.deepNoteDetail,
                  review: noteActions.deepNoteReview,
                  progress: noteActions.deepNoteProgress,
                  busy: noteActions.deepNoteReviewBusy,
                  controlBusy: noteActions.deepNoteControlBusy,
                  modelOptions: deepNoteModelOptions,
                  onAdjust: (requirement) => {
                    void noteActions.adjustDeepNoteOutline(requirement);
                  },
                  onConfirm: (selectedSectionIds) => {
                    void noteActions.confirmDeepNoteOutline(selectedSectionIds);
                  },
                  onPause: () => {
                    void noteActions.pauseDeepNote();
                  },
                  onResume: () => {
                    void noteActions.resumeDeepNote();
                  },
                  onRetry: () => {
                    void noteActions.retryDeepNote();
                  },
                  onRestart: () => {
                    void noteActions.restartDeepNote();
                  },
                  onCancel: noteActions.cancelDeepNote,
                  onAbandon: noteActions.abandonDeepNote,
                  onSwitchModel: handleDeepNoteModelSwitch,
                  onOpenNote: () => changeWorkspaceMode("notes"),
                  onReturn: () => changeWorkspaceMode("chat"),
                }}>
                  <WorkspaceViewHost
                    mode={workspaceMode}
                    contextOpen={workspaceMode === "work"
                      ? workContextPanelOpen
                      : workspaceMode === "notes"
                        ? notesContextPanelOpen
                        : false}
                    onReturnToChat={() => changeWorkspaceMode("chat")}
                  />
                </DeepNoteViewRuntimeProvider>
              </OverviewViewRuntimeProvider>
            </WorkViewRuntimeProvider>
          </NotesViewRuntimeProvider>
        </ChatViewRuntimeProvider>
      )}
      </ImageViewerProvider>


      {noteActions.noteEditRequest || noteActions.noteEditResult ? (
        <NoteEditDialog
          request={noteActions.noteEditRequest}
          result={noteActions.noteEditResult}
          busy={noteActions.noteEditBusy}
          onClose={() => void noteActions.closeNoteEdit()}
          onPrepare={(noteId, requirement) => void noteActions.prepareExistingNoteEdit(noteId, requirement)}
          onApply={(selection) => void noteActions.applyNoteEdit(selection)}
        />
      ) : null}

      {noteActions.feedback ? (
        <div
          className={`app-toast app-toast-${noteActions.feedback.kind}`}
          role="status"
          aria-live="polite"
        >
          {noteActions.feedback.kind === "progress" ? (
            <LoaderCircle size={15} className="app-toast-spinner" />
          ) : null}
          <span>{noteActions.feedback.text}</span>
          {noteActions.deepNoteDetail ? (
            <button
              className="app-toast-cancel"
              type="button"
              onClick={() => changeWorkspaceMode("deepNote")}
            >
              查看详情
            </button>
          ) : null}
          {noteActions.feedback.kind === "progress" && noteActions.deepNoteActive ? (
            <button className="app-toast-cancel" type="button" onClick={noteActions.cancelDeepNote}>取消</button>
          ) : null}
        </div>
      ) : null}

      {remoteInstall ? (
        <Suspense fallback={null}>
          <RemoteInstallDialog
            kind={remoteInstall.kind}
            initialQuery={remoteInstall.query}
            onClose={() => setRemoteInstall(null)}
            onInstalled={(message) => {
              setRemoteInstallResult(message);
              // 远端装进来的 Skill / 插件同样可能带新触发词。
              if (remoteInstall.kind !== "pet") void skills.refresh();
            }}
          />
        </Suspense>
      ) : null}

      {remoteInstallResult ? (
        <div className="app-toast" role="status" aria-live="polite">
          <span>{remoteInstallResult}</span>
          <button className="app-toast-cancel" type="button" onClick={() => setRemoteInstallResult(null)}>
            知道了
          </button>
        </div>
      ) : null}
    </main>
    </I18nProvider>
  );
}

export default App;
