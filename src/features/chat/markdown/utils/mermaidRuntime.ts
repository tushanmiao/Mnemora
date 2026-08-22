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
      const rendered = await mermaid.render(id, code, measurementHost);
      return {
        ...rendered,
        // Mermaid 在不可见/折叠容器中偶尔产生该非法变换。Cherry Studio
        // 同样在交付 SVG 前修复它，避免整个图形偏移或消失。
        svg: rendered.svg.replace(/translate\(undefined,\s*NaN\)/g, "translate(0, 0)"),
      };
    } finally {
      measurementHost.remove();
    }
  });
  renderQueue = task.then(() => undefined, () => undefined);
  return task;
}
