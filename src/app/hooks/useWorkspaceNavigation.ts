import { useCallback, useState } from "react";
import type { LibrarySort } from "../../features/library/types";
import type { SettingsCategory } from "../../features/settings/components/SettingsPage";
import type {
  WorkContextView,
  WorkLibraryView,
  WorkspaceMode,
} from "../../features/workspace/types";

export type AppView = "workspace" | "settings";

/** 应用一级导航状态。启动视图固定为 Chat，设置页作为覆盖工作区的独立页面。 */
export function useWorkspaceNavigation() {
  const [activeView, setActiveView] = useState<AppView>("workspace");
  const [workspaceMode, setWorkspaceMode] = useState<WorkspaceMode>("chat");
  const [workLibraryView, setWorkLibraryView] = useState<WorkLibraryView>("all");
  const [workSearchQuery, setWorkSearchQuery] = useState("");
  const [workCollectionId, setWorkCollectionId] = useState<string | null>(null);
  const [workLibrarySort, setWorkLibrarySort] = useState<LibrarySort>("updated");
  const [workContextPanelOpen, setWorkContextPanelOpen] = useState(false);
  const [workContextView, setWorkContextView] = useState<WorkContextView>("info");
  const [notesContextPanelOpen, setNotesContextPanelOpen] = useState(false);
  const [settingsCategory, setSettingsCategory] = useState<SettingsCategory>("general");
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);

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

  return {
    activeView,
    setActiveView,
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
  };
}
