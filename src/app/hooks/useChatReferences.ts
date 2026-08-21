import { useCallback, useEffect, useRef, useState } from "react";
import type { ChatQuote, LiteratureReference, NoteReference } from "../../types/chat";
import type {
  LiteratureNavigationRequest,
  WorkContextView,
  WorkPdfDocument,
  WorkspaceMode,
} from "../../features/workspace/types";
import type { useConversations } from "../../features/conversations/hooks/useConversations";
import {
  appendLiteratureReference,
  normalizeLinkedLibraryItemIds,
} from "../../features/chat/utils/literatureReferences";
import { appendNoteReference } from "../../features/chat/utils/noteReferences";
import { addChatQuote, removeChatQuote } from "../../features/chat/utils/quotes";

type ConversationsRuntime = ReturnType<typeof useConversations>;

export function useChatReferences({
  conversations,
  workspaceMode,
  workContextPanelOpen,
  workContextView,
  setWorkspaceMode,
  setWorkContextPanelOpen,
  setWorkContextView,
  setNotesContextPanelOpen,
  focusComposer,
}: {
  conversations: ConversationsRuntime;
  workspaceMode: WorkspaceMode;
  workContextPanelOpen: boolean;
  workContextView: WorkContextView;
  setWorkspaceMode: (mode: WorkspaceMode) => void;
  setWorkContextPanelOpen: (open: boolean) => void;
  setWorkContextView: (view: WorkContextView) => void;
  setNotesContextPanelOpen: (open: boolean) => void;
  focusComposer: () => void;
}) {
  const [quotes, setQuotes] = useState<ChatQuote[]>([]);
  const [literatureReferences, setLiteratureReferences] = useState<LiteratureReference[]>([]);
  const literatureReferencesRef = useRef<LiteratureReference[]>([]);
  const [literatureReferenceError, setLiteratureReferenceError] = useState("");
  const workScopeInitializedConversationRef = useRef<string | null>(null);
  const [noteReferences, setNoteReferences] = useState<NoteReference[]>([]);
  const noteReferencesRef = useRef<NoteReference[]>([]);
  const preserveNoteReferencesRef = useRef(false);
  const [workPdfDocuments, setWorkPdfDocuments] = useState<WorkPdfDocument[]>([]);
  const [literatureNavigationRequest, setLiteratureNavigationRequest] = useState<LiteratureNavigationRequest | null>(null);

  useEffect(() => {
    setQuotes([]);
    literatureReferencesRef.current = [];
    workScopeInitializedConversationRef.current = null;
    setLiteratureReferences([]);
    setLiteratureReferenceError("");
    if (preserveNoteReferencesRef.current) {
      preserveNoteReferencesRef.current = false;
    } else {
      noteReferencesRef.current = [];
      setNoteReferences([]);
    }
  }, [conversations.currentConversationId]);

  const appendQuote = useCallback((text: string) => {
    setQuotes((current) => addChatQuote(current, text));
  }, []);
  const removeQuote = useCallback((quoteId: string) => {
    setQuotes((current) => removeChatQuote(current, quoteId));
  }, []);
  const clearQuotes = useCallback(() => setQuotes([]), []);

  const updateLinkedLibraryItemIds = useCallback((itemIds: string[]) => {
    if (conversations.requestInFlightRef.current) {
      setLiteratureReferenceError("AI 正在生成，结束后再修改文献范围。");
      return;
    }
    const normalized = normalizeLinkedLibraryItemIds(itemIds);
    setLiteratureReferenceError(
      normalized.length < new Set(itemIds).size ? "文献范围最多关联 12 篇有效文献。" : "",
    );
    conversations.updateCurrentConversation((conversation) => ({
      ...conversation,
      linkedLibraryItemIds: normalized,
      updatedAt: Date.now(),
    }));
  }, [conversations]);

  const addLiteratureReference = useCallback((reference: LiteratureReference) => {
    setWorkContextPanelOpen(true);
    setWorkContextView("chat");
    if (!conversations.currentConversation) {
      if (conversations.currentConversationId) {
        void conversations.ensureCurrentConversationLoaded().then((loaded) => {
          if (loaded) addLiteratureReference(reference);
          else setLiteratureReferenceError("恢复当前对话失败，请重新选择对话。");
        });
        return;
      }
      setLiteratureReferenceError("请先新建或选择一个对话，再加入文献引用。");
      return;
    }
    if (conversations.requestInFlightRef.current) {
      setLiteratureReferenceError("AI 正在生成，结束后再加入新的文献引用。");
      return;
    }
    focusComposer();
    const result = appendLiteratureReference(literatureReferencesRef.current, reference);
    setLiteratureReferenceError(result.error);
    if (!result.added) return;
    literatureReferencesRef.current = result.references;
    setLiteratureReferences(result.references);
    conversations.updateCurrentConversation((conversation) => ({
      ...conversation,
      linkedLibraryItemIds: normalizeLinkedLibraryItemIds([
        ...(conversation.linkedLibraryItemIds ?? []),
        reference.libraryItemId,
      ]),
      updatedAt: Date.now(),
    }));
  }, [conversations, focusComposer, setWorkContextPanelOpen, setWorkContextView]);

  const removeLiteratureReference = useCallback((referenceId: string) => {
    const next = literatureReferencesRef.current.filter((reference) => reference.id !== referenceId);
    literatureReferencesRef.current = next;
    setLiteratureReferences(next);
    setLiteratureReferenceError("");
  }, []);
  const clearLiteratureReferences = useCallback(() => {
    literatureReferencesRef.current = [];
    setLiteratureReferences([]);
    setLiteratureReferenceError("");
  }, []);

  const addNoteReference = useCallback((reference: NoteReference) => {
    if (!conversations.currentConversation) {
      if (conversations.currentConversationId) {
        void conversations.ensureCurrentConversationLoaded().then((loaded) => {
          if (loaded) addNoteReference(reference);
        });
        return;
      }
      preserveNoteReferencesRef.current = true;
      conversations.createNewConversation();
    }
    if (conversations.requestInFlightRef.current) return;
    const result = appendNoteReference(noteReferencesRef.current, reference);
    if (!result.added) {
      if (result.error) window.alert(result.error);
      return;
    }
    noteReferencesRef.current = result.references;
    setNoteReferences(result.references);
    if (workspaceMode === "work") {
      setWorkContextPanelOpen(true);
      setWorkContextView("chat");
    } else {
      setNotesContextPanelOpen(true);
    }
    focusComposer();
  }, [
    conversations,
    focusComposer,
    setNotesContextPanelOpen,
    setWorkContextPanelOpen,
    setWorkContextView,
    workspaceMode,
  ]);
  const removeNoteReference = useCallback((referenceId: string) => {
    const next = noteReferencesRef.current.filter((reference) => reference.id !== referenceId);
    noteReferencesRef.current = next;
    setNoteReferences(next);
  }, []);
  const clearNoteReferences = useCallback(() => {
    noteReferencesRef.current = [];
    setNoteReferences([]);
  }, []);

  const openLiteratureReference = useCallback((reference: LiteratureReference) => {
    setWorkspaceMode("work");
    setWorkContextPanelOpen(true);
    setWorkContextView("chat");
    setLiteratureNavigationRequest({
      requestId: crypto.randomUUID(),
      libraryItemId: reference.libraryItemId,
      title: reference.title,
      pageIndex: reference.pageIndex,
    });
  }, [setWorkspaceMode, setWorkContextPanelOpen, setWorkContextView]);
  const handleLiteratureNavigationHandled = useCallback((requestId: string) => {
    setLiteratureNavigationRequest((current) => current?.requestId === requestId ? null : current);
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
    conversations,
    updateLinkedLibraryItemIds,
    workContextPanelOpen,
    workContextView,
    workPdfDocuments,
    workspaceMode,
  ]);

  return {
    quotes,
    appendQuote,
    removeQuote,
    clearQuotes,
    literatureReferences,
    literatureReferenceError,
    clearLiteratureReferenceError: () => setLiteratureReferenceError(""),
    addLiteratureReference,
    removeLiteratureReference,
    clearLiteratureReferences,
    updateLinkedLibraryItemIds,
    noteReferences,
    addNoteReference,
    removeNoteReference,
    clearNoteReferences,
    workPdfDocuments,
    setWorkPdfDocuments,
    literatureNavigationRequest,
    openLiteratureReference,
    handleLiteratureNavigationHandled,
  };
}
