// 模块解析钩子（跑在独立的 hooks 线程里）：把 mermaid 依赖的 dompurify
// 换成 mermaid-dompurify-stub.mjs。只服务于 scripts/ 里的 mermaid 校验脚本。
const stubUrl = new URL("./mermaid-dompurify-stub.mjs", import.meta.url).href;

export async function resolve(specifier, context, nextResolve) {
  if (specifier === "dompurify") {
    return { url: stubUrl, shortCircuit: true, format: "module" };
  }
  return nextResolve(specifier, context);
}
