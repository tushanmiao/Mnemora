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
export function renderMermaid(code: string, id: string, config: MermaidConfig) {
  const task = renderQueue.then(async () => {
    const { default: mermaid } = await loadMermaid();
    mermaid.initialize(config);
    await mermaid.parse(code, { suppressErrors: false });
    return mermaid.render(id, code);
  });
  renderQueue = task.then(() => undefined, () => undefined);
  return task;
}
