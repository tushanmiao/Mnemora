---
id: markdown-notes
name: Markdown 笔记
description: 将对话和材料整理为结构清楚、可追踪来源、兼容 GFM 的 Markdown 笔记，适合知识沉淀和后续 PDF 笔记工作流。
version: 1.0.0
license: MIT
compatibility: 输出使用 Mnemora 当前可渲染的 CommonMark/GFM 子集，支持 Mermaid 图表、脚注、学习提示块和安全 HTML；不依赖 Obsidian Vault、CLI、Wikilink 或专有嵌入语法。
triggers:
  - /markdown
  - /note
argument-hint: "<笔记用途、结构或目标读者>"
metadata:
  mnemora:
    default-enabled: true
    supported-modes: [chat, notes]
    risk: low
    resource-cost: low
    source-repository: https://github.com/kepano/obsidian-skills
    source-path: skills/obsidian-markdown/SKILL.md
    source-revision: a1dc48e68138490d522c04cbf5822214c6eb1202
    attribution: "Obsidian Markdown Skill by Steph Ango (@kepano), licensed under MIT."
    adapted: true
    adaptation-notes: 将 Obsidian 专用 Markdown 工作流收敛为 Mnemora 的安全 GFM；保留并适配 Mermaid、脚注和引用，移除 Wikilink、Vault 嵌入、CSS class、脚本和其他未支持语法。
---

# Markdown 笔记

生成可以直接阅读和继续编辑的 Markdown，不声称已经写入知识库或本地文件。

## 工作流程

1. 明确笔记用途：速记、学习卡片、概念说明、会议记录、决策记录或资料摘录。
2. 提取主题、定义、事实、证据、观点、决定、行动项和未解决问题。
3. 合并真正重复的内容，保留相互冲突的说法及各自来源。
4. 使用标题、列表、引用、表格、任务列表和代码块组织内容。
5. 保留文档名、页码、链接或消息上下文；没有来源时不要编造。
6. 需要表达流程、时序或结构关系时，使用 `mermaid` 代码块；Mnemora 会默认渲染图表，同时保留切换回源码的入口。
7. 当输出会直接作为笔记正文保存时，不要用 `markdown`、`md`、`text` 或四反引号包裹整份正文；真实 Mermaid 必须是正文顶层代码块。只有用户明确要求展示 Markdown 源码示例时，才使用外层源码围栏。

## 格式规则

- 一个章节只承担一个主要主题，标题应准确描述内容。
- 表格只用于需要横向比较的数据，长段落不要强行放进表格。
- 代码、命令、路径和标识符使用反引号；多行代码使用带语言标记的代码块。
- 不使用依赖特定笔记软件的 Wikilink、嵌入、CSS class 或脚本；不要在 Markdown 中生成事件属性、`javascript:` 链接或外部 iframe。
- 默认结构为“摘要、核心概念、关键事实与证据、结论或决定、待办、来源”。
