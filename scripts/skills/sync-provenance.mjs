#!/usr/bin/env node
/**
 * 从每个 Skill 的 mnemora.json 生成来源声明，杜绝手工维护导致的漂移。
 *
 * mnemora.json 是唯一事实来源，本脚本产出两样东西：
 *   1. 每个 Skill 目录下的 SOURCE.md（THIRD_PARTY_NOTICES.md 承诺过它存在）
 *   2. THIRD_PARTY_NOTICES.md 里 <!-- generated --> 标记之间的表格
 *
 * 用法：
 *   node scripts/skills/sync-provenance.mjs          写入
 *   node scripts/skills/sync-provenance.mjs --check  只校验，CI 用；有漂移则退出码 1
 */
import { readdirSync, readFileSync, writeFileSync, statSync, existsSync } from "node:fs";
import { createHash } from "node:crypto";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const SKILLS_DIR = join(dirname(fileURLToPath(import.meta.url)), "../../src-tauri/resources/skills");
const NOTICES = join(SKILLS_DIR, "THIRD_PARTY_NOTICES.md");
const BEGIN = "<!-- generated:provenance-table -->";
const END = "<!-- /generated:provenance-table -->";
const checkOnly = process.argv.includes("--check");

const problems = [];
const skills = [];

for (const id of readdirSync(SKILLS_DIR).sort()) {
  const dir = join(SKILLS_DIR, id);
  if (!statSync(dir).isDirectory()) continue;

  const manifestPath = join(dir, "mnemora.json");
  if (!existsSync(manifestPath)) {
    // 空目录或半删除的残留：既没有清单也无法声明来源，必须显式暴露出来。
    problems.push(`${id}：没有 mnemora.json（目录内 ${readdirSync(dir).length} 个文件），无法生成来源声明`);
    continue;
  }
  if (!existsSync(join(dir, "SKILL.md"))) problems.push(`${id}：缺少 SKILL.md`);

  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  const p = manifest.provenance ?? {};
  // 自有 Skill 没有上游，只需要说明它是自有的；带上游的必须钉死 repo/path/commit。
  if (!p.firstParty) {
    for (const field of ["repository", "path", "revision"]) {
      if (!p[field]) problems.push(`${id}：provenance.${field} 为空`);
    }
  }
  if (!manifest.license) problems.push(`${id}：license 为空`);

  skills.push({ id, dir, license: manifest.license, ...p, title: readTitle(dir) });
}

// 同一份 SKILL.md 被收录两次会让模型看到两条一模一样的描述，无从选择。
// markdown-notes 与 obsidian-markdown 曾经就是逐字节重复的一对。
const byHash = new Map();
for (const s of skills) {
  const hash = createHash("sha256").update(readFileSync(join(s.dir, "SKILL.md"))).digest("hex");
  if (byHash.has(hash)) {
    problems.push(`${byHash.get(hash)} 与 ${s.id} 的 SKILL.md 内容完全相同，应只保留一个`);
  } else {
    byHash.set(hash, s.id);
  }
}

// 触发词撞车会让 Slash 命令指向不确定的 Skill。
const triggerOwner = new Map();
for (const id of readdirSync(SKILLS_DIR).sort()) {
  const manifestPath = join(SKILLS_DIR, id, "mnemora.json");
  if (!existsSync(manifestPath)) continue;
  for (const trigger of JSON.parse(readFileSync(manifestPath, "utf8")).triggers ?? []) {
    if (triggerOwner.has(trigger)) {
      problems.push(`触发词 ${trigger} 同时属于 ${triggerOwner.get(trigger)} 和 ${id}`);
    } else {
      triggerOwner.set(trigger, id);
    }
  }
}

