import type { MermaidConfig } from "mermaid";

type MermaidModule = typeof import("mermaid");

let modulePromise: Promise<MermaidModule> | undefined;
let renderQueue: Promise<void> = Promise.resolve();

function loadMermaid() {
  modulePromise ??= import("mermaid");
  return modulePromise;
}

/** Mermaid uses process-wide configuration, so initialize/parse/render must be
 * one serialized operation. This prevents concurrent diagrams from swapping
 * light/dark variables while another SVG is being produced. */
export function renderMermaid(code: string, id: string, config: MermaidConfig, containerWidth: number) {
  const task = renderQueue.then(async () => {
    const { default: mermaid } = await loadMermaid();
    const measurementHost = document.createElement("div");
    measurementHost.setAttribute("aria-hidden", "true");
    measurementHost.style.cssText = [
      "position:fixed",
      "inset:auto auto 0 -100000px",
      `width:${Math.max(280, Math.round(containerWidth))}px`,
      "height:auto",
      "overflow:visible",
      "visibility:hidden",
      "pointer-events:none",
    ].join(";");
    document.body.appendChild(measurementHost);
    mermaid.initialize(config);
    try {
      await mermaid.parse(code, { suppressErrors: false });
      return await mermaid.render(id, code, measurementHost);
    } finally {
      measurementHost.remove();
    }
  });
  renderQueue = task.then(() => undefined, () => undefined);
  return task;
}
