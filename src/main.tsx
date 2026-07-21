import React from "react";
import ReactDOM from "react-dom/client";

const root = ReactDOM.createRoot(document.getElementById("root") as HTMLElement);

async function renderWindow() {
  if (window.location.hash.startsWith("#html-preview/")) {
    const { default: HtmlPreviewWindow } = await import(
      "./features/html-preview/HtmlPreviewWindow"
    );
    root.render(<HtmlPreviewWindow />);
    return;
  }

  const { default: App } = await import("./App");
  root.render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
}

void renderWindow();
