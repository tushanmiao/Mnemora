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
import { LoaderCircle } from "lucide-react";
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
import {
  exportStoredConversation,
  loadStoredConversation,
  saveStoredConversationAsNote,
} from "./features/conversations/api/conversations";
import {
  saveMessageAsNote,
  summarizeConversationToNote,
} from "./features/chat/utils/noteGeneration";
import { SettingsPage, type SettingsCategory } from "./features/settings/components/SettingsPage";
import { useAppSettings } from "./features/settings/hooks/useAppSettings";
import { resolveThemeBackgroundCss } from "./features/settings/utils/themeBackground";
import { resolveReadingFontFamily } from "./features/settings/utils/fontSettings";
import { useSkills } from "./features/skills/hooks/useSkills";
import { PdfReaderBridgeProvider } from "./features/pdf/context/PdfReaderContext";
import { useLibrary } from "./features/library/hooks/useLibrary";
import type { LibrarySort } from "./features/library/types";
import {
  appendLiteratureReference,
  normalizeLinkedLibraryItemIds,
} from "./features/chat/utils/literatureReferences";
import { addChatQuote, removeChatQuote } from "./features/chat/utils/quotes";
import {
  DEFAULT_LAYOUT_PREFERENCES,
  LAYOUT_PANEL_LIMITS,
  useLayoutPreferences,
} from "./features/layout/hooks/useLayoutPreferences";
import type { AiPermissionMode, ChatQuote, LiteratureReference, NoteReference } from "./types/chat";
import { resolveConversationModel } from "./types/modelSettings";
import { resolveSupportsVision } from "./data/modelMatching";
import type {
  WorkContextView,
  WorkLibraryView,
  LiteratureNavigationRequest,
  WorkPdfDocument,
  WorkspaceMode,
} from "./features/workspace/types";
import { I18nProvider } from "./i18n/I18nProvider";
import { ImageViewerProvider } from "./features/chat/image-viewer/ImageViewerContext";
import { appendNoteReference } from "./features/chat/utils/noteReferences";
import { retryLazy } from "./bootstrap/retryLazy";
import { NotesErrorBoundary } from "./features/notes/components/NotesErrorBoundary";

const WorkWorkspace = lazy(() => import("./features/workspace/components/WorkWorkspace"));
const WorkContextPanel = lazy(() => import("./features/workspace/components/WorkContextPanel").then(
  (module) => ({ default: module.WorkContextPanel }),
));
const NotesWorkspace = retryLazy(() => import("./features/notes/components/NotesWorkspace"));
const NotesContextPanel = lazy(() => import("./features/notes/components/NotesContextPanel").then(
  (module) => ({ default: module.NotesContextPanel }),
));

type AppView = "workspace" | "settings";

const CHAT_WORKSPACE_MIN_WIDTH = 420;
const WORK_MAIN_MIN_WIDTH = 520;

