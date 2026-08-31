// 把真实语料里的 mermaid 图连同「原样能否解析」的判定导出成 JSON，
// 交给 Rust 侧的 lint 规则做误报率验证。
//
//   node --import ./scripts/mermaid-loader.mjs scripts/dump-mermaid-corpus.mjs <输出文件> <目录或文件>...
import { readdir, readFile, stat, writeFile } from "node:fs/promises";
import path from "node:path";

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

async function collectFiles(target) {
  const info = await stat(target);
  if (!info.isDirectory()) return [target];
  const out = [];
  for (const entry of await readdir(target, { withFileTypes: true })) {
    if (entry.name === "node_modules" || entry.name === ".git") continue;
    const full = path.join(target, entry.name);
    if (entry.isDirectory()) out.push(...(await collectFiles(full)));
    else if (/\.(?:md|markdown|json)$/i.test(entry.name)) out.push(full);
  }
  return out;
}

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
    found.push(...extractDiagrams(text));
  }
  return found;
}

const [outFile, ...targets] = process.argv.slice(2);
if (!outFile || targets.length === 0) {
  console.error("用法: node --import ./scripts/mermaid-loader.mjs scripts/dump-mermaid-corpus.mjs <输出文件> <目录或文件>...");
  process.exit(2);
}

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

const corpus = [];
for (const file of (await Promise.all(targets.map(collectFiles))).flat()) {
  const text = await readFile(file, "utf8");
  const diagrams = /\.json$/i.test(file) ? diagramsFromJson(text) : extractDiagrams(text);
  for (const { line, code } of diagrams) {
    corpus.push({ file, line, code, parses: await parses(code) });
  }
}

await writeFile(outFile, JSON.stringify(corpus, null, 2), "utf8");
console.log(`导出 ${corpus.length} 个图到 ${outFile}（原样可解析 ${corpus.filter((d) => d.parses).length}）`);
