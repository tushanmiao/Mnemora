---
id: knowledge-base-retrieval
name: 本地知识库检索
description: 从 Mnemora 的本地笔记和文献库中查找、读取、比较并引用证据，适合“在我的笔记里找”“根据文献库回答”“对比已有材料”等请求；不把无结果伪装成不存在相关知识。
version: 1.0.0
license: MIT
compatibility: 需要 knowledge_list、knowledge_search 与 knowledge_read；当前检索是有界词法检索，不等同于向量语义检索。
triggers: [/knowledge, /kb, /library]
argument-hint: "<要在本地知识库中查找的问题>"
required-tools: [knowledge_search, knowledge_read]
recommended-tools: [knowledge_list, present_artifact]
metadata:
  mnemora:
    default-enabled: true
    supported-modes: [chat, work, notes]
    risk: low
    resource-cost: medium
    source-repository: https://github.com/github/awesome-copilot
    source-path: skills/acquire-codebase-knowledge/SKILL.md
    source-revision: b8f38227480c9f3fe04d6496d4fcab9a880e5a15
    attribution: "Knowledge retrieval workflow independently adapted from GitHub awesome-copilot research skills, MIT."
    adapted: true
    adaptation-notes: 绑定 Mnemora 本地笔记、文献和稳定引用合同；加入无结果、无文本层与有界读取边界。
---

# 本地知识库检索

1. 先用 `knowledge_search` 找候选，必要时用 `knowledge_list` 了解目录。
2. 只读取与问题直接相关的笔记行或 PDF 页，不能根据标题、文件名或摘要假装读过正文。
3. 每个关键主张附上 `[knowledge:...]` 引用；笔记与文献来源分开描述。
4. 搜索返回 `successNoResults` 时说明“本次词法检索没有命中”，不要断言知识库绝对没有相关内容。
5. PDF 页没有文本层时明确说明不能提取，不能根据上下文猜页面内容。
6. 多来源冲突时分别呈现，并说明哪一条更直接、更近期或证据更强。
7. 当前工具不是向量检索；同义词问题可改写少量查询，但避免无限搜索。
