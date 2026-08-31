// 双向验证 mermaidRepair.ts：既看修好了多少，也看弄坏了多少。
//
// 只统计「修好」是不够的——一条把 30 个坏图修好、同时把 200 个好图弄坏的规则
// 在那种统计下也会显得很成功。所以这里对每个图跑两次解析，原样一次、修复后
// 一次，然后按 4 个象限归类。broken 必须为 0 才允许上线。
//
// 用法（必须带 loader，否则 mermaid 会在无 DOM 的 node 里撞上 DOMPurify）：
//   node --import ./scripts/mermaid-loader.mjs scripts/check-mermaid-repair.mjs <目录或文件>...
import { mkdtemp, readdir, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { build } from "esbuild";

const FENCE = /^([ \t]*)```+\s*mermaid\s*$/i;
const REPAIR_SOURCE = "src/features/chat/markdown/utils/mermaidRepair.ts";

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

/** 递归收集 .md/.json 文件；用户的会话数据是 json，文档是 md。 */
async function collectFiles(target) {
  const info = await stat(target);
  if (!info.isDirectory()) return [target];
  const out = [];
  for (const entry of await readdir(target, { withFileTypes: true })) {
    if (entry.name === "node_modules" || entry.name === ".git") continue;
    const full = path.join(target, entry.name);
    if (entry.isDirectory()) out.push(...await collectFiles(full));
    else if (/\.(?:md|markdown|json)$/i.test(entry.name)) out.push(full);
  }
  return out;
}

/**
 * json 里的 mermaid 藏在字符串字段里，反序列化后再按围栏提取。任何字符串值
 * 都可能是消息正文，所以全量扫。
 */
function diagramsFromJson(text) {
  const found = [];
  const walk = (value) => {
    if (typeof value === "string") found.push(...extractDiagrams(value));
    else if (Array.isArray(value)) value.forEach(walk);
    else if (value && typeof value === "object") Object.values(value).forEach(walk);
  };
  try {
    walk(JSON.parse(text));
  } catch {
    // 不是合法 json 就当纯文本扫一遍。
    found.push(...extractDiagrams(text));
  }
  return found;
}

const kindOf = (code) => (code.trim().split(/\s+/)[0] ?? "?").replace(/[^\w-]/g, "");

const targets = process.argv.slice(2);
if (targets.length === 0) {
  console.error("用法: node --import ./scripts/mermaid-loader.mjs scripts/check-mermaid-repair.mjs <目录或文件>...");
  process.exit(2);
}

// 把 TS 修复模块编译出来，保证验证的就是生产代码本身，而不是它的副本。
const workdir = await mkdtemp(path.join(tmpdir(), "mnemora-mermaid-"));
const compiled = path.join(workdir, "mermaidRepair.mjs");
await build({
  entryPoints: [REPAIR_SOURCE],
  outfile: compiled,
  format: "esm",
  platform: "neutral",
  bundle: true,
});
const { repairMermaidSource } = await import(new URL(`file://${compiled.replace(/\\/g, "/")}`).href);

const { default: mermaid } = await import("mermaid");
mermaid.initialize({ startOnLoad: false, securityLevel: "loose", suppressErrorRendering: true });

const parses = async (code) => {
  try {
    await mermaid.parse(code);
    return true;
  } catch {
    return false;
  }
};

const buckets = { fixed: [], broken: [], stillBad: [], untouched: 0, unchangedGood: 0 };
const kinds = new Map();
let total = 0;
let touched = 0;

for (const file of (await Promise.all(targets.map(collectFiles))).flat()) {
  const text = await readFile(file, "utf8");
  const diagrams = /\.json$/i.test(file) ? diagramsFromJson(text) : extractDiagrams(text);
  for (const { line, code } of diagrams) {
    total += 1;
    const kind = kindOf(code);
    kinds.set(kind, (kinds.get(kind) ?? 0) + 1);

    const before = await parses(code);
    const { source: repaired, repairs } = repairMermaidSource(code);
    if (repairs.length === 0) {
      if (before) buckets.unchangedGood += 1;
      else buckets.stillBad.push({ file, line, kind, repairs, note: "无规则命中" });
      continue;
    }

    touched += 1;
    const after = await parses(repaired);
    const at = { file, line, kind, repairs: repairs.join("+") };
    if (!before && after) buckets.fixed.push(at);
    else if (before && !after) buckets.broken.push(at);
    else if (before && after) buckets.untouched += 1;
    else buckets.stillBad.push({ ...at, note: "修复后仍失败" });
  }
}

await rm(workdir, { recursive: true, force: true });

const summary = [...kinds].sort((a, b) => b[1] - a[1]).map(([k, n]) => `${k}×${n}`).join("，");
console.log(`扫描 ${total} 个 mermaid 图：${summary}\n`);
console.log(`规则命中 ${touched} 个，其中：`);
console.log(`  修好 (原本失败 → 现在通过)  ${buckets.fixed.length}`);
console.log(`  弄坏 (原本通过 → 现在失败)  ${buckets.broken.length}`);
console.log(`  命中但两次都通过            ${buckets.untouched}`);
console.log(`未命中规则：原本通过 ${buckets.unchangedGood}，原本失败 ${buckets.stillBad.length}\n`);

for (const item of buckets.fixed) console.log(`  已修复 ${item.file}:${item.line} (${item.kind}) [${item.repairs}]`);
for (const item of buckets.broken) console.error(`  弄坏了 ${item.file}:${item.line} (${item.kind}) [${item.repairs}]`);
for (const item of buckets.stillBad) console.log(`  仍失败 ${item.file}:${item.line} (${item.kind}) ${item.note}`);

console.log(buckets.broken.length === 0 ? "\n零破坏，规则可以上线。" : `\n有 ${buckets.broken.length} 个被弄坏，不可上线。`);
process.exit(buckets.broken.length === 0 ? 0 : 1);