/** 生成笔记链路的错误既可能是 Error，也可能是 Rust 返回的 ModelError 结构。 */
function noteErrorText(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "object" && error !== null && "message" in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string" && message.trim()) return message;
  }
  return String(error);
}

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
  const [notesContextPanelOpen, setNotesContextPanelOpen] = useState(false);
  const [settingsCategory, setSettingsCategory] = useState<SettingsCategory>("general");
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [modelMenuRequest, setModelMenuRequest] = useState(0);
  const [composerFocusRequest, setComposerFocusRequest] = useState(0);
  const [pendingLiteratureReferences, setPendingLiteratureReferences] = useState<LiteratureReference[]>([]);
  const pendingLiteratureReferencesRef = useRef<LiteratureReference[]>([]);
  const workScopeInitializedConversationRef = useRef<string | null>(null);
  const [literatureReferenceError, setLiteratureReferenceError] = useState("");
  const [pendingNoteReferences, setPendingNoteReferences] = useState<NoteReference[]>([]);
  const pendingNoteReferencesRef = useRef<NoteReference[]>([]);
  const preservePendingNoteReferencesRef = useRef(false);
  const [workPdfDocuments, setWorkPdfDocuments] = useState<WorkPdfDocument[]>([]);
  const [literatureNavigationRequest, setLiteratureNavigationRequest] = useState<LiteratureNavigationRequest | null>(null);
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
  // 选中助手回答片段后的引用状态；切换会话即失效，最多保留 10 条。
  const [quotedTexts, setQuotedTexts] = useState<ChatQuote[]>([]);
  const appendQuote = useCallback((text: string) => {
    setQuotedTexts((current) => addChatQuote(current, text));
  }, []);
  const removeQuote = useCallback((quoteId: string) => {
    setQuotedTexts((current) => removeChatQuote(current, quoteId));
  }, []);
  const clearQuotes = useCallback(() => {
    setQuotedTexts([]);
  }, []);
  useEffect(() => {
    setQuotedTexts([]);
    pendingLiteratureReferencesRef.current = [];
    workScopeInitializedConversationRef.current = null;
    setPendingLiteratureReferences([]);
    setLiteratureReferenceError("");
    if (preservePendingNoteReferencesRef.current) {
      preservePendingNoteReferencesRef.current = false;
    } else {
      pendingNoteReferencesRef.current = [];
      setPendingNoteReferences([]);
    }
  }, [conversations.currentConversationId]);

  const updateLinkedLibraryItemIds = useCallback((itemIds: string[]) => {
    if (conversations.requestInFlightRef.current) {
      setLiteratureReferenceError("AI 正在生成，结束后再修改文献范围。");
      return;
    }
    const normalized = normalizeLinkedLibraryItemIds(itemIds);
    if (normalized.length < new Set(itemIds).size) {
      setLiteratureReferenceError("文献范围最多关联 12 篇有效文献。");
    } else {
      setLiteratureReferenceError("");
    }
    conversations.updateCurrentConversation((conversation) => ({
      ...conversation,
      linkedLibraryItemIds: normalized,
      updatedAt: Date.now(),
    }));
  }, [conversations.requestInFlightRef, conversations.updateCurrentConversation]);

  const addLiteratureReference = useCallback((reference: LiteratureReference) => {
    if (!conversations.currentConversation) {
      setLiteratureReferenceError("请先新建或选择一个对话，再加入文献引用。");
      setWorkContextPanelOpen(true);
      setWorkContextView("chat");
      return;
    }
    if (conversations.requestInFlightRef.current) {
      setLiteratureReferenceError("AI 正在生成，结束后再加入新的文献引用。");
      setWorkContextPanelOpen(true);
      setWorkContextView("chat");
      return;
    }
    setWorkContextPanelOpen(true);
    setWorkContextView("chat");
    setComposerFocusRequest((request) => request + 1);
    const result = appendLiteratureReference(
      pendingLiteratureReferencesRef.current,
      reference,
    );
    setLiteratureReferenceError(result.error);
    if (!result.added) return;
    pendingLiteratureReferencesRef.current = result.references;
    setPendingLiteratureReferences(result.references);
    conversations.updateCurrentConversation((conversation) => ({
      ...conversation,
      linkedLibraryItemIds: normalizeLinkedLibraryItemIds([
        ...(conversation.linkedLibraryItemIds ?? []),
        reference.libraryItemId,
      ]),
      updatedAt: Date.now(),
    }));
  }, [
    conversations.currentConversation,
    conversations.requestInFlightRef,
    conversations.updateCurrentConversation,
  ]);

  const removePendingLiteratureReference = useCallback((referenceId: string) => {
    const next = pendingLiteratureReferencesRef.current.filter(
      (reference) => reference.id !== referenceId,
    );
    pendingLiteratureReferencesRef.current = next;
    setPendingLiteratureReferences(next);
    setLiteratureReferenceError("");
  }, []);

  const clearPendingLiteratureReferences = useCallback(() => {
    pendingLiteratureReferencesRef.current = [];
    setPendingLiteratureReferences([]);
    setLiteratureReferenceError("");
  }, []);

  const addNoteReference = useCallback((reference: NoteReference) => {
    if (!conversations.currentConversation) {
      preservePendingNoteReferencesRef.current = true;
      conversations.createNewConversation();
    }
    if (conversations.requestInFlightRef.current) return;
    const result = appendNoteReference(pendingNoteReferencesRef.current, reference);
    if (!result.added) {
      if (result.error) window.alert(result.error);
      return;
    }
    pendingNoteReferencesRef.current = result.references;
    setPendingNoteReferences(result.references);
    setNotesContextPanelOpen(true);
    setComposerFocusRequest((request) => request + 1);
  }, [conversations]);

  const removePendingNoteReference = useCallback((referenceId: string) => {
    const next = pendingNoteReferencesRef.current.filter((reference) => reference.id !== referenceId);
    pendingNoteReferencesRef.current = next;
    setPendingNoteReferences(next);
  }, []);

  const clearPendingNoteReferences = useCallback(() => {
    pendingNoteReferencesRef.current = [];
    setPendingNoteReferences([]);
  }, []);

  const openLiteratureReference = useCallback((reference: LiteratureReference) => {
    setActiveView("workspace");
    setWorkspaceMode("work");
    setWorkContextPanelOpen(true);
    setWorkContextView("chat");
    setLiteratureNavigationRequest({
      requestId: crypto.randomUUID(),
      libraryItemId: reference.libraryItemId,
      title: reference.title,
      pageIndex: reference.pageIndex,
    });
  }, []);

  const handleLiteratureNavigationHandled = useCallback((requestId: string) => {
    setLiteratureNavigationRequest((current) => (
      current?.requestId === requestId ? null : current
    ));
  }, []);

  useEffect(() => {
    if (
      workspaceMode !== "work"
      || !workContextPanelOpen
      || workContextView !== "chat"
      || !conversations.currentConversation
      || conversations.requestInFlightRef.current
    ) return;
    if (workScopeInitializedConversationRef.current === conversations.currentConversation.id) return;
    if ((conversations.currentConversation.linkedLibraryItemIds?.length ?? 0) > 0) {
      workScopeInitializedConversationRef.current = conversations.currentConversation.id;
      return;
    }
    const activeDocument = workPdfDocuments.find((document) => document.active);
    if (!activeDocument) return;
    workScopeInitializedConversationRef.current = conversations.currentConversation.id;
    updateLinkedLibraryItemIds([activeDocument.libraryItemId]);
  }, [
    conversations.currentConversation,
    updateLinkedLibraryItemIds,
    workContextPanelOpen,
    workContextView,
    workPdfDocuments,
    workspaceMode,
  ]);

  const currentModel = useMemo(() => {
    const conversation = conversations.currentConversation;
    return resolveConversationModel(
      settings.modelSettings,
      conversation?.providerId ?? null,
      conversation?.modelId ?? null,
    );
  }, [conversations.currentConversation, settings.modelSettings]);

  // 生成笔记的轻量反馈：progress 常驻到被结果替换，结果 4 秒后自动消失。
  const [noteFeedback, setNoteFeedback] = useState<{
    kind: "progress" | "success" | "error";
    text: string;
  } | null>(null);
  const noteFeedbackTimerRef = useRef<number | null>(null);
  const noteSummaryBusyRef = useRef(false);

  const showNoteFeedback = useCallback((
    kind: "progress" | "success" | "error",
    text: string,
  ) => {
    if (noteFeedbackTimerRef.current !== null) window.clearTimeout(noteFeedbackTimerRef.current);
    noteFeedbackTimerRef.current = null;
    setNoteFeedback({ kind, text });
    if (kind !== "progress") {
      noteFeedbackTimerRef.current = window.setTimeout(() => setNoteFeedback(null), 4_000);
    }
  }, []);

  useEffect(() => () => {
    if (noteFeedbackTimerRef.current !== null) window.clearTimeout(noteFeedbackTimerRef.current);
  }, []);

  /** 对话原文转存为笔记；Rust 按 ID 加载，无需前端先打开对话。 */
  const handleSaveConversationAsNote = useCallback((conversationId: string) => {
    void saveStoredConversationAsNote(conversationId)
      .then((note) => showNoteFeedback("success", `已保存为笔记「${note.title}」`))
      .catch((error) => showNoteFeedback("error", `保存笔记失败：${noteErrorText(error)}`));
  }, [showNoteFeedback]);

  /** AI 总结为笔记；用对话自己记录的模型（回退全局默认），全程串行防重入。 */
  const handleSummarizeConversationToNote = useCallback(async (conversationId: string) => {
    if (noteSummaryBusyRef.current) {
      showNoteFeedback("error", "已有一个总结任务正在进行，请稍候再试。");
      return;
    }
    noteSummaryBusyRef.current = true;
    showNoteFeedback("progress", "正在用模型总结对话…");
    try {
      const conversation = conversationId === conversations.currentConversationId
        && conversations.currentConversation
        ? conversations.currentConversation
        : await loadStoredConversation(conversationId);
      const model = resolveConversationModel(
        settings.modelSettings,
        conversation.providerId,
        conversation.modelId,
      );
      if (!model) {
        showNoteFeedback("error", "请先在设置中配置可用的默认模型。");
        return;
      }
      const note = await summarizeConversationToNote(
        conversation,
        model,
        {
          maxOutputTokens: settings.appSettings.maxOutputTokens,
          thinkingEnabled: settings.appSettings.thinkingEnabled,
        },
      );
      showNoteFeedback("success", `已生成总结笔记「${note.title}」`);
    } catch (error) {
      showNoteFeedback("error", `总结失败：${noteErrorText(error)}`);
    } finally {
      noteSummaryBusyRef.current = false;
    }
  }, [
    conversations.currentConversation,
    conversations.currentConversationId,
    settings.appSettings.maxOutputTokens,
    settings.appSettings.thinkingEnabled,
    settings.modelSettings,
    showNoteFeedback,
  ]);

  /** 单条助手回答转存为笔记；返回是否成功，供气泡按钮展示对勾反馈。 */
  const handleSaveMessageAsNote = useCallback(async (messageId: string) => {
    const conversation = conversations.currentConversation;
    if (!conversation) return false;
    try {
      const note = await saveMessageAsNote(conversation, messageId);
      showNoteFeedback("success", `已保存为笔记「${note.title}」`);
      return true;
    } catch (error) {
      showNoteFeedback("error", `保存笔记失败：${noteErrorText(error)}`);
      return false;
    }
  }, [conversations.currentConversation, showNoteFeedback]);

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

  const sidebarWidth = workspaceMode === "chat" || workspaceMode === "notes"
    ? layoutPreferences.chatSidebarWidth
    : layoutPreferences.workSidebarWidth;
  const sidebarDefaultWidth = workspaceMode === "chat" || workspaceMode === "notes"
    ? DEFAULT_LAYOUT_PREFERENCES.chatSidebarWidth
    : DEFAULT_LAYOUT_PREFERENCES.workSidebarWidth;

  const previewSidebarWidth = useCallback((width: number) => {
    appShellRef.current?.style.setProperty("--sidebar-width", `${Math.round(width)}px`);
  }, []);
  const commitSidebarWidth = useCallback((width: number) => {
    savePanelWidth(workspaceMode === "work" ? "workSidebar" : "chatSidebar", width);
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
  const previewNotesContextWidth = useCallback((width: number) => {
    appShellRef.current?.style.setProperty("--notes-context-width", `${Math.round(width)}px`);
  }, []);
  const commitNotesContextWidth = useCallback((width: number) => {
    savePanelWidth("notesContext", width);
  }, [savePanelWidth]);

  const customBackground = resolveThemeBackgroundCss(settings.appSettings.themeBackground);
  const appThemeStyle = {
    "--app-font-size": `${settings.appSettings.fontSize}px`,
    "--reading-letter-spacing": `${settings.appSettings.letterSpacing}px`,
    "--reading-font-family": resolveReadingFontFamily(settings.appSettings),
    "--app-custom-background": customBackground ?? "var(--color-app)",
    "--app-surface-opacity": `${customBackground
      ? settings.appSettings.themeBackground.surfaceOpacity
      : 100}%`,
    "--sidebar-width": `${sidebarWidth}px`,
    "--work-context-width": `${layoutPreferences.workContextWidth}px`,
    "--notes-context-width": `${layoutPreferences.notesContextWidth}px`,
  } as CSSProperties;

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

  const chatWorkspace = (
    <section
      className={`chat-workspace${workspaceMode !== "chat" ? " chat-workspace-panel" : ""}`}
      aria-label={workspaceMode === "work" ? "文献 AI 对话" : workspaceMode === "notes" ? "笔记 AI 对话" : "当前对话"}
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
        compact={workspaceMode !== "chat"}
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
        onQuoteMessage={appendQuote}
        onSaveMessageAsNote={handleSaveMessageAsNote}
        onLiteratureReferenceOpen={openLiteratureReference}
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
        focusRequest={composerFocusRequest}
        contextUsage={contextUsage}
        contextWindowTokens={currentModel?.model.contextWindowTokens ?? null}
        supportsVision={currentModel
          ? resolveSupportsVision(
              currentModel.model.apiModel,
              currentModel.model.capabilities?.vision,
            ) ?? null
          : null}
        showLiteraturePicker={workspaceMode === "work"}
        quotes={quotedTexts}
        onQuoteRemove={removeQuote}
        onQuotesClear={clearQuotes}
        literatureReferences={pendingLiteratureReferences}
        onLiteratureReferenceRemove={removePendingLiteratureReference}
        onLiteratureReferencesClear={clearPendingLiteratureReferences}
        noteReferences={pendingNoteReferences}
        onNoteReferenceRemove={removePendingNoteReference}
        onNoteReferencesClear={clearPendingNoteReferences}
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
    <I18nProvider language={settings.appSettings.interfaceLanguage}>
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
      <ImageViewerProvider>
      {workspaceMode !== "notes" ? <Sidebar
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
        onCreateConversation={() => {
          conversations.createNewConversation();
          setWorkspaceMode("chat");
        }}
        onSelectConversation={(conversationId) => {
          conversations.selectConversation(conversationId);
          setWorkspaceMode("chat");
        }}
        onDeleteConversation={conversations.deleteConversation}
        onExportConversation={(conversationId, format) => {
          const item = conversations.conversationListItems.find((conversation) => conversation.id === conversationId);
          void exportStoredConversation(conversationId, item?.title ?? "Mnemora 会话", format)
            .catch((error) => {
              const message = error instanceof Error ? error.message : String(error);
              window.alert(`导出失败：${message}`);
            });
        }}
        onSaveConversationAsNote={handleSaveConversationAsNote}
        onSummarizeConversationToNote={(conversationId) => {
          void handleSummarizeConversationToNote(conversationId);
        }}
        onClearConversations={conversations.clearConversations}
        onLoadMoreConversations={conversations.loadMoreConversations}
        onOpenSettings={() => openSettings("general")}
        onOpenSkills={() => openSettings("skills")}
        onOpenNotes={() => changeWorkspaceMode("notes")}
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
        />
      ) : (
        <section
          className="workspace-stage"
          data-workspace-mode={workspaceMode}
          data-context-open={(workspaceMode === "work" ? workContextPanelOpen : workspaceMode === "notes" ? notesContextPanelOpen : false) ? "true" : "false"}
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
                  collections={library.collections}
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
                  literatureNavigationRequest={literatureNavigationRequest}
                  onToggleContextPanel={() => setWorkContextPanelOpen((open) => !open)}
                  onAskSelection={addLiteratureReference}
                  onPdfDocumentsChange={setWorkPdfDocuments}
                  onLiteratureNavigationHandled={handleLiteratureNavigationHandled}
                  onImport={library.importPdfs}
                  onRefresh={library.refresh}
                  onDismissNotice={library.clearNotice}
                  onSortChange={setWorkLibrarySort}
                  onSelectItem={library.selectItem}
                  onMarkOpened={library.markOpened}
                  onOpenExternal={library.openExternal}
                  onSetFavorite={library.setFavorite}
                  onSaveItem={library.saveItem}
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
                    pdfDocuments={workPdfDocuments}
                    linkedLibraryItemIds={conversations.currentConversation?.linkedLibraryItemIds ?? []}
                    literatureReferenceError={literatureReferenceError}
                    conversationAvailable={conversations.currentConversation !== null}
                    libraryItem={library.selectedItem}
                    collections={library.collections}
                    itemSaving={library.actionPending}
                    onViewChange={setWorkContextView}
                    onClose={() => setWorkContextPanelOpen(false)}
                    onLinkedLibraryItemIdsChange={updateLinkedLibraryItemIds}
                    onAddLiteratureReference={addLiteratureReference}
                    onClearLiteratureReferenceError={() => setLiteratureReferenceError("")}
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

          {workspaceMode === "notes" ? (
            <NotesErrorBoundary>
              <Suspense fallback={<div className="workspace-loading" role="status">正在打开笔记</div>}>
              <NotesWorkspace
                chatOpen={notesContextPanelOpen}
                chatBusy={chatRuntime.requestInFlight}
                userDisplayName={settings.appSettings.userDisplayName}
                onBack={() => {
                  setNotesContextPanelOpen(false);
                  changeWorkspaceMode("chat");
                }}
                onToggleChat={() => {
                  if (!conversations.currentConversation) conversations.createNewConversation();
                  setNotesContextPanelOpen((open) => !open);
                }}
                onAskSelection={addNoteReference}
              />
              {notesContextPanelOpen ? (
                // 独立 Suspense：面板懒加载挂起时不能连累 NotesWorkspace 整体回退卸载，
                // 否则正在编辑的笔记状态会被清空（首次打开 AI 面板即触发）。
                <Suspense fallback={<div className="workspace-loading" role="status">正在打开 AI 面板</div>}>
                  <NotesContextPanel
                    chatPanel={chatWorkspace}
                    onClose={() => setNotesContextPanelOpen(false)}
                    resize={{
                      value: layoutPreferences.notesContextWidth,
                      defaultValue: DEFAULT_LAYOUT_PREFERENCES.notesContextWidth,
                      minValue: LAYOUT_PANEL_LIMITS.notesContext.min,
                      maxValue: LAYOUT_PANEL_LIMITS.notesContext.max,
                      getMaxValue: getWorkContextMaxWidth,
                      onPreview: previewNotesContextWidth,
                      onCommit: commitNotesContextWidth,
                    }}
                  />
                </Suspense>
              ) : null}
              </Suspense>
            </NotesErrorBoundary>
          ) : null}

          {workspaceMode === "chat" ? chatWorkspace : null}
        </section>
      )}
      </ImageViewerProvider>

      {noteFeedback ? (
        <div
          className={`app-toast app-toast-${noteFeedback.kind}`}
          role="status"
          aria-live="polite"
        >
          {noteFeedback.kind === "progress" ? (
            <LoaderCircle size={15} className="app-toast-spinner" />
          ) : null}
          <span>{noteFeedback.text}</span>
        </div>
      ) : null}
    </main>
    </I18nProvider>
  );
}

export default App;
