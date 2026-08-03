import { lazy, Suspense } from "react";
import NotesWorkspace from "../../notes/components/NotesWorkspace";
import { useNotesViewRuntime } from "../runtime/NotesViewRuntime";

const NotesContextPanel = lazy(() => import("../../notes/components/NotesContextPanel").then(
  (module) => ({ default: module.NotesContextPanel }),
));

/** 笔记视图退出后，编辑器、列表和可选 AI 面板同时卸载。 */
export default function NotesView() {
  const runtime = useNotesViewRuntime();
  return (
    <>
      <NotesWorkspace {...runtime.workspace} />
      {runtime.contextPanel ? (
        // 面板单独挂起，首次加载 AI 面板时不替换正在编辑的笔记界面。
        <Suspense fallback={<div className="workspace-loading" role="status">正在打开 AI 面板</div>}>
          <NotesContextPanel {...runtime.contextPanel} />
        </Suspense>
      ) : null}
    </>
  );
}
