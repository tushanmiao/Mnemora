/**
 * 静态校验设置页的 class 与 CSS 变量。
 *
 * 两件事：
 *  1. 面板里用到的 class 是否真有定义（改版后最容易留下引用了却没定义的类名）
 *  2. CSS 里 var(--x) 的 --x 是否真被定义过（--color-success 就是这样长期失效的）
 *
 * 运行时由 JS 注入的变量列在 RUNTIME_VARS 里，不算缺失。
 *
 * 用法：node scripts/verify-settings-classes.mjs
 */
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

const ROOT = process.cwd();
const PANEL_DIR = join(ROOT, "src/features/settings/components");

/** 这些由内联 style 或脚本在运行时设置，CSS 里只读不写。 */
const RUNTIME_VARS = new Set([
  "--app-custom-background",
  "--app-surface-opacity",
  "--work-context-width",
  "--notes-context-width",
  "--sidebar-width",
  "--notes-outline-width",
  "--note-workspace-outline-width",
  "--english-mastered-angle",
  "--english-learned-angle",
  "--english-archived-angle",
  "--total-scale-factor",
  "--font-height",
]);

function readAll(dir, ext) {
  let out = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) out = out.concat(readAll(path, ext));
    else if (entry.name.endsWith(ext)) out.push({ path, text: readFileSync(path, "utf8") });
  }
  return out;
}

const rel = (path) => path.replace(ROOT, "").replace(/\\/g, "/");

// 收集全工程 CSS 里定义过的 class 与自定义属性
const definedClasses = new Set();
const definedVars = new Set();
const usedVars = new Map();

for (const { path, text } of readAll(join(ROOT, "src"), ".css")) {
  for (const m of text.matchAll(/\.(-?[_a-zA-Z][\w-]*)/g)) definedClasses.add(m[1]);
  for (const m of text.matchAll(/(--[\w-]+)\s*:/g)) definedVars.add(m[1]);
  for (const m of text.matchAll(/var\((--[\w-]+)/g)) {
    if (!usedVars.has(m[1])) usedVars.set(m[1], path);
  }
}

// 收集设置面板里用到的 class。
// 插值处换成 \x00 哨兵：`a-${k} b` → `a-\x00 b`，按空白切分后
// `a-\x00` 仍是一个 token，可识别为「含插值、无法静态判定」并跳过。
const usedClasses = new Map();
for (const { path, text } of readAll(PANEL_DIR, ".tsx")) {
  for (const m of text.matchAll(/className=(?:"([^"]*)"|\{`([^`]*)`\})/g)) {
    const tokens = (m[1] ?? m[2] ?? "")
      .replace(/\$\{[^}]*\}/g, "\x00")
      .split(/\s+/)
      .filter(Boolean);
    for (const cls of tokens) {
      if (cls.includes("\x00")) continue;
      if (!usedClasses.has(cls)) usedClasses.set(cls, path);
    }
  }
}

const missingClasses = [...usedClasses].filter(([cls]) => !definedClasses.has(cls));
const missingVars = [...usedVars].filter(([name]) => (
  !definedVars.has(name) && !RUNTIME_VARS.has(name)
));

// 设置页之外的历史遗留只警告，不判失败——否则这个守卫永远是红的，
// 也就永远不会被人当回事。
const isSettings = (path) => path.includes(`${join("features", "settings")}`);
const blocking = missingVars.filter(([, path]) => isSettings(path));
const elsewhere = missingVars.filter(([, path]) => !isSettings(path));

let failed = false;

if (missingClasses.length > 0) {
  failed = true;
  console.error(`\n未定义的 class（${missingClasses.length}）：`);
  for (const [cls, path] of missingClasses) console.error(`  .${cls}  ←  ${rel(path)}`);
}

if (blocking.length > 0) {
  failed = true;
  console.error(`\n设置页未定义的 CSS 变量（${blocking.length}）：`);
  for (const [name, path] of blocking) console.error(`  ${name}  ←  ${rel(path)}`);
}

if (elsewhere.length > 0) {
  console.warn(`\n警告：设置页之外还有 ${elsewhere.length} 个未定义变量（本次不判失败）：`);
  for (const [name, path] of elsewhere) console.warn(`  ${name}  ←  ${rel(path)}`);
}

if (!failed) {
  console.log(`\nOK：设置页 ${usedClasses.size} 个 class 全部有定义，CSS 变量无缺失。`);
}

process.exit(failed ? 1 : 0);
