---
id: workspace-code-reading
name: 工作区代码阅读
description: 在用户配置的工作目录内建立项目地图、搜索符号、读取最小代码范围并解释调用关系，适合理解仓库架构、功能入口、数据流和具体实现；默认是解读，不自动转为 Code Review。
version: 1.0.0
license: MIT
compatibility: 需要工作目录设置及 workspace_list、workspace_glob、workspace_search、workspace_read；仅支持有界只读分析。
triggers: [/workspace, /codebase, /repo]
argument-hint: "<要理解的模块、功能、文件或调用关系>"
required-tools: [workspace_search, workspace_read]
recommended-tools: [workspace_list, workspace_glob, present_artifact]
metadata:
  mnemora:
    default-enabled: true
    supported-modes: [chat, work, notes]
    risk: low
    resource-cost: medium
    source-repository: https://github.com/github/awesome-copilot
    source-path: skills/acquire-codebase-knowledge/SKILL.md
    source-revision: b8f38227480c9f3fe04d6496d4fcab9a880e5a15
    attribution: "Acquire Codebase Knowledge skill from GitHub's awesome-copilot repository, licensed under MIT."
    adapted: true
    adaptation-notes: 扩展现有附件代码解读为只读工作区级发现；保留证据追踪、项目地图和未知项标记，不加入任意 Shell 或写入。
---

# 工作区代码阅读

## 工作流

1. 先用 `workspace_list` 或 `workspace_glob` 了解最小目录范围，不递归倾倒整个仓库。
2. 用 `workspace_search` 找入口、类型、调用方、配置键或错误文本。
3. 用 `workspace_read` 读取定义及必要调用上下文；每个判断引用文件与行号。
4. 按“入口 → 数据结构 → 核心处理 → 外部边界 → 输出/状态”建立数据流。
5. 区分代码事实、命名暗示和推断；没有读取到的实现不得假设存在。
6. 面向初学者先讲它解决什么问题，再解释模块和术语。
7. 除非用户明确要求审查，否则不按严重级别罗列缺陷；普通请求以理解为目标。

工作区工具拒绝 `.env`、私钥和凭据，跳过依赖与构建目录；遇到这些边界应说明限制，不尝试绕过。
