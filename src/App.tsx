import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import "./styles/app.css";
import "./styles/themes.css";
import { ChatHeader, type ModelSelectorGroup } from "./features/chat/components/ChatHeader";
import { ChatInput } from "./features/chat/components/ChatInput";
import type { LocalSlashCommand, SlashCommandExecutionResult } from "./features/chat/commands/slashCommands";
import { MessageList } from "./features/chat/components/MessageList";
import { useChatRuntime } from "./features/chat/hooks/useChatRuntime";
import { estimateConversationContext } from "./features/chat/utils/contextUsage";
import { activeContextMessages, contextSummaryPrompt } from "./features/chat/utils/contextCompression";
import { Sidebar } from "./features/conversations/components/Sidebar";
import { useConversations } from "./features/conversations/hooks/useConversations";
import { SettingsPage, type SettingsCategory } from "./features/settings/components/SettingsPage";
import { useAppSettings } from "./features/settings/hooks/useAppSettings";
import { resolveThemeBackgroundCss } from "./features/settings/utils/themeBackground";
import { useSkills } from "./features/skills/hooks/useSkills";
import { PdfReaderBridgeProvider } from "./features/pdf/context/PdfReaderContext";
import { useLibrary } from "./features/library/hooks/useLibrary";
import type { LibrarySort } from "./features/library/types";
import {
  DEFAULT_LAYOUT_PREFERENCES,
  LAYOUT_PANEL_LIMITS,
  useLayoutPreferences,
} from "./features/layout/hooks/useLayoutPreferences";
import type { AiPermissionMode } from "./types/chat";
import { resolveDefaultModel } from "./types/modelSettings";
import { resolveSupportsVision } from "./data/modelMatching";
import type {
  WorkContextView,
  WorkLibraryView,
  WorkspaceMode,
} from "./features/workspace/types";

const WorkWorkspace = lazy(() => import("./features/workspace/components/WorkWorkspace"));
const WorkContextPanel = lazy(() => import("./features/workspace/components/WorkContextPanel").then(
  (module) => ({ default: module.WorkContextPanel }),
));

type AppView = "workspace" | "settings";

const CHAT_WORKSPACE_MIN_WIDTH = 420;
const WORK_MAIN_MIN_WIDTH = 520;

