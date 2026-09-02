// 检查 src/ 下的 CSS 是否引用了没有定义的自定义属性。
//
// 起因：新写的样式里出现过 `--color-text-primary` 和 `--color-text-muted` 两个
// 拼错的令牌。CSS 对未定义的 var() 完全静默 —— 颜色直接落成继承值，tsc 和
// vitest 都看不见，只能靠人眼在界面上撞见。所以把它变成一条可运行的检查。
//
// 用法：node scripts/check-css-tokens.mjs
import fs from "node:fs";
import path from "node:path";

const ROOT = "src";

/** 收集 src/ 下所有 CSS 文件，路径统一成正斜杠，Windows 上也能按同一套键查表。 */
function collectStylesheets(directory, collected = []) {
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const full = path.join(directory, entry.name);
    if (entry.isDirectory()) collectStylesheets(full, collected);
    else if (entry.name.endsWith(".css")) collected.push(full.split(path.sep).join("/"));
  }
  return collected;
}

/** 收集 src/ 下所有 TS/TSX，用来找运行时注入的自定义属性。 */
function collectScripts(directory, collected = []) {
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const full = path.join(directory, entry.name);
    if (entry.isDirectory()) collectScripts(full, collected);
    else if (/\.tsx?$/.test(entry.name)) collected.push(full.split(path.sep).join("/"));
  }
  return collected;
}

const stylesheets = collectStylesheets(ROOT);
const defined = new Set();
const sources = new Map();

for (const file of stylesheets) {
  const source = fs.readFileSync(file, "utf8");
  sources.set(file, source);
  for (const match of source.matchAll(/(--[a-zA-Z0-9-]+)\s*:/g)) defined.add(match[1]);
}

// 一部分属性由 JS 在运行时写进 style 上（布局宽度、进度角度、主题背景），CSS 里
// 找不到定义是正常的。把 TS/TSX 里的赋值一并算作定义，否则检查会淹没在噪声里。
for (const file of collectScripts(ROOT)) {
  const source = fs.readFileSync(file, "utf8");
  for (const match of source.matchAll(/["'`](--[a-zA-Z0-9-]+)["'`]/g)) defined.add(match[1]);
}

// pdf.js 在文本层的每个 span 上设置这两个属性，我们的样式只是消费它们。
for (const token of ["--total-scale-factor", "--font-height", "--scale-x", "--min-font-size-inv"]) {
  defined.add(token);
}

const problems = [];
for (const [file, source] of sources) {
  const missing = new Set();
  for (const match of source.matchAll(/var\((--[a-zA-Z0-9-]+)/g)) {
    if (!defined.has(match[1])) missing.add(match[1]);
  }
  // 引用处才是有用的定位信息，所以报行号而不是只报文件。
  for (const token of missing) {
    const line = source.slice(0, source.indexOf(`var(${token}`)).split("\n").length;
    problems.push(`${file}:${line} 引用了未定义的 ${token}`);
  }
}

console.log(`扫描 ${stylesheets.length} 个样式表，已定义 ${defined.size} 个自定义属性。`);
if (problems.length === 0) {
  console.log("未发现未定义的 var() 引用。");
  process.exit(0);
}
for (const problem of problems) console.error(problem);
process.exit(1);
