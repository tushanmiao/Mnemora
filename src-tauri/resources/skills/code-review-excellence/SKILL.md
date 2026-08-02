---
id: code-review-excellence
name: 代码审查
description: 以缺陷、回归、安全、性能、可维护性和测试风险为中心审查代码或差异，输出按严重程度排序且可执行的发现。
version: 1.0.0
license: MIT
compatibility: 可审查用户粘贴的代码和当前会话文本附件；Mnemora 尚未提供仓库扫描、Git 差异或自动执行测试工具。
triggers:
  - /code-review
  - /review-code
argument-hint: "<代码、变更目标或重点风险>"
recommended-tools:
  - read_attachment_text
metadata:
  mnemora:
    default-enabled: false
    supported-modes: [chat]
    risk: low
    resource-cost: low
    source-repository: https://github.com/wshobson/agents
    source-path: plugins/developer-essentials/skills/code-review-excellence/SKILL.md
    source-revision: c4b82b0ad771190355eb8e204b1329732a18449a
    attribution: "Code Review Excellence by Seth Hobson, licensed under MIT."
    adapted: true
    adaptation-notes: 保留分阶段审查、严重性排序和可执行反馈方法；移除依赖仓库命令、CI、PR 平台及特定语言的大段示例。
---

# 代码审查

审查目标是发现会影响行为、可靠性和维护成本的问题。不要把格式偏好冒充缺陷，也不要在没有代码证据时断言存在问题。

## 审查顺序

1. 理解变更目标、输入输出、调用边界和用户可见行为。
2. 先检查整体设计、状态流转和跨模块契约，再逐段检查实现。
3. 优先寻找逻辑错误、边界条件、并发问题、资源泄漏、错误处理和兼容性回归。
4. 检查输入验证、权限、敏感数据、注入、路径和不可信内容处理。
5. 检查不必要的常驻状态、重复计算、阻塞 I/O、大对象复制和无界集合。
6. 判断测试是否覆盖正常路径、边界、失败、取消和迁移场景。

## 输出合同

- 发现优先于总结，按“严重、较高、一般、建议”排序。
- 每项发现说明位置、触发条件、实际影响和建议修复方向。
- 无法确认时写成问题或假设，不写成既定事实。
- 没有发现时明确说明，并指出尚未覆盖的测试或剩余风险。
- 不要求为了个人风格重写已经清楚且正确的代码。
