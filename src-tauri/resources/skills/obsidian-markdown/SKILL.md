---
id: obsidian-markdown
name: Obsidian Markdown
description: 编写或整理兼容 Obsidian 的 Markdown，包括属性、内部链接、嵌入、提示块、标签、数学公式和 Mermaid；用于用户明确提及 Obsidian、双链或 Vault 笔记格式的场景。
version: 1.0.0
license: MIT
compatibility: 不直接访问 Obsidian Vault；可结合工作区读取工具理解现有笔记，并用 present_artifact 交付内容。
triggers: [/obsidian, /obsidian-markdown]
argument-hint: "<要创建、转换或解释的 Obsidian 笔记>"
recommended-tools: [workspace_search, workspace_read, present_artifact]
metadata:
  mnemora:
    default-enabled: true
    supported-modes: [chat, work, notes]
    risk: low
    resource-cost: low
    source-repository: https://github.com/kepano/obsidian-skills
    source-path: skills/obsidian-markdown/SKILL.md
    source-revision: a1dc48e68138490d522c04cbf5822214c6eb1202
    attribution: "Obsidian Markdown skill by kepano/obsidian-skills, licensed under MIT."
    adapted: true
    adaptation-notes: 适配 Mnemora 只读工作区工具和结构化 Artifact；保留 Obsidian 属性、链接、嵌入、Callout 与标签语法边界。
---

# Obsidian Markdown

生成标准 Markdown，并在用户确实需要时使用 Obsidian 扩展语法。

- 属性放在文档开头 YAML frontmatter；属性名稳定、值类型一致。
- 内部链接使用 `[[页面]]`、`[[页面#标题]]` 或 `[[页面|别名]]`。
- 嵌入使用 `![[页面]]` 或 `![[附件]]`；不要虚构不存在的目标。
- Callout 使用 `> [!type] 标题`，正文保持引用块缩进。
- 标签使用可读层级，如 `#主题/子主题`，避免同义标签泛滥。
- 数学、代码和 Mermaid 使用普通 fenced code block。

读取既有 Vault 时先用 `workspace_search` 找目标，再用 `workspace_read` 读取最小范围；不要扫描或改写整个 Vault。没有写入工具时，应交付可复制的 Markdown，并明确未写入磁盘。