function App() {
  const appShellRef = useRef<HTMLElement>(null);
  const [activeView, setActiveView] = useState<AppView>("workspace");
  const [workspaceMode, setWorkspaceMode] = useState<WorkspaceMode>("chat");
  const [workLibraryView, setWorkLibraryView] = useState<WorkLibraryView>("all");
  const [workSearchQuery, setWorkSearchQuery] = useState("");
  const [workCollectionId, setWorkCollectionId] = useState<string | null>(null);
  const [workLibrarySort, setWorkLibrarySort] = useState<LibrarySort>("updated");
  // Work 以文献阅读为主，右侧工具栏按需打开；Chat 不默认占用阅读空间。
  const [workContextPanelOpen, setWorkContextPanelOpen] = useState(false);
  const [workContextView, setWorkContextView] = useState<WorkContextView>("info");
  const [settingsCategory, setSettingsCategory] = useState<SettingsCategory>("general");
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [modelMenuRequest, setModelMenuRequest] = useState(0);
  const { preferences: layoutPreferences, savePanelWidth } = useLayoutPreferences();
  const navigateToWorkspace = useCallback(() => setActiveView("workspace"), []);
  const changeWorkspaceMode = useCallback((mode: WorkspaceMode) => {
    setWorkspaceMode(mode);
    setActiveView("workspace");
  }, []);
  const changeWorkLibraryView = useCallback((view: WorkLibraryView) => {
    setWorkLibraryView(view);
    setWorkCollectionId(null);
    setActiveView("workspace");
  }, []);
  const changeWorkCollection = useCallback((collectionId: string) => {
    setWorkCollectionId(collectionId);
    setWorkLibraryView("all");
    setActiveView("workspace");
  }, []);
  const openSettings = useCallback((category: SettingsCategory = "general") => {
    setSettingsCategory(category);
    setActiveView("settings");
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
  // 选中助手回答片段后的引用状态；切换会话即失效。
  const [quotedText, setQuotedText] = useState<string | null>(null);
  useEffect(() => {
    setQuotedText(null);
  }, [conversations.currentConversationId]);

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
    skills: skills.skills,
    currentConversation: conversations.currentConversation,
    currentModel,
    conversationsRef: conversations.conversationsRef,
    requestInFlightRef: conversations.requestInFlightRef,
    cacheConversation: conversations.cacheConversation,
    saveStableConversation: conversations.saveStableConversation,
    protectConversation: conversations.protectConversation,
    releaseConversation: conversations.releaseConversation,
  });

  const handleSlashCommand = useCallback(async (
    command: LocalSlashCommand,
    argumentsValue: string,
  ): Promise<SlashCommandExecutionResult> => {
    switch (command) {
      case "help":
        return {
          executed: true,
          message: "可用命令：/new、/clear、/compact [重点]、/model、/settings、/skills、/memory、/attach。",
        };
      case "new":
        conversations.createNewConversation();
        return { executed: true };
      case "clear": {
        if (!window.confirm("确定永久删除当前对话及其附件吗？此操作无法撤销。")) {
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
    }
  }, [chatRuntime, conversations, openSettings]);

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

  const handleConversationSkillsChange = useCallback((enabledSkillIds: string[]) => {
    if (conversations.requestInFlightRef.current) return;
    const available = new Set(
      skills.skills.filter((skill) => skill.enabled).map((skill) => skill.id),
    );
    conversations.updateCurrentConversation((conversation) => ({
      ...conversation,
      enabledSkillIds: [...new Set(enabledSkillIds.filter((id) => available.has(id)))].slice(0, 3),
      updatedAt: Date.now(),
    }));
  }, [conversations, skills.skills]);

  const sidebarWidth = workspaceMode === "chat"
    ? layoutPreferences.chatSidebarWidth
    : layoutPreferences.workSidebarWidth;
  const sidebarDefaultWidth = workspaceMode === "chat"
    ? DEFAULT_LAYOUT_PREFERENCES.chatSidebarWidth
    : DEFAULT_LAYOUT_PREFERENCES.workSidebarWidth;

  const previewSidebarWidth = useCallback((width: number) => {
    appShellRef.current?.style.setProperty("--sidebar-width", `${Math.round(width)}px`);
  }, []);
  const commitSidebarWidth = useCallback((width: number) => {
    savePanelWidth(workspaceMode === "chat" ? "chatSidebar" : "workSidebar", width);
  }, [savePanelWidth, workspaceMode]);
  const getSidebarMaxWidth = useCallback(() => (
    Math.max(
      LAYOUT_PANEL_LIMITS.chatSidebar.min,
      Math.min(
        LAYOUT_PANEL_LIMITS.chatSidebar.max,
        window.innerWidth - CHAT_WORKSPACE_MIN_WIDTH,
      ),
    )
  ), []);

  const previewWorkContextWidth = useCallback((width: number) => {
    appShellRef.current?.style.setProperty("--work-context-width", `${Math.round(width)}px`);
  }, []);
  const commitWorkContextWidth = useCallback((width: number) => {
    savePanelWidth("workContext", width);
  }, [savePanelWidth]);
  const getWorkContextMaxWidth = useCallback((handle: HTMLButtonElement) => {
    const stage = handle.closest<HTMLElement>(".workspace-stage");
    const availableWidth = stage?.getBoundingClientRect().width ?? window.innerWidth;
    return Math.max(
      LAYOUT_PANEL_LIMITS.workContext.min,
      Math.min(
        LAYOUT_PANEL_LIMITS.workContext.max,
        availableWidth - WORK_MAIN_MIN_WIDTH,
      ),
    );
  }, []);

  const customBackground = resolveThemeBackgroundCss(settings.appSettings.themeBackground);
  const appThemeStyle = {
    "--app-font-size": `${settings.appSettings.fontSize}px`,
    "--app-custom-background": customBackground ?? "var(--color-app)",
    "--app-surface-opacity": `${customBackground
      ? settings.appSettings.themeBackground.surfaceOpacity
      : 100}%`,
    "--sidebar-width": `${sidebarWidth}px`,
    "--work-context-width": `${layoutPreferences.workContextWidth}px`,
  } as CSSProperties;

  const workResourceLabel = {
    all: "全部文献",
    recent: "最近阅读",
    favorites: "收藏",
    unfiled: "未分类",
    notes: "笔记",
    "mind-maps": "思维导图",
    trash: "回收站",
  } satisfies Record<WorkLibraryView, string>;
  const selectedWorkCollection = library.collections.find(
    (collection) => collection.id === workCollectionId,
  ) ?? null;
  const activeWorkResourceLabel = library.selectedItem?.title
    ?? selectedWorkCollection?.name
    ?? workResourceLabel[workLibraryView];

  const chatWorkspace = (
    <section
      className={`chat-workspace${workspaceMode === "work" ? " chat-workspace-panel" : ""}`}
      aria-label={workspaceMode === "work" ? "文献 AI 对话" : "当前对话"}
      key="shared-chat-workspace"
    >
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
        modelMenuRequest={modelMenuRequest}
        permission={conversations.currentConversation?.permissionMode ?? "askSensitive"}
        permissionDisabled={!conversations.currentConversation}
        theme={settings.resolvedTheme}
        compact={workspaceMode === "work"}
        onModelChange={handleModelChange}
        onPermissionChange={handlePermissionChange}
        onToggleTheme={settings.toggleTheme}
      />
      <MessageList
        messages={conversations.currentConversation?.messages ?? []}
        conversationId={conversations.currentConversationId}
        hasConversation={conversations.currentConversation !== null}
        actionsDisabled={chatRuntime.requestInFlight}
        canRegenerate={Boolean(currentModel)}
        suggestionsDisabled={!currentModel || chatRuntime.requestInFlight}
        onCreateConversation={conversations.createNewConversation}
        onSuggestionSelect={chatRuntime.sendMessage}
        onEditMessage={chatRuntime.editMessage}
        onRegenerateMessage={chatRuntime.regenerateMessage}
        onDeleteMessage={chatRuntime.deleteMessage}
        onQuoteMessage={setQuotedText}
      />
      <ChatInput
        conversationId={conversations.currentConversationId}
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
        supportsVision={currentModel
          ? resolveSupportsVision(
              currentModel.model.apiModel,
              currentModel.model.capabilities?.vision,
            ) ?? null
          : null}
        quote={quotedText}
        onQuoteClear={() => setQuotedText(null)}
        contextMessageCount={conversations.currentConversation?.messages.length ?? 0}
        contextCompressionCount={conversations.currentConversation?.contextCompressionCount ?? 0}
        contextDisabled={!conversations.currentConversation || !currentModel}
        contextMessages={conversations.currentConversation?.messages ?? []}
        contextSystemPrompt={settings.appSettings.systemPrompt}
        skills={skills.skills}
        selectedSkillIds={conversations.currentConversation?.enabledSkillIds ?? []}
        onSelectedSkillsChange={handleConversationSkillsChange}
        onSend={chatRuntime.sendMessage}
        onStop={settings.appSettings.streamEnabled ? chatRuntime.stopGeneration : undefined}
        onSlashCommand={handleSlashCommand}
      />
    </section>
  );

  return (
    <main
      ref={appShellRef}
      className="app-shell"
      data-theme={settings.resolvedTheme}
      data-theme-preset={settings.appSettings.themePreset}
      data-theme-color={settings.appSettings.themeColor}
      data-custom-background={customBackground ? "true" : "false"}
      style={appThemeStyle}
      aria-label="Mnemora application"
    >
      <Sidebar
        mode={workspaceMode}
        workLibraryView={workLibraryView}
        workSearchQuery={workSearchQuery}
        workCollections={library.collections}
        workSelectedCollectionId={workCollectionId}
        workLibraryBusy={library.actionPending}
        workLibraryRuntimeAvailable={library.runtimeAvailable}
        collapsed={sidebarCollapsed}
        settingsOpen={activeView === "settings"}
        skillsOpen={activeView === "settings" && settingsCategory === "skills"}
        userDisplayName={settings.appSettings.userDisplayName}
        userAvatar={settings.appSettings.userAvatar}
        conversations={conversations.conversationListItems}
        conversationListLoading={conversations.conversationListLoading}
        conversationListError={conversations.conversationListError}
        conversationListHasMore={conversations.conversationListHasMore}
        currentConversationId={conversations.currentConversationId}
        onCreateConversation={conversations.createNewConversation}
        onSelectConversation={conversations.selectConversation}
        onDeleteConversation={conversations.deleteConversation}
        onClearConversations={conversations.clearConversations}
        onLoadMoreConversations={conversations.loadMoreConversations}
        onOpenSettings={() => openSettings("general")}
        onOpenSkills={() => openSettings("skills")}
        onModeChange={changeWorkspaceMode}
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
          value: sidebarWidth,
          defaultValue: sidebarDefaultWidth,
          minValue: LAYOUT_PANEL_LIMITS.chatSidebar.min,
          maxValue: LAYOUT_PANEL_LIMITS.chatSidebar.max,
          getMaxValue: getSidebarMaxWidth,
          onPreview: previewSidebarWidth,
          onCommit: commitSidebarWidth,
        }}
      />

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
        />
      ) : (
        <section
          className="workspace-stage"
          data-workspace-mode={workspaceMode}
          data-context-open={workContextPanelOpen ? "true" : "false"}
        >
          {/* Work 条件挂载，后续 PDF 阅读器在这里随模式切换卸载并释放资源。 */}
          {workspaceMode === "work" ? (
            <PdfReaderBridgeProvider>
              <Suspense fallback={<div className="workspace-loading" role="status">正在打开 Work</div>}>
                <WorkWorkspace
                  libraryView={workLibraryView}
                  searchQuery={workSearchQuery}
                  collectionName={selectedWorkCollection?.name ?? null}
                  items={library.items}
                  total={library.total}
                  loading={library.loading}
                  error={library.error}
                  notice={library.notice}
                  actionPending={library.actionPending}
                  selectedItem={library.selectedItem}
                  selectedItemLoading={library.selectedItemLoading}
                  selectionError={library.selectionError}
                  sort={workLibrarySort}
                  contextPanelOpen={workContextPanelOpen}
                  chatBusy={chatRuntime.requestInFlight}
                  onToggleContextPanel={() => setWorkContextPanelOpen((open) => !open)}
                  onImport={library.importPdfs}
                  onRefresh={library.refresh}
                  onDismissNotice={library.clearNotice}
                  onSortChange={setWorkLibrarySort}
                  onSelectItem={library.selectItem}
                  onMarkOpened={library.markOpened}
                  onOpenExternal={library.openExternal}
                  onSetFavorite={library.setFavorite}
                  onMoveToTrash={library.moveToTrash}
                  onRestoreItem={library.restoreItem}
                  onDeletePermanently={library.deletePermanently}
                />
                {workContextPanelOpen ? (
                  <WorkContextPanel
                    activeView={workContextView}
                    resourceLabel={activeWorkResourceLabel}
                    resourceCount={library.total}
                    searchQuery={workSearchQuery}
                    chatBusy={chatRuntime.requestInFlight}
                    chatPanel={chatWorkspace}
                    libraryItem={library.selectedItem}
                    collections={library.collections}
                    itemSaving={library.actionPending}
                    onViewChange={setWorkContextView}
                    onClose={() => setWorkContextPanelOpen(false)}
                    onSaveLibraryItem={library.saveItem}
                    resize={{
                      value: layoutPreferences.workContextWidth,
                      defaultValue: DEFAULT_LAYOUT_PREFERENCES.workContextWidth,
                      minValue: LAYOUT_PANEL_LIMITS.workContext.min,
                      maxValue: LAYOUT_PANEL_LIMITS.workContext.max,
                      getMaxValue: getWorkContextMaxWidth,
                      onPreview: previewWorkContextWidth,
                      onCommit: commitWorkContextWidth,
                    }}
                  />
                ) : null}
              </Suspense>
            </PdfReaderBridgeProvider>
          ) : null}

          {workspaceMode === "chat" ? chatWorkspace : null}
        </section>
      )}
    </main>
  );
}

export default App;
