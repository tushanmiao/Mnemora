import { Suspense, useState } from "react";
import { useI18n } from "../../../i18n/I18nProvider";
import type { WorkspaceMode } from "../types";
import { findWorkspaceView } from "../viewRegistry";
import { WorkspaceViewBoundary } from "./WorkspaceViewBoundary";

type WorkspaceViewHostProps = {
  mode: WorkspaceMode;
  contextOpen: boolean;
  onReturnToChat: () => void;
};

/** 清单驱动的单视图宿主：同一时刻只渲染一个懒加载视图组件。 */
export function WorkspaceViewHost({
  mode,
  contextOpen,
  onReturnToChat,
}: WorkspaceViewHostProps) {
  const { t } = useI18n();
  const [retryKey, setRetryKey] = useState(0);
  const definition = findWorkspaceView(mode);
  if (!definition) throw new Error(`未知工作区视图：${mode}`);
  const ViewComponent = definition.component;
  const viewLabel = t(definition.labelKey);

  return (
    <section
      className="workspace-stage"
      data-workspace-mode={mode}
      data-context-open={contextOpen ? "true" : "false"}
    >
      <WorkspaceViewBoundary
        key={`${mode}:${retryKey}`}
        viewLabel={viewLabel}
        failureLabel={t("view.loadFailed", { view: viewLabel })}
        retryLabel={t("view.retry")}
        returnChatLabel={t("view.returnChat")}
        onRetry={() => setRetryKey((value) => value + 1)}
        onReturnToChat={onReturnToChat}
      >
        <Suspense
          fallback={(
            <div className="workspace-loading" role="status">
              {t("view.loading", { view: viewLabel })}
            </div>
          )}
        >
          <ViewComponent />
        </Suspense>
      </WorkspaceViewBoundary>
    </section>
  );
}
