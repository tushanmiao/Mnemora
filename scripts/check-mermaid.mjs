// 校验 markdown 里的 mermaid 代码块能否被 mermaid 自己的解析器接受。
//
// 用法（必须带 loader，否则 mermaid 会在无 DOM 的 node 里撞上 DOMPurify）：
//   node --import ./scripts/mermaid-loader.mjs scripts/check-mermaid.mjs <file.md>...
import { readFile } from "node:fs/promises";

const FENCE = /^([ \t]*)```+\s*mermaid\s*$/i;

function extractDiagrams(text) {
  const lines = text.split(/\r?\n/);
  const found = [];
  for (let i = 0; i < lines.length; i += 1) {
    const open = FENCE.exec(lines[i]);
    if (!open) continue;
    const indent = open[1];
    const body = [];
    let j = i + 1;
    for (; j < lines.length; j += 1) {
      if (new RegExp(`^${indent}\`\`\`+\\s*$`).test(lines[j])) break;
      body.push(lines[j].startsWith(indent) ? lines[j].slice(indent.length) : lines[j]);
    }
    found.push({ line: i + 1, code: body.join("\n") });
    i = j;
  }
  return found;
}

const kindOf = (code) => (code.trim().split(/\s+/)[0] ?? "?").replace(/[^\w-]/g, "");

const files = process.argv.slice(2);
if (files.length === 0) {
  console.error("用法: node --import ./scripts/mermaid-loader.mjs scripts/check-mermaid.mjs <file.md>...");
  process.exit(2);
}

const { default: mermaid } = await import("mermaid");
mermaid.initialize({ startOnLoad: false, securityLevel: "loose", suppressErrorRendering: true });

let total = 0;
let failed = 0;
const kinds = new Map();

for (const file of files) {
  const diagrams = extractDiagrams(await readFile(file, "utf8"));
  for (const { line, code } of diagrams) {
    total += 1;
    const kind = kindOf(code);
    kinds.set(kind, (kinds.get(kind) ?? 0) + 1);
    try {
      await mermaid.parse(code);
    } catch (error) {
      failed += 1;
      const message = error?.message ?? String(error);
      console.error(`${file}:${line} (${kind}) 解析失败:\n${message}\n`);
    }
  }
}

const summary = [...kinds].map(([k, n]) => `${k}×${n}`).join("，");
console.log(`共 ${total} 个 mermaid 图：${summary}`);
console.log(failed === 0 ? "全部通过。" : `失败 ${failed} 个。`);
process.exit(failed === 0 ? 0 : 1);
