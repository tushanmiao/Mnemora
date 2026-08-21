import { useCallback, useEffect, useMemo, useState } from "react";
import type { LibraryItem, LibraryNote } from "../../library/types";
import type { WorkNoteSourceContext, WorkResourceTab } from "../types";

const WORK_SESSION_STORAGE_KEY = "mnemora.work-session.v1";
const MAX_OPEN_WORK_TABS = 20;

const LIBRARY_TAB: WorkResourceTab = {
  id: "library",
  kind: "library",
  title: "我的文库",
  closable: false,
};

type StoredWorkSession = {
  version: 1;
  tabs: WorkResourceTab[];
  activeTabId: string;
};

function validResourceTab(value: unknown): value is WorkResourceTab {
  if (!value || typeof value !== "object") return false;
  const tab = value as Partial<WorkResourceTab>;
  const validKind = tab.kind === "pdf" || tab.kind === "note";
  const validPrefix = tab.kind === "pdf"
    ? typeof tab.id === "string" && tab.id.startsWith("pdf:")
    : tab.kind === "note" && typeof tab.id === "string" && tab.id.startsWith("note:");
  const validNoteSource = tab.noteSource === undefined || (
    tab.kind === "note"
    && Boolean(tab.noteSource)
    && typeof tab.noteSource?.sourcePdfId === "string"
    && tab.noteSource.sourcePdfId.length > 0
    && typeof tab.noteSource.sourcePdfTitle === "string"
    && tab.noteSource.sourcePdfTitle.trim().length > 0
    && (tab.noteSource.sourcePageIndex === null
      || (Number.isInteger(tab.noteSource.sourcePageIndex) && tab.noteSource.sourcePageIndex >= 0))
  );
  return validKind
    && validPrefix
    && typeof tab.title === "string"
    && tab.title.trim().length > 0
    && tab.title.length <= 500
    && tab.closable === true
    && typeof tab.resourceId === "string"
    && tab.resourceId.length > 0
    && validNoteSource;
}

export function normalizeWorkSession(value: unknown): StoredWorkSession {
  if (!value || typeof value !== "object") {
    return { version: 1, tabs: [LIBRARY_TAB], activeTabId: LIBRARY_TAB.id };
  }
  const candidate = value as Partial<StoredWorkSession>;
  const resourceTabs = Array.isArray(candidate.tabs)
    ? candidate.tabs.filter(validResourceTab).slice(0, MAX_OPEN_WORK_TABS - 1)
    : [];
  const deduplicated = new Map<string, WorkResourceTab>();
  for (const tab of resourceTabs) deduplicated.set(tab.id, tab);
  const tabs = [LIBRARY_TAB, ...deduplicated.values()];
  const activeTabId = typeof candidate.activeTabId === "string"
    && tabs.some((tab) => tab.id === candidate.activeTabId)
    ? candidate.activeTabId
    : LIBRARY_TAB.id;
  return { version: 1, tabs, activeTabId };
}

function readWorkSession(): StoredWorkSession {
  try {
    const raw = window.localStorage.getItem(WORK_SESSION_STORAGE_KEY);
    return raw ? normalizeWorkSession(JSON.parse(raw)) : normalizeWorkSession(null);
  } catch {
    return normalizeWorkSession(null);
  }
}

