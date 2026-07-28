import { useCallback, useEffect, useRef, useState } from "react";
import type { WorkLibraryView } from "../../workspace/types";
import {
  chooseLibraryPdfFiles,
  createLibraryCollection,
  deleteLibraryCollection,
  deleteLibraryItemPermanently,
  getLibraryItem,
  importLibraryPdfs,
  isLibraryRuntime,
  listLibraryCollections,
  listLibraryItems,
  markLibraryItemOpened,
  moveLibraryItemToTrash,
  openLibraryItem,
  renameLibraryCollection,
  restoreLibraryItem,
  setLibraryItemFavorite,
  updateLibraryItem,
} from "../api/library";
import type {
  LibraryCollection,
  LibraryImportResult,
  LibraryItem,
  LibraryItemUpdate,
  LibrarySort,
  LibraryView,
} from "../types";

type UseLibraryOptions = {
  enabled: boolean;
  view: WorkLibraryView;
  searchQuery: string;
  collectionId: string | null;
  sort: LibrarySort;
};

const DATA_VIEWS = new Set<WorkLibraryView>([
  "all",
  "recent",
  "favorites",
  "unfiled",
  "trash",
]);

function normalizeError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function importNotice(result: LibraryImportResult): string {
  const parts = [];
  if (result.imported.length > 0) parts.push(`已导入 ${result.imported.length} 篇`);
  if (result.duplicates.length > 0) parts.push(`跳过 ${result.duplicates.length} 篇重复文献`);
  if (result.failed.length > 0) parts.push(`${result.failed.length} 篇导入失败`);
  return parts.join("，") || "没有导入文献。";
}

