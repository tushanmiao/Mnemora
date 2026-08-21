---
id: document-authoring
name: 结构化文档撰写
description: 将研究、对话、代码分析或知识库证据组织成结构清楚、可引用、适合学习和长期维护的 Markdown 文档；适合教程、技术说明、决策记录、复盘和笔记，不负责直接写入 Office 文件。
version: 1.0.0
license: MIT
compatibility: 可使用 present_artifact 交付 Markdown/HTML；需要事实依据时先使用相应读取或检索工具。
triggers: [/write-doc, /author, /document]
argument-hint: "<文档目的、读者和已有材料>"
recommended-tools: [present_artifact, knowledge_search, knowledge_read, web_search, web_fetch]
metadata:
  mnemora:
    default-enabled: true
    supported-modes: [chat, work, notes]
    risk: low
    resource-cost: medium
    source-repository: https://github.com/github/awesome-copilot
    source-path: skills/documentation-writer/SKILL.md
    source-revision: caab1f623bb68a330f294a11279597d7ae7be737
    attribution: "Documentation authoring guidance from GitHub's awesome-copilot repository, licensed under MIT."
    adapted: true
    adaptation-notes: 适配 Mnemora 中文学习文档、零基础解释、证据引用和只读 Artifact 边界。
---

# 结构化文档撰写

1. 明确读者、使用场景和读完后应能完成什么。
2. 先建立事实清单和引用，再决定结构；不能用流畅措辞掩盖证据缺口。
3. 结论先行，再按概念、机制、流程、例子、取舍、错误与恢复组织内容。
4. 首次出现关键技术概念保留英文原名并解释中文含义。
5. 面向初学者给出完整输入到输出示例，区分“必须理解”和“以后深入”。
6. 标题层级连续，段落不过长；表格只用于确切映射或比较。
7. 引用靠近事实；外部来源、本地知识库和代码路径使用各自稳定标识。
8. 使用 `present_artifact(kind="markdown")` 时说明它是结构化交付，未自动保存到文件。
