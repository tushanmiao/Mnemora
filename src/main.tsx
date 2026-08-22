import React from "react";
import ReactDOM from "react-dom/client";
import { RootErrorBoundary } from "./bootstrap/RootErrorBoundary";
import { StartupFailure } from "./bootstrap/StartupFailure";
import {
  createStartupDiagnostic,
  installGlobalErrorCapture,
  recordStartupDiagnostic,
  setStartupStage,
} from "./bootstrap/startupDiagnostics";
import "./bootstrap/startup.css";

installGlobalErrorCapture();
const rootElement = document.getElementById("root");
if (!rootElement) throw new Error("找不到应用根节点 #root。");
const root = ReactDOM.createRoot(rootElement);

async function renderWindow() {
  setStartupStage("import-window");
  if (window.location.hash.startsWith("#html-preview/")) {
    const { default: HtmlPreviewWindow } = await import(
      "./features/html-preview/HtmlPreviewWindow"
    );
    setStartupStage("render-window");
    root.render(<RootErrorBoundary title="HTML 预览启动失败"><HtmlPreviewWindow /></RootErrorBoundary>);
    setStartupStage("ready");
    return;
  }
  if (window.location.hash === "#pet") {
    const { default: PetWindow } = await import("./features/pet/PetWindow");
    setStartupStage("render-window");
    root.render(<RootErrorBoundary title="桌面宠物启动失败"><PetWindow /></RootErrorBoundary>);
    setStartupStage("ready");
    return;
  }

  const { default: App } = await import("./App");
  setStartupStage("render-window");
  root.render(
    <React.StrictMode>
      <RootErrorBoundary>
        <App />
      </RootErrorBoundary>
    </React.StrictMode>,
  );
  setStartupStage("ready");
}

void renderWindow().catch((reason) => {
  const diagnostic = createStartupDiagnostic(reason);
  recordStartupDiagnostic(diagnostic);
  console.error("Mnemora 启动失败", diagnostic);
  root.render(<StartupFailure diagnostic={diagnostic} />);
});