export function useLibrary({
  enabled,
  view,
  searchQuery,
  collectionId,
  sort,
}: UseLibraryOptions) {
  const runtimeAvailable = isLibraryRuntime();
  const [items, setItems] = useState<LibraryItem[]>([]);
  const [total, setTotal] = useState(0);
  const [collections, setCollections] = useState<LibraryCollection[]>([]);
  const [selectedItem, setSelectedItem] = useState<LibraryItem | null>(null);
  const [selectedItemLoading, setSelectedItemLoading] = useState(false);
  const [selectionError, setSelectionError] = useState("");
  const [loading, setLoading] = useState(false);
  const [actionPending, setActionPending] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [refreshVersion, setRefreshVersion] = useState(0);
  const [debouncedSearchQuery, setDebouncedSearchQuery] = useState(searchQuery);
  const listRequestRef = useRef(0);
  const itemsRef = useRef<LibraryItem[]>([]);
  const selectedItemIdRef = useRef<string | null>(null);
  const enabledRef = useRef(enabled);
  enabledRef.current = enabled;

  useEffect(() => {
    itemsRef.current = items;
  }, [items]);

  useEffect(() => {
    const timer = window.setTimeout(() => setDebouncedSearchQuery(searchQuery), 180);
    return () => window.clearTimeout(timer);
  }, [searchQuery]);

  const refresh = useCallback(() => setRefreshVersion((version) => version + 1), []);

  const loadCollections = useCallback(async () => {
    if (!enabled) return;
    try {
      const nextCollections = await listLibraryCollections();
      if (enabledRef.current) setCollections(nextCollections);
    } catch (loadError) {
      if (enabledRef.current) setError(normalizeError(loadError));
    }
  }, [enabled]);

  useEffect(() => {
    if (!enabled) {
      listRequestRef.current += 1;
      setItems([]);
      setTotal(0);
      setCollections([]);
      setSelectedItem(null);
      setSelectedItemLoading(false);
      setSelectionError("");
      selectedItemIdRef.current = null;
      setLoading(false);
      setActionPending(false);
      setError("");
      setNotice("");
      return;
    }
    void loadCollections();
  }, [enabled, loadCollections]);

  useEffect(() => {
    if (!enabled || !DATA_VIEWS.has(view)) {
      setItems([]);
      setTotal(0);
      setSelectedItem(null);
      setSelectedItemLoading(false);
      setSelectionError("");
      selectedItemIdRef.current = null;
      setLoading(false);
      return;
    }
    const requestId = listRequestRef.current + 1;
    listRequestRef.current = requestId;
    setLoading(true);
    setError("");
    void listLibraryItems({
      view: view as LibraryView,
      searchQuery: debouncedSearchQuery,
      collectionId,
      sort,
      offset: 0,
      limit: 500,
    })
      .then((page) => {
        if (listRequestRef.current !== requestId) return;
        setItems(page.items);
        setTotal(page.total);
        const selectedId = selectedItemIdRef.current;
        if (selectedId) {
          const refreshed = page.items.find((item) => item.id === selectedId);
          if (refreshed) setSelectedItem(refreshed);
        }
      })
      .catch((loadError) => {
        if (listRequestRef.current === requestId) setError(normalizeError(loadError));
      })
      .finally(() => {
        if (listRequestRef.current === requestId) setLoading(false);
      });
  }, [collectionId, debouncedSearchQuery, enabled, refreshVersion, sort, view]);

  const selectItem = useCallback(async (itemId: string | null) => {
    selectedItemIdRef.current = itemId;
    setSelectionError("");
    if (!itemId) {
      setSelectedItem(null);
      setSelectedItemLoading(false);
      return null;
    }
    const localItem = itemsRef.current.find((item) => item.id === itemId);
    if (localItem) setSelectedItem(localItem);
    if (!runtimeAvailable) return localItem ?? null;
    setSelectedItemLoading(!localItem);
    try {
      const item = await getLibraryItem(itemId);
      if (enabledRef.current && selectedItemIdRef.current === itemId) setSelectedItem(item);
      return item;
    } catch (loadError) {
      if (enabledRef.current && selectedItemIdRef.current === itemId) {
        setSelectionError(normalizeError(loadError));
      }
      return localItem ?? null;
    } finally {
      if (enabledRef.current && selectedItemIdRef.current === itemId) {
        setSelectedItemLoading(false);
      }
    }
  }, [runtimeAvailable]);

  const runAction = useCallback(async <T,>(action: () => Promise<T>): Promise<T> => {
    setActionPending(true);
    setNotice("");
    try {
      return await action();
    } catch (actionError) {
      const message = normalizeError(actionError);
      if (enabledRef.current) setNotice(`操作失败：${message}`);
      throw new Error(message);
    } finally {
      setActionPending(false);
    }
  }, []);

  const importPdfs = useCallback(async () => {
    if (!runtimeAvailable) {
      setError("请在 Tauri 桌面应用中导入 PDF。");
      return null;
    }
    const paths = await chooseLibraryPdfFiles();
    if (paths.length === 0) return null;
    const result = await runAction(() => importLibraryPdfs(paths, collectionId));
    if (!enabledRef.current) return result;
    setNotice(importNotice(result));
    refresh();
    await loadCollections();
    return result;
  }, [collectionId, loadCollections, refresh, runAction, runtimeAvailable]);

  const saveItem = useCallback(async (update: LibraryItemUpdate) => {
    const item = await runAction(() => updateLibraryItem(update));
    if (!enabledRef.current) return item;
    setSelectedItem(item);
    selectedItemIdRef.current = item.id;
    setItems((current) => current.map((candidate) => candidate.id === item.id ? item : candidate));
    refresh();
    await loadCollections();
    setNotice("文献信息已保存。");
    return item;
  }, [loadCollections, refresh, runAction]);

  const setFavorite = useCallback(async (itemId: string, favorite: boolean) => {
    const item = await runAction(() => setLibraryItemFavorite(itemId, favorite));
    if (!enabledRef.current) return item;
    setItems((current) => current.map((candidate) => candidate.id === item.id ? item : candidate));
    if (selectedItemIdRef.current === item.id) setSelectedItem(item);
    refresh();
    return item;
  }, [refresh, runAction]);

  const moveToTrash = useCallback(async (itemId: string) => {
    const item = await runAction(() => moveLibraryItemToTrash(itemId));
    if (!enabledRef.current) return item;
    if (selectedItemIdRef.current === item.id) setSelectedItem(item);
    setNotice("文献已移入回收站。");
    refresh();
    await loadCollections();
    return item;
  }, [loadCollections, refresh, runAction]);

  const restoreItem = useCallback(async (itemId: string) => {
    const item = await runAction(() => restoreLibraryItem(itemId));
    if (!enabledRef.current) return item;
    if (selectedItemIdRef.current === item.id) setSelectedItem(item);
    setNotice("文献已恢复。");
    refresh();
    await loadCollections();
    return item;
  }, [loadCollections, refresh, runAction]);

  const deletePermanently = useCallback(async (itemId: string) => {
    const removed = await runAction(() => deleteLibraryItemPermanently(itemId));
    if (!enabledRef.current) return removed;
    if (removed && selectedItemIdRef.current === itemId) {
      selectedItemIdRef.current = null;
      setSelectedItem(null);
    }
    setNotice(removed ? "文献及应用内 PDF 快照已永久删除。" : "文献不存在。");
    refresh();
    await loadCollections();
    return removed;
  }, [loadCollections, refresh, runAction]);

  const markOpened = useCallback(async (itemId: string) => {
    if (!runtimeAvailable) return selectItem(itemId);
    const item = await runAction(() => markLibraryItemOpened(itemId));
    if (!enabledRef.current) return item;
    setSelectedItem(item);
    selectedItemIdRef.current = item.id;
    setItems((current) => current.map((candidate) => candidate.id === item.id ? item : candidate));
    return item;
  }, [runAction, runtimeAvailable, selectItem]);

  const openExternal = useCallback(async (itemId: string) => {
    const item = await runAction(() => openLibraryItem(itemId));
    if (!enabledRef.current) return item;
    setSelectedItem(item);
    selectedItemIdRef.current = item.id;
    refresh();
    return item;
  }, [refresh, runAction]);

  const createCollection = useCallback(async (name: string) => {
    const collection = await runAction(() => createLibraryCollection(name));
    if (!enabledRef.current) return collection;
    await loadCollections();
    setNotice(`已创建分类“${collection.name}”。`);
    return collection;
  }, [loadCollections, runAction]);

  const renameCollection = useCallback(async (collectionIdValue: string, name: string) => {
    await runAction(() => renameLibraryCollection(collectionIdValue, name));
    if (!enabledRef.current) return;
    await loadCollections();
    refresh();
    setNotice("分类已重命名。");
  }, [loadCollections, refresh, runAction]);

  const removeCollection = useCallback(async (collectionIdValue: string) => {
    const removed = await runAction(() => deleteLibraryCollection(collectionIdValue));
    if (!enabledRef.current) return removed;
    await loadCollections();
    refresh();
    if (removed) setNotice("分类已删除，文献本身仍保留在文库中。");
    return removed;
  }, [loadCollections, refresh, runAction]);

  return {
    runtimeAvailable,
    items,
    total,
    collections,
    selectedItem,
    selectedItemLoading,
    selectionError,
    loading,
    actionPending,
    error,
    notice,
    clearNotice: () => setNotice(""),
    refresh,
    selectItem,
    importPdfs,
    saveItem,
    setFavorite,
    moveToTrash,
    restoreItem,
    deletePermanently,
    markOpened,
    openExternal,
    createCollection,
    renameCollection,
    removeCollection,
  };
}