export function useWorkSession() {
  const [session, setSession] = useState<StoredWorkSession>(readWorkSession);

  useEffect(() => {
    try {
      window.localStorage.setItem(WORK_SESSION_STORAGE_KEY, JSON.stringify(session));
    } catch {
      // 存储不可用时，页签仍可在当前 WebView 生命周期中工作。
    }
  }, [session]);

  const activeTab = useMemo(
    () => session.tabs.find((tab) => tab.id === session.activeTabId) ?? session.tabs[0],
    [session],
  );

  const selectTab = useCallback((tabId: string) => {
    setSession((current) => current.tabs.some((tab) => tab.id === tabId)
      ? { ...current, activeTabId: tabId }
      : current);
  }, []);

  const openNote = useCallback((
    note: Pick<LibraryNote, "id" | "title">,
    noteSource?: WorkNoteSourceContext,
  ) => {
    const tabId = `note:${note.id}`;
    setSession((current) => {
      const existing = current.tabs.find((tab) => tab.id === tabId);
      if (existing) {
        return {
          ...current,
          tabs: current.tabs.map((tab) => tab.id === tabId ? {
            ...tab,
            title: note.title,
            noteSource: noteSource ?? tab.noteSource,
          } : tab),
          activeTabId: tabId,
        };
      }
      const tabs = [
        ...current.tabs,
        {
          id: tabId,
          kind: "note" as const,
          title: note.title,
          closable: true,
          resourceId: note.id,
          noteSource,
        },
      ].slice(-(MAX_OPEN_WORK_TABS - 1));
      return {
        ...current,
        tabs: [LIBRARY_TAB, ...tabs.filter((tab) => tab.id !== LIBRARY_TAB.id)],
        activeTabId: tabId,
      };
    });
  }, []);

  const updateNoteTab = useCallback((note: LibraryNote) => {
    const tabId = `note:${note.id}`;
    setSession((current) => ({
      ...current,
      tabs: current.tabs.map((tab) => tab.id === tabId ? { ...tab, title: note.title } : tab),
    }));
  }, []);

  const showLibrary = useCallback(() => {
    setSession((current) => ({ ...current, activeTabId: LIBRARY_TAB.id }));
  }, []);

  const openPdfReference = useCallback((itemId: string, title: string) => {
    const tabId = `pdf:${itemId}`;
    setSession((current) => {
      const existing = current.tabs.find((tab) => tab.id === tabId);
      if (existing) {
        const tabs = current.tabs.map((tab) => tab.id === tabId
          ? { ...tab, title }
          : tab);
        return { ...current, tabs, activeTabId: tabId };
      }
      const tabs = [
        ...current.tabs,
        {
          id: tabId,
          kind: "pdf" as const,
          title,
          closable: true,
          resourceId: itemId,
        },
      ].slice(-(MAX_OPEN_WORK_TABS - 1));
      return {
        ...current,
        tabs: [LIBRARY_TAB, ...tabs.filter((tab) => tab.id !== LIBRARY_TAB.id)],
        activeTabId: tabId,
      };
    });
  }, []);

  const openPdf = useCallback((item: LibraryItem) => {
    openPdfReference(item.id, item.title);
  }, [openPdfReference]);

  const closeTab = useCallback((tabId: string) => {
    setSession((current) => {
      const targetIndex = current.tabs.findIndex((tab) => tab.id === tabId);
      const target = current.tabs[targetIndex];
      if (!target?.closable) return current;
      const tabs = current.tabs.filter((tab) => tab.id !== tabId);
      if (current.activeTabId !== tabId) return { ...current, tabs };
      const nextActive = tabs[Math.max(0, targetIndex - 1)] ?? tabs[0] ?? LIBRARY_TAB;
      return { ...current, tabs, activeTabId: nextActive.id };
    });
  }, []);

  const closeResource = useCallback((resourceId: string) => {
    setSession((current) => {
      const target = current.tabs.find((tab) => tab.resourceId === resourceId);
      if (!target) return current;
      const tabs = current.tabs.filter((tab) => tab.resourceId !== resourceId);
      return {
        ...current,
        tabs,
        activeTabId: current.activeTabId === target.id ? LIBRARY_TAB.id : current.activeTabId,
      };
    });
  }, []);

  return {
    tabs: session.tabs,
    activeTab,
    selectTab,
    showLibrary,
    openPdf,
    openPdfReference,
    openNote,
    updateNoteTab,
    closeTab,
    closeResource,
  };
}
