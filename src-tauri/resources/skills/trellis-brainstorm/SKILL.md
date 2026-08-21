---
id: trellis-brainstorm
name: 需求收敛与方案规划
description: 在复杂功能、架构调整或需求不清晰时，先基于仓库证据澄清目标、范围、验收标准和关键取舍，再形成可执行方案；适合 Chat、Work 和深度笔记规划，不替代用户做产品决策。
version: 1.0.0
license: AGPL-3.0
compatibility: 规划与分析型 Skill；优先使用当前对话、附件和已授权工具核实事实，不虚构已执行的研究或实现。
triggers:
  - /brainstorm
  - /requirements
  - /plan
  - /trellis
metadata:
  mnemora:
    default-enabled: true
    supported-modes: [chat, work, notes]
    risk: low
    resource-cost: low
    source-repository: https://github.com/mindfold-ai/Trellis
    source-path: .agents/skills/trellis-brainstorm/SKILL.md
    source-revision: 9914a7f2b1b61c0e3890be474f771f49aa115f88
    attribution: "Trellis Brainstorm by Mindfold LLC, distributed under AGPL-3.0; Mnemora uses an independent Chinese adaptation of the workflow ideas."
    adapted: true
    adaptation-notes: 仅吸收证据优先、单问题收敛、范围与验收标准澄清、计划确认后再执行等工作流思想；未复制 Trellis 的脚本、任务目录、Agent 配置或代码。Mnemora 中的分析结果仍由当前会话和实际工具事实决定。

---

# 需求收敛与方案规划

当用户提出复杂功能、架构重构、性能优化或多个可行方案时，先把问题收敛成可以验证和执行的方案。该 Skill 的重点不是制造流程文档，而是避免在目标、边界和验收方式未明确时过早实施。

## 工作方式

1. 用一句话重述用户真正想得到的结果，区分目标、症状和实现手段。
2. 先检查当前对话、附件、代码、测试、配置和已有文档；能从已有材料确认的事实，不重复向用户提问。
3. 明确列出已确认事实、仍需用户决定的产品选择、技术未知项和暂不纳入的范围。
4. 对每个关键决策给出推荐方案、理由和代价。涉及用户偏好、范围、风险容忍度或最终行为时，不替用户擅自决定。
5. 一次只提出一个会改变方案的最高价值问题；如果没有阻塞性问题，就给出完整计划并等待用户确认。
6. 计划必须包含目标、范围、非目标、数据流或工作流、验收标准、风险和验证方法。
7. 用户确认后，才进入执行；执行结果与计划不一致时，说明差异并重新收敛，不把未完成的工作描述为已完成。

## 与 Mnemora 的边界

- 普通 Chat 中可以直接给出小型分析；只有复杂或高风险任务才展开完整规划。
- 不因为加载了本 Skill 就伪造工具调用、文件研究、测试结果或完成状态。
- 需要读取仓库、附件或运行测试时，只有实际工具返回结果才能作为事实。
- 用户明确要求立即修复且范围清晰时，可以省略形式化提问，但仍应保留最小的范围和验收判断。
- 深度生成笔记可以把规划结果转换为语义计划和 DAG 节点，但本 Skill 本身不执行章节生成，也不替代运行层调度器。

## 推荐输出结构

根据任务复杂度选择精简或完整形式。完整形式至少包括：

- 核心目标与问题本质；
- 已确认事实与证据；
- 推荐方案及关键取舍；
- 处理范围与明确不处理的内容；
- 工作流、接口或数据边界；
- 可观察的验收标准；
- 风险、未知项和回滚方式；
- 下一步需要用户确认的单个决策，或确认后执行的计划。

## 来源与许可证

本 Skill 是对 Trellis `trellis-brainstorm` 的独立适配，不包含其代码、脚本或任务系统。来源仓库及原始文件路径、固定版本和许可证信息记录在 frontmatter；Trellis 为 AGPL-3.0，任何直接复制或衍生代码的行为都必须单独进行许可证审查。