function readTitle(dir) {
  const text = readFileSync(join(dir, "SKILL.md"), "utf8");
  const front = text.match(/^---\r?\n([\s\S]*?)\r?\n---/);
  const name = front?.[1].match(/^name:\s*(.+)$/m)?.[1]?.trim();
  return (name ?? "").replace(/^["']|["']$/g, "");
}

function sourceMarkdown(s) {
  if (s.firstParty) {
    return `# 来源声明：${s.id}

> 本文件由 \`scripts/skills/sync-provenance.mjs\` 从 \`mnemora.json\` 生成，请勿手工编辑。

| 项目 | 内容 |
|---|---|
| 上游仓库 | 无，Mnemora 自有 Skill |
| 许可证 | ${s.license} |

${s.attribution ?? ""}
`;
  }
  const shortRepo = s.repository.replace("https://github.com/", "");
  return `# 来源声明：${s.id}

> 本文件由 \`scripts/skills/sync-provenance.mjs\` 从 \`mnemora.json\` 生成，请勿手工编辑。

| 项目 | 内容 |
|---|---|
| 上游仓库 | [${shortRepo}](${s.repository}) |
| 原始路径 | \`${s.path}\` |
| 固定 Commit | \`${s.revision}\` |
| 许可证 | ${s.license} |
| 是否改编 | ${s.adapted ? "是" : "否，按上游原样收录"} |

${s.attribution ?? ""}

许可证全文见同目录的 \`LICENSE.txt\`。升级此 Skill 时请同步更新 \`mnemora.json\` 的 \`provenance.revision\`，
然后重新运行生成脚本，不要单独修改本文件或 \`THIRD_PARTY_NOTICES.md\` 的表格。
`;
}

function noticesTable() {
  const head = "| Skill | 名称 | 上游项目 | 原始路径 | 固定 Commit | 许可证 |\n|---|---|---|---|---|---|";
  const rows = skills.map((s) => {
    if (s.firstParty) {
      return `| \`${s.id}\` | ${s.title || "—"} | Mnemora 自有 | — | — | ${s.license} |`;
    }
    const shortRepo = s.repository.replace("https://github.com/", "");
    return `| \`${s.id}\` | ${s.title || "—"} | [${shortRepo}](${s.repository}) | \`${s.path}\` | \`${s.revision.slice(0, 12)}\` | ${s.license}${s.adapted ? " · 已适配" : ""} |`;
  });
  return [BEGIN, `共 ${skills.length} 个内置 Skill。本表由脚本生成，请勿手工编辑。`, "", head, ...rows, END].join("\n");
}

let drifted = false;
for (const s of skills) {
  const target = join(s.dir, "SOURCE.md");
  const next = sourceMarkdown(s);
  const current = existsSync(target) ? readFileSync(target, "utf8") : null;
  if (current !== next) {
    drifted = true;
    if (!checkOnly) writeFileSync(target, next, "utf8");
    console.log(`${checkOnly ? "漂移" : "写入"} ${s.id}/SOURCE.md`);
  }
}

const notices = readFileSync(NOTICES, "utf8");
const table = noticesTable();
let nextNotices;
if (notices.includes(BEGIN) && notices.includes(END)) {
  nextNotices = notices.replace(new RegExp(`${BEGIN}[\\s\\S]*?${END}`), table);
} else {
  // 首次运行：把旧的手工表整体替换成生成块。
  const before = notices.slice(0, notices.indexOf("| Mnemora Skill"));
  const afterIndex = notices.indexOf("## 未纳入的候选");
  const after = afterIndex >= 0 ? notices.slice(afterIndex) : "";
  nextNotices = `${before}${table}\n\n${after}`;
}
if (nextNotices !== notices) {
  drifted = true;
  if (!checkOnly) writeFileSync(NOTICES, nextNotices, "utf8");
  console.log(`${checkOnly ? "漂移" : "写入"} THIRD_PARTY_NOTICES.md`);
}

if (problems.length > 0) {
  console.error("\n发现问题：");
  for (const line of problems) console.error(`  - ${line}`);
}
if (checkOnly && (drifted || problems.length > 0)) {
  console.error("\n来源声明与 mnemora.json 不一致，请运行 node scripts/skills/sync-provenance.mjs");
  process.exit(1);
}
if (problems.length > 0) process.exit(1);
console.log(`\n完成：${skills.length} 个 Skill 的来源声明已同步。`);
