// 校验 md/Summary 文档里的 `file.rs:行号` 引用是否还指向它声称的东西。
//
// 文档里的行号是最容易腐烂的部分：代码一改就漂，而读者（尤其是面试时的追问）
// 会直接按行号翻代码。这个脚本把每条引用取出来，打印它当前指向的首行，人工
// 一眼就能看出是不是还对得上。
//
// 用法：node scripts/check-doc-line-refs.mjs [文档路径...]
import { readFile } from "node:fs/promises";

const docs = process.argv.slice(2);
if (docs.length === 0) {
  console.error("用法：node scripts/check-doc-line-refs.mjs md/Summary/01-深度笔记.md ...");
  process.exit(2);
}

// 文档里出现过的源文件名 → 仓库内实际路径。多个根目录按顺序试，命中即止。
const ROOTS = [
  "src-tauri/src/chat/note_pipeline/",
  "src-tauri/src/library/",
  "src-tauri/src/",
  "src/features/chat/notePipeline/",
  "src/features/settings/components/",
  "src/features/tasks/projections/",
  "src/features/workspace/runtime/",
  "src/app/hooks/",
];

const cache = new Map();

/** 读取一个候选路径，失败返回 null。 */
async function tryPath(path) {
  if (cache.has(path)) return cache.get(path);
  let entry = null;
  try {
    const text = await readFile(path, "utf8");
    entry = { path, lines: text.split(/\r?\n/) };
  } catch {
    entry = null;
  }
  cache.set(path, entry);
  return entry;
}

/**
 * 解析引用指向的源文件。
 *
 * `linkTarget` 来自 markdown 链接，相对文档所在目录；给了就只认它，因为那是
 * 作者写明的路径。没给才退回按文件名在 ROOTS 里搜——那是有歧义的猜测。
 */
async function readSource(name, linkTarget, docPath) {
  if (linkTarget && !/^https?:/.test(linkTarget)) {
    const docDir = docPath.replace(/\\/g, "/").replace(/\/[^/]*$/, "");
    const joined = new URL(linkTarget, `file:///${docDir}/`).pathname.replace(/^\//, "");
    const entry = await tryPath(joined);
    if (entry) return { ...entry, resolvedBy: "link" };
    return null;
  }
  for (const root of ROOTS) {
    const entry = await tryPath(root + name);
    if (entry) return { ...entry, resolvedBy: "guess" };
  }
  return null;
}

/**
 * 两种写法都要认：
 *   1. 裸引用   `service.rs:3772-3895`
 *   2. 带链接   [`types.rs:116`](../../src-tauri/src/mcp/types.rs)
 *
 * 第 2 种自带路径，必须优先采用——仓库里有 16 个 types.rs、7 个 repository.rs，
 * 只按文件名猜会挑错文件，然后报出一堆假失效。
 */
const REF = /\[?`([A-Za-z_][A-Za-z0-9_]*\.(?:rs|ts|tsx)):(\d+)(?:-(\d+))?`(?:\]\(([^)]+)\))?/g;
let checked = 0;
let broken = 0;

for (const doc of docs) {
  const text = await readFile(doc, "utf8");
  const lines = text.split(/\r?\n/);
  const seen = new Map(); // "name:start-end" → 文档中首次出现的行

  lines.forEach((line, index) => {
    for (const match of line.matchAll(REF)) {
      const [, name, startRaw, endRaw, linkTarget] = match;
      // 同一处引用可能在文档里出现多次；按「文件名:范围 + 路径」去重。
      const key = `${name}:${startRaw}${endRaw ? `-${endRaw}` : ""}`;
      if (!seen.has(key) || (linkTarget && !seen.get(key).linkTarget)) {
        seen.set(key, { docLine: index + 1, name, startRaw, endRaw, linkTarget });
      }
    }
  });

  console.log(`\n=== ${doc}（${seen.size} 条唯一引用）===`);
  for (const [key, ref] of seen) {
    const start = Number(ref.startRaw);
    const end = ref.endRaw ? Number(ref.endRaw) : start;
    const source = await readSource(ref.name, ref.linkTarget, doc);
    checked += 1;

    if (!source) {
      broken += 1;
      const where = ref.linkTarget ? `链接指向 ${ref.linkTarget}` : "按文件名未找到";
      console.log(`  ✗ ${key.padEnd(28)} 文档:${ref.docLine}  ${where}`);
      continue;
    }
    if (start > source.lines.length || end > source.lines.length) {
      broken += 1;
      console.log(`  ✗ ${key.padEnd(28)} 文档:${ref.docLine}  超出文件长度 ${source.lines.length}`);
      continue;
    }
    const head = (source.lines[start - 1] ?? "").trim().slice(0, 76);
    const mark = source.resolvedBy === "guess" ? "?" : "·";
    console.log(`  ${mark} ${key.padEnd(28)} 文档:${String(ref.docLine).padEnd(5)} → ${head}`);
  }
}

console.log(`\n共 ${checked} 条引用，${broken} 条明确失效（越界或文件缺失）。`);
console.log("标记 ? 的是按文件名猜的路径（仓库里有同名文件，可能猜错）；· 是文档写明了链接路径。");
console.log("首行内容需人工确认是否仍与文档叙述一致——行号在范围内也可能已经指向别的代码。");
process.exit(broken > 0 ? 1 : 0);
