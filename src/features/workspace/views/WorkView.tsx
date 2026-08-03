import { lazy, Suspense } from "react";
import { PdfReaderBridgeProvider } from "../../pdf/context/PdfReaderContext";
import WorkWorkspace from "../components/WorkWorkspace";
import { useWorkViewRuntime } from "../runtime/WorkViewRuntime";

const WorkContextPanel = lazy(() => import("../components/WorkContextPanel").then(
  (module) => ({ default: module.WorkContextPanel }),
));

/** Work 的 PDF Bridge 随视图挂载，切走后立即释放 PDF 阅读器关联状态。 */
export default function WorkView() {
  const runtime = useWorkViewRuntime();
  return (
    <PdfReaderBridgeProvider>
      <WorkWorkspace {...runtime.workspace} />
      {runtime.contextPanel ? (
        <Suspense fallback={<div className="workspace-loading" role="status">正在打开右侧面板</div>}>
          <WorkContextPanel {...runtime.contextPanel} />
        </Suspense>
      ) : null}
    </PdfReaderBridgeProvider>
  );
}
