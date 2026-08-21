import { useCallback, type CSSProperties, type RefObject } from "react";
import type { AppSettings } from "../../types/appSettings";
import type { WorkspaceMode } from "../../features/workspace/types";
import {
  DEFAULT_LAYOUT_PREFERENCES,
  LAYOUT_PANEL_LIMITS,
  useLayoutPreferences,
} from "../../features/layout/hooks/useLayoutPreferences";
import { resolveThemeBackgroundCss } from "../../features/settings/utils/themeBackground";
import { resolveNoteFontFamily, resolveReadingFontFamily } from "../../features/settings/utils/fontSettings";

const CHAT_WORKSPACE_MIN_WIDTH = 420;
const WORK_MAIN_MIN_WIDTH = 520;

export function useWorkspaceLayout(
  appShellRef: RefObject<HTMLElement | null>,
  workspaceMode: WorkspaceMode,
  appSettings: AppSettings,
) {
  const { preferences, savePanelWidth } = useLayoutPreferences();
  const sidebarWidth = workspaceMode === "work"
    ? preferences.workSidebarWidth
    : preferences.chatSidebarWidth;
  const sidebarDefaultWidth = workspaceMode === "work"
    ? DEFAULT_LAYOUT_PREFERENCES.workSidebarWidth
    : DEFAULT_LAYOUT_PREFERENCES.chatSidebarWidth;

  const previewSidebarWidth = useCallback((width: number) => {
    appShellRef.current?.style.setProperty("--sidebar-width", `${Math.round(width)}px`);
  }, [appShellRef]);
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
  }, [appShellRef]);
  const commitWorkContextWidth = useCallback((width: number) => {
    savePanelWidth("workContext", width);
  }, [savePanelWidth]);
  const getContextMaxWidth = useCallback((handle: HTMLButtonElement) => {
    const stage = handle.closest<HTMLElement>(".workspace-stage");
    const availableWidth = stage?.getBoundingClientRect().width ?? window.innerWidth;
    return Math.max(
      LAYOUT_PANEL_LIMITS.workContext.min,
      Math.min(LAYOUT_PANEL_LIMITS.workContext.max, availableWidth - WORK_MAIN_MIN_WIDTH),
    );
  }, []);
  const previewNotesContextWidth = useCallback((width: number) => {
    appShellRef.current?.style.setProperty("--notes-context-width", `${Math.round(width)}px`);
  }, [appShellRef]);
  const commitNotesContextWidth = useCallback((width: number) => {
    savePanelWidth("notesContext", width);
  }, [savePanelWidth]);

  const customBackground = resolveThemeBackgroundCss(appSettings.themeBackground);
  const appThemeStyle = {
    "--reading-font-size": `${appSettings.fontSize}px`,
    "--reading-letter-spacing": `${appSettings.letterSpacing}px`,
    "--reading-font-family": resolveReadingFontFamily(appSettings),
    "--note-font-size": `${appSettings.noteFontSize}px`,
    "--note-line-height": String(appSettings.noteLineHeight),
    "--note-font-family": resolveNoteFontFamily(appSettings),
    "--app-custom-background": customBackground ?? "var(--color-app)",
    "--app-surface-opacity": `${customBackground ? appSettings.themeBackground.surfaceOpacity : 100}%`,
    "--sidebar-width": `${sidebarWidth}px`,
    "--work-context-width": `${preferences.workContextWidth}px`,
    "--notes-context-width": `${preferences.notesContextWidth}px`,
  } as CSSProperties;

  return {
    preferences,
    sidebarWidth,
    sidebarDefaultWidth,
    appThemeStyle,
    hasCustomBackground: Boolean(customBackground),
    previewSidebarWidth,
    commitSidebarWidth,
    getSidebarMaxWidth,
    previewWorkContextWidth,
    commitWorkContextWidth,
    getContextMaxWidth,
    previewNotesContextWidth,
    commitNotesContextWidth,
  };
}
