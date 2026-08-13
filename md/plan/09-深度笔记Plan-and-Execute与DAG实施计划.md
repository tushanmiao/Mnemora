# Mnemora Plan 09：深度笔记 Plan-and-Execute 与受约束 DAG 实施计划

> 用途：将本轮关于深度生成笔记、规划模式、证据系统、DAG 调度、技能加载、恢复和质量门禁的全部共识，整理为第一版正式实施依据。
>
> 状态：架构决策已确认，尚未开始编码实施。
>
> 文档语言：中文；关键技术概念首次出现时保留英文原名。
>
> 适用范围：Mnemora 的“深度生成笔记”专用工作流，不改变普通 Chat、普通 Markdown/JSON 导出和现有通用 Agent Tool Loop 的基本职责。

---

## 一、最终结论

深度笔记采用：

> **Plan-and-Execute 作为外层生命周期，受约束的 DAG（Directed Acyclic Graph，有向无环图）作为内部执行表示，借鉴 Claude Code/Codex 的 Plan Mode 作为用户审核和执行前确认机制。**

三者职责不同，不能互相替代：

| 机制 | 负责解决的问题 |
| --- | --- |
| Plan-and-Execute | 何时规划、执行、观察、验证、重规划和结束 |
| DAG | 节点之间的依赖关系、就绪判断和受控并行 |
| Plan Mode | 执行有副作用的正式生成前，如何让用户检查和确认计划 |
| Skill | Planner 应采用什么方法论、澄清问题和质量标准 |

目标流程：

```text
用户发起深度笔记
    ↓
模型能力与输入附件预检
    ↓
固定 Input Snapshot（输入快照）
    ↓
有界、只读的材料侦察
    ↓
当前模型生成结构化 DeepNote Plan
    ↓
用户审核、编辑或进行规划访谈
    ↓
Rust 将语义计划编译成受约束 DAG
    ↓
用户确认并锁定 Plan Version
    ↓
Evidence 提取 → Ledger 更新 → 章节执行
    ↓
确定性验证 → 条件式语义审查 → 最多 5 次局部修订
    ↓
必要时局部 Replan；实质变更再次请求确认
    ↓
全局验证 → 局部补丁 → 组装 → Markdown + Sidecar 持久化
```

第一版不引入独立 Planner Model，不自动切换 Provider，不静默降级为简版笔记，也不把任意自然语言 DAG 直接交给模型执行。

---

## 二、必须解决的核心问题

表面问题是“如何把对话生成得更完整”，真正的问题是：

> 模型提出的计划，能否成为运行层可以验证、暂停、恢复、重试、审计和安全写入的真实执行合同？

当前深度笔记管线已经有“分析 → 提纲 → 用户确认 → 逐章生成 → 组装 → 保存”的雏形，但仍存在以下差距：

1. 提纲主要是标题和简介，缺少章节依赖、成功标准、证据要求和影响范围。
2. 章节返回非空文本后就可能被视为完成，不能证明核心论断已经获得证据支持。
3. 当前章节通过前一章末尾摘要维持线性连续性，无法表达独立章节、共享证据和条件依赖。
4. 没有统一的 Evidence Artifact，模型可能生成看似合理但无法定位的页码、消息 ID 或引用。
5. 没有贯穿 Run 的 Note Ledger，容易出现术语不一致、事实矛盾和跨章节重复。
6. 规划阶段和正式执行阶段的边界不够明确，无法保证用户确认前不产生最终写入。
7. 章节失败、网络中断、应用重启或模型切换后，无法以节点级检查点安全恢复。
8. 提纲解析失败时的自动简版降级会改变用户选择的任务类型。
9. 普通导出和深度生成笔记的职责容易混淆，导致不必要的模型调用。
10. 前端工作流如果直接承担状态真相，会造成页面切换、内存释放和恢复时的不一致。

---

## 三、已确认的不可变产品原则

### 3.1 入口和模型

1. 深度笔记是专用工作流，不增加“普通 Chat / Agent”模式开关。
2. Planner、Writer、Reviewer、Replanner 默认全部使用用户当前选择的模型。
3. 运行层不自动切换模型或 Provider。
4. 模型能力由设置页显式配置和内置模型数据库共同决定，未知能力不得伪造支持。
5. 纯文本对话生成不强制要求 Tool；涉及文档、图片或动态工具能力时，必须执行能力门禁。
6. 发现当前模型缺少能力时，在正式启动前明确提示缺失项，并提供“切换模型、移除附件或取消”等选择；切换由用户主动完成。
7. 模型不支持原生 Structured Outputs 时仍可尝试严格 JSON，但必须经过 Rust Schema 校验。
8. 结构化输出连续达到格式尝试预算仍失败时暂停 Run，不换模型、不降级、不猜测。

### 3.2 计划与用户确认

1. 所有深度笔记正式执行前必须经过一次计划确认。
2. 快速规划默认直接生成计划；规划访谈为可选入口。
3. 规划访谈借鉴 `grill-me`：一次只问一个会实质改变结果的问题，并给出推荐答案和理由。
4. 用户编辑语义计划，不直接编辑底层 DAG。
5. 用户确认后锁定 `Plan Version`；旧模型响应不得覆盖新版本。
6. 局部重规划可以自动执行并记录原因；实质性变更必须暂停并重新请求用户确认。
7. 实质性变更包括新增/删除章节、扩大来源范围、改变笔记用途、降低证据标准、引入新的 AI 补充边界或明显增加生成规模。

### 3.3 证据、事实和质量

1. 证据冲突必须显式保留，模型不得强行制造统一结论。
2. 来源事实、数据、观点和结论必须绑定真实 Evidence ID。
3. 一般解释不要求逐句引用，但核心论断必须满足证据要求。
4. AI 补充背景可以没有原材料证据，但必须明确标记，不能伪装成文献结论。
5. 确定性规则检查优先于 LLM Critic；不对每个章节强制调用 Reviewer。
6. 章节语义修订最多 5 次；仍不合格则失败、阻塞或请求用户处理。
7. 全局语义审查只能产生局部补丁，不允许自动重写整篇笔记。
8. 没有用户确认、真实证据或通过验证时，不能写入最终笔记。

### 3.4 生命周期、数据和资源

1. 深度笔记使用固定输入快照；新消息、附件或原笔记变化不会静默进入当前 Run。
2. 用户显式纳入新资料后，创建新的 Plan Version，并只重算受影响节点。
3. Rust 编排器和持久化状态是运行真相，React 只负责命令、订阅和投影。
4. 运行中只读预览；用户编辑前必须暂停 Run，并创建用户修订版本。
5. 用户手写内容拥有最高优先级，模型不得静默覆盖。
6. 节点级检查点、幂等写入和输入 Hash 保证应用重启后可恢复。
7. 轻量状态长期保存，重型中间产物按需加载，完成后仅清理无引用或可重建内容。
8. 全局同一时间只允许一个 Run 进入正式执行；其他 Run 可以规划、暂停、查看或排队。
9. 单 Run 默认串行执行；只有无依赖、只读或明确幂等的节点允许最多 2 路并行。

---

## 四、术语与职责边界

### 4.1 Planner

Planner 是当前模型在结构化契约约束下生成或修改 `DeepNotePlan` 的角色。它负责目标拆解、章节语义、证据要求和建议依赖，但不能直接改变运行状态或宣布节点完成。

### 4.2 Executor

Executor 是 Rust 编排器中的节点执行器。它只执行编译后的封闭节点类型，接受真实的解析结果、模型输出和运行层结果，不把模型声称完成的内容当作事实。

### 4.3 Observation

Observation 是真实执行结果，例如解析出的 Source Chunk、模型响应、Schema 错误、证据匹配结果、验证报告、用户确认或 Tool Result。模型正文中的“我已经读过”不是 Observation。

### 4.4 Replanner

Replanner 根据失败、矛盾、新证据、预算和剩余步骤生成局部 Plan Patch。它不每轮重写整份计划，不修改已经有证据支持的历史步骤。

### 4.5 Evaluator / Reviewer

Evaluator 优先由 Schema、规则、来源 Hash、结构检查和测试程序完成；LLM Reviewer 只在规则无法判断时进行语义审查。Reviewer 不能单独把章节标记为完成。

### 4.6 Skill

Skill 是可渐进加载的方法论和领域规则，不是运行状态。Skill 可以指导 Planner 如何提问、组织章节和评价证据，但不能改变 Rust 的预算、权限、节点状态和持久化规则。

### 4.7 Evidence Artifact

Evidence Artifact 是由稳定 Source Chunk、来源位置、内容 Hash、模型归纳和论断映射组成的轻量中间产物。它不是整份 PDF 文本的复制品，也不是模型自由编写的引用。

### 4.8 Note Ledger

Note Ledger 是 Run 级共享知识账本，保存目标、读者层级、术语、已确认事实、证据映射、章节覆盖、冲突和缺口。模型只能提交经过验证的 Ledger Patch。

---

## 五、总体架构

```text
Chat / 文献阅读页 / 笔记页
            │
            ▼
Deep Note Workspace（独立工作区）
            │  命令与事件订阅
            ▼
DeepNoteOrchestrator（Rust，运行真相）
   ├─ Capability Preflight
   ├─ Input Snapshot
   ├─ Read-only Reconnaissance
   ├─ Planner / Plan Version
   ├─ Plan Compiler
   ├─ DAG Scheduler
   ├─ Evidence Artifact Store
   ├─ Note Ledger
   ├─ Validator / Conditional Reviewer
   ├─ Budget / Permission / Cancellation
   ├─ Checkpoint / Recovery / Idempotency
   └─ Markdown + Sidecar Persist
            │
            ▼
Frontend Workflow Projection + 正文/目录/证据按需展示
```

### 5.1 前端职责

- 发起深度笔记、切换工作区和提交用户确认。
- 展示能力预检、语义计划、章节状态、正文、证据、冲突和验证结果。
- 订阅 Rust 事件并生成投影，不自行推进 DAG 或猜测节点状态。
- 运行中允许只读预览；编辑操作必须发出暂停和版本化修订命令。
- 完成后默认折叠过程，优先显示正文和回答目录；用户可按需打开完整过程。
- Evidence、Ledger、DAG 和长结果采用按需加载，避免一次性常驻 WebView 状态。

### 5.2 Rust 编排器职责

- 建立和恢复 Run，持有取消令牌、预算和当前状态。
- 固定输入快照，验证模型能力和附件边界。
- 解析并校验模型结构化计划，编译受约束 DAG。
- 调度节点、保存事件、检查点和副作用账本。
- 执行确定性验证，决定 Advance、Retry、Revise、Replan、AskUser、Finalize 或 Fail。
- 以幂等事务创建最终笔记和 Sidecar。

### 5.3 Provider 与当前模型

- Planner、Writer、Reviewer、Replanner 复用当前模型和模型设置。
- Provider Adapter 只负责协议适配、结构化输出、usage、reasoning、错误和流式结果规范化。
- 不支持 Tool 的模型不注入不可执行 Tool Schema；纯文本深度笔记仍可以继续。
- 不支持相关附件能力时，在输入预检阶段阻止开始，不让模型猜测附件内容。

---

## 六、Plan Mode 与规划访谈

### 6.1 借鉴边界

Claude Code/Codex 的 Plan Mode 的核心价值是：先调查、先形成方案、用户审核后再执行具有副作用的动作。Mnemora 借鉴其交互和确认边界，不把代码工具的只读权限模型原样套在笔记上。

规划阶段允许读取对话和附件，建立临时证据索引；禁止创建最终笔记、覆盖已有笔记、导出文件和修改原始资料。

### 6.2 快速规划

默认流程：

```text
输入预检
→ 有界侦察
→ 当前模型输出 DeepNotePlan
→ Schema 与来源 ID 校验
→ 用户审核计划
```

用户可执行：

- 开始生成；
- 继续调整要求；
- 直接编辑语义计划；
- 切换模型后重新预检和验证；
- 取消并保留规划记录。

### 6.3 规划访谈

只有用户主动选择，或存在无法可靠推断且会改变结果的关键冲突时，才加载 `grill-me` 类 Skill。一次只问一个问题，例如：

```text
这份笔记主要用于系统学习、快速复习还是发表前整理？
推荐：系统学习与复习，因为它会影响章节深度和自检设计。
```

不允许把规划访谈变成每次深度笔记都必须完成的冗长问答。

### 6.4 计划确认页必须展示

- 笔记目标、用途和读者层次；
- 输入来源、未使用来源和附件能力状态；
- 章节结构、每章目的和建议依赖；
- 证据要求、AI 补充边界和冲突提示；
- 预计模型调用、Token、时间和并发；
- 当前模型、Provider、是否支持 Structured Outputs；
- “开始生成”“继续调整”“切换模型”“取消”操作。

---

## 七、DeepNotePlan v1 与 DAG 编译

当前深度笔记作为第一版正式架构，不设计旧笔记或旧 Run 迁移分支。数据库和接口直接采用下面的最终契约方向。

### 7.1 语义计划字段

建议计划至少包含：

```text
planId
runId
version
goal
audience
scope
title
summary
weakPoints
allowAiSupplement
evidencePolicy
sections[]
budget
sourceIds[]
```

每个章节至少包含：

```text
sectionId
heading
kind
purpose
brief
dependsOn[]
evidenceRequirements[]
successCriteria[]
sourceScope[]
targetDepth
allowAiSupplement
```

### 7.2 Plan 状态

```text
draft
awaitingUserConfirmation
active
completed
blocked
cancelled
superseded
```

### 7.3 章节状态

```text
pending
ready
inProgress
completed
needsReview
needsRevision
failed
blocked
skipped
```

### 7.4 Rust Plan Compiler

模型只提出语义计划和建议依赖，Rust 编译器负责：

1. 分配稳定 Node ID 和 Section ID。
2. 验证章节 ID 唯一、依赖存在且没有循环。
3. 将计划补充为封闭的系统节点。
4. 检查节点所需能力、权限和来源是否存在。
5. 判断节点是否可以并行。
6. 绑定预算、重试、审批、超时和输出上限。
7. 生成可恢复的最终执行图。

计划编译失败时返回结构化错误，由当前模型生成局部修复；不得从自然语言猜测一个“差不多能执行”的图。

### 7.5 封闭节点类型

首版节点类型固定为：

```text
AnalyzeInput       分析输入快照
ReconSource        只读材料侦察
ExtractEvidence    提取并验证证据
BuildLedger        建立或更新知识账本
DraftSection       生成章节初稿
ValidateSection    章节规则验证
ReviewSection      条件式语义审查
ReviseSection      局部修订
ValidateGlobal     全局确定性验证
ApplyPatch         应用受验证的局部补丁
AssembleNote       组装 Markdown 正文
PersistNote        事务写入正文与 Sidecar
```

Skill 可以改变节点参数和方法，但不能动态注册未经过版本化的底层节点类型。

### 7.6 DAG 并发规则

- 执行器默认串行，优先保证术语、Ledger 和上下文一致性。
- 只有无依赖、只读、无共享写入或明确幂等的节点才允许并行。
- 单 Run 最大并发数为 `2`。
- 章节撰写默认不并行；后续只有 Release 实测证明内存和流畅性安全，才扩大范围。
- 当前全局只允许一个 Run 进入正式执行阶段。

---

## 八、输入快照与两阶段取证

### 8.1 Input Snapshot

开始规划时记录：

```text
conversationRevision
messageIds
attachmentIds
attachmentContentHashes
selectedLiteratureIds
selectedNoteIds
modelProviderId
modelId
effectiveCapabilities
permissionMode
createdAt
```

执行始终基于同一快照。执行期间出现新消息、附件替换或原笔记变化时，只提示用户，不自动纳入。

用户选择纳入新内容后：

1. 计算新的快照差异。
2. 创建新 Plan Version。
3. 标记受影响 Evidence、Ledger 和章节节点。
4. 复用仍然有效的节点。
5. 对失效节点重新取证或生成。

### 8.2 规划阶段有界侦察

允许读取：

- 对话结构和关键消息；
- 附件元数据、目录、摘要和关键片段；
- PDF 页码、DOCX/XLSX 的结构信息；
- 明显的证据缺口、冲突和 OCR 风险。

不要求规划阶段扫描所有材料。侦察预算满足以下任一条件即可停止：

- 每个候选章节已有来源线索；
- 可以判断章节是否可执行；
- 连续读取没有发现新主题；
- 达到规划阶段 Token、时间或 Artifact 预算。

预算耗尽但无法形成可靠计划时，显示“材料侦察不足”，由用户选择增加预算、缩小范围或切换模型。

### 8.3 执行阶段完整取证

用户确认后按章节需要提取完整 Evidence。只有用户显式加入的新资料才可以扩大来源范围；不能通过 Planner 伪造“已读取”状态。

---

## 九、Evidence Artifact 与来源验证

### 9.1 生成流程

```text
原始材料
→ 运行层确定性解析
→ Source Chunk（稳定 ID、位置、Hash）
→ 模型筛选相关片段和论断
→ Rust 校验引用确实存在
→ 保存 Evidence Artifact
```

### 9.2 Source Chunk

Source Chunk 应保留：

- `sourceId`、`chunkId`；
- 来源类型：conversation、pdf、docx、xlsx、note；
- 消息 ID、文档 ID、页码、段落或表格位置；
- 原文片段；
- 内容 Hash；
- OCR 置信度（适用时）。

原始片段不可由模型修改。模型产生的归纳和原文必须分字段保存。

### 9.3 Evidence 字段

```text
evidenceId
sourceChunkIds[]
claim
modelSynthesis
sourceExcerpt
supportLevel
status: verified | conflicting | insufficient | invalidated
createdAt
```

Rust 必须验证：

- 引用的 Chunk 存在；
- 摘录确实属于对应 Chunk；
- 来源 Hash 与当前快照一致；
- 页码、消息 ID 或位置真实有效；
- OCR 低置信度不能自动支撑严格事实论断。

### 9.4 冲突与缺口

- 冲突 Evidence 由多个来源分别保存，并标记适用条件。
- 能解释时生成对比或边界内容；不能解释时明确写出材料冲突。
- 核心证据不足时，章节进入 `needsEvidence`，触发局部取证或重规划。
- 扩大到未确认来源、联网搜索或新增附件时必须再次请求用户确认。
- Reviewer 可以识别冲突，但不能凭常识裁决来源真伪。

---

## 十、Note Ledger 与章节一致性

### 10.1 Ledger 内容

```text
noteGoal
audience
canonicalTerms[]
verifiedFacts[]
evidenceClaimLinks[]
coveredTopics[]
openQuestions[]
conflicts[]
aiSupplements[]
sectionSummaries[]
globalConstraints[]
```

### 10.2 更新规则

1. Evidence 节点先提交事实和术语候选。
2. Rust 校验后更新 Ledger。
3. 章节生成只加载相关账本切片，不把整个笔记全文注入每次请求。
4. 模型只能提交 `Ledger Patch`，不能静默覆盖已验证事实。
5. 章节验证后提交摘要、覆盖范围和新增术语。
6. 术语冲突、事实矛盾或重复覆盖达到阈值时触发局部修订或 Replan。

---

## 十一、执行循环、验证与重规划

### 11.1 标准循环

```text
读取 Ready 节点
→ 执行
→ 记录 Observation / Evidence
→ 确定性 Progress Gate
→ Advance / Retry / Revise / Replan / AskUser / Finalize / Fail
```

### 11.2 章节验证顺序

1. Markdown 结构是否合法。
2. 是否为空、明显过短或超出边界。
3. 必需主题是否覆盖。
4. Evidence ID、来源位置和 Hash 是否有效。
5. 核心论断证据覆盖率是否达到要求。
6. AI 补充是否正确标记。
7. 与已完成章节是否重复或冲突。
8. 是否满足章节 `successCriteria`。

只有满足门禁的章节才能进入 `completed`。模型不能仅凭返回“完成”改变状态。

### 11.3 条件式语义审查

仅在以下情形调用当前模型 Reviewer：

- 规则检查无法判断语义覆盖；
- 可能与 Evidence 矛盾；
- 重复、遗漏或术语冲突达到阈值；
- 章节连续失败；
- 全文组装后存在全局一致性问题。

Reviewer 输出固定结果：

```text
accept
revise
replan
blocked
```

Reviewer 不能直接写入正文或宣布最终完成。

### 11.4 局部重规划

触发条件：

- 工具或解析明确失败，原步骤无法继续；
- 结果为空或不满足成功标准；
- 新证据否定计划前提；
- 依赖节点阻塞；
- 连续两轮没有新增 Evidence；
- 同一等价动作连续失败；
- 用户改变目标或来源范围；
- 剩余预算不足以完成原计划。

局部 Replan 只修改未完成部分，保留已验证节点和用户手写内容。每次修改写入新的 Plan Version，并记录 `revisionReason` 和差异。

---

## 十二、预算、重试与停止条件

各类预算分开计数，不能将所有字段都命名为 `retryAttempts`。

### 12.1 已确认上限

| 预算 | 数值 | 计数语义 |
| --- | ---: | --- |
| Provider 网络自动重试 | 5 | 不含首次请求，最多 6 次网络请求 |
| 节点执行尝试 | 5 | 含首次执行，首次失败后最多再执行 4 次 |
| 章节语义修订 | 5 | 不含初稿，初稿失败后最多修订 5 次 |
| Plan 局部修订 | 4 | 同一 Run 自动局部 Replan 最多 4 次 |
| 单 Run DAG 并发 | 2 | 仅安全节点 |
| 全局正式执行 Run | 1 | 其他 Run 排队或暂停 |
| 语义模型调用预算 | `min(2 + sectionCount × 3, 80)` | Planner、Writer、Reviewer、Replanner 等计入 |

### 12.2 预算行为

- 网络重试不消耗语义调用预算。
- 达到局部上限时停止对应循环，保留错误和检查点。
- 达到全局预算时停止非必要 Reviewer 和新规划，优先保存已有结果。
- 达到预算不允许静默降低证据标准或生成简版笔记。
- 用户明确增加预算后创建新的预算版本，旧统计不清零。
- 取消、暂停、错误、预算耗尽和等待用户确认都必须持久化。

### 12.3 防止重规划抖动

- 规范化计划后计算 Hash；无实际变化不增加版本。
- 每个 Tool/节点轮次最多一次 Plan Mutation。
- 同一等价动作连续两次失败必须换策略或阻塞。
- Replan 后连续两轮无新增 Evidence，停止自动扩展，转为 Finalize、Blocked 或 AskUser。
- 已拒绝的审批不得原样反复请求。

---

## 十三、模型切换与能力门禁

### 13.1 启动前预检

根据输入快照计算需求：

- 纯文本：可使用普通文本模型；提示当前模型不支持 Tool 时只能基于现有文本。
- PDF、DOCX、XLSX：必须具备对应运行层文档解析和模型消费能力。
- 图片：必须具备明确的视觉能力。
- 动态搜索或其他工具节点：必须具备 Tool 能力。
- Structured Outputs：支持则优先，不支持则走严格 JSON 兼容路径。

预检失败时禁止开始，并提供：

```text
切换模型 / 移除不支持附件 / 返回设置 / 取消
```

未知能力按不支持处理，不伪造支持。

### 13.2 执行中切换模型

用户主动切换模型后：

1. 暂停当前 Run。
2. 创建新的 `Execution Version`。
3. 记录旧模型、新模型、Provider 和切换时间。
4. 保留 Input Snapshot、真实 Evidence 和用户手写内容。
5. 对未验证模型章节重新审查；已通过确定性证据验证的章节可复用。
6. 旧请求迟到时不得覆盖新版本结果。

---

## 十四、Skill 体系与渐进式披露

### 14.1 默认内置 Skill

项目默认安装一组有明确来源的 Skill，例如：

- `grill-me`：逐项澄清影响结果的关键决策；
- `question-framing`：发现用户未直接提出但更关键的问题；
- `plan-review`：检查计划依赖、风险和验证标准；
- `deep-note-planner`：生成学习型深度笔记计划；
- `evidence-review`：检查证据覆盖、来源冲突和缺口；
- `technical-explainer`：面向初学者分层解释技术内容；
- `compare-and-contrast`：建立多文献或多概念对比矩阵。

采用 GitHub 上成熟项目时，必须：

- 记录原作者、仓库、路径、版本或 Commit Hash；
- 保留许可证和版权声明；
- 在应用中展示来源和版本；
- 只有许可证允许再分发的 Skill 才能直接捆绑进安装包；
- 无明确再分发许可的项目可支持用户自行安装，但不能擅自打包复制。

### 14.2 加载策略

- 默认安装不等于默认加载。
- 目录只保存名称、简介、来源、能力依赖和适用条件。
- Skill 正文仅在用户选择或模型真实激活时加载。
- Skill 不可修改预算、权限、状态机和数据库。
- 前端只有收到真实 `skillActivated` 事件才显示技能活动。
- `grill-me` 不在每次深度笔记中自动触发，只在规划访谈或关键冲突时使用。

---

## 十五、持久化、检查点与恢复

### 15.1 建议的状态对象

```text
DeepNoteRun
  runId
  conversationId
  inputSnapshotHash
  currentPlanVersion
  executionVersion
  phase
  budgetSnapshot
  modelSnapshot
  status
  createdAt / updatedAt

PlanVersion
  planId / runId / version
  planJson
  compiledDagJson
  revisionReason
  createdAt

DagNode
  nodeId / planVersion
  nodeType
  sectionId
  dependsOn
  status
  attemptCount
  evidenceIds
  validationJson

EvidenceArtifact
  evidenceId
  sourceChunkIds
  contentHash
  status
  payloadRef

NoteLedger
  runId / version
  ledgerJson
  patchHistory

DeepNoteEvent
  runId / sequence
  eventType
  payloadRef
  createdAt

DeepNoteOutput
  noteId
  markdown
  sidecarJson
  idempotencyKey
```

### 15.2 恢复规则

- 每个节点开始、结果、验证、失败和完成都写入事件。
- `running` 节点在重启后标记为 `interrupted`，不能假定完成。
- 已完成且输入 Hash、计划版本和模型执行版本仍有效的节点直接复用。
- 中断节点使用相同输入快照重新执行。
- 最终笔记写入、来源绑定和 Run 完成使用事务。
- 所有写入带 `idempotencyKey`，恢复不会重复创建笔记。
- 取消只停止新节点，不删除已完成 Evidence、章节和检查点。
- 用户可以继续、重试失败节点、保存不完整草稿或取消 Run。

### 15.3 存储回收

- SQLite 主要保存计划、状态、索引、Hash、引用和事件。
- 大型原文片段、模型原始输出和临时解析结果放在可按需加载的 Artifact 存储。
- Run 进行中、暂停或失败时保留恢复所需内容。
- 完成后只清理无引用或可由原始附件重建的重型内容。
- 删除原始附件前提示哪些来源将失去复查能力。

---

## 十六、Markdown 正文与证据 Sidecar

### 16.1 双层存储

```text
Note
├─ Markdown 正文：用户阅读、编辑、渲染、导出
└─ Structured Sidecar：章节 ID、Claim、Evidence、来源、验证和版本
```

Markdown 是用户可移植的权威正文，不把 DAG、Ledger 和内部 ID 塞进正文格式。Sidecar 是机器验证和来源追踪的权威元数据。

### 16.2 用户编辑

- 运行中只读预览。
- 用户点击编辑后暂停 Run。
- 记录编辑版本和内容来源：`modelGenerated`、`userEdited` 或 `mixed`。
- 只改措辞时可保留 Evidence，但必须重新验证。
- 改变事实论断、章节目标或结构时必须重新检查 Evidence 或创建 Plan Version。
- 全局补丁触及用户手写区域时必须再次确认。

### 16.3 导出

普通 Markdown、JSON、PDF 导出是确定性格式转换，不再次调用模型：

- Markdown 生成脚注或文末来源列表；
- PDF 生成可读的页下注释或尾注；
- 对话来源显示“来源：会话”，不暴露内部消息 ID；
- AI 补充与文献事实分开标记；
- 冲突证据同时导出支持方、反对方和来源；
- 无法公开定位的内容明确标为“来源不可复查”；
- DAG、Ledger、Plan Version 和验证事件不进入普通导出。

另提供用户主动选择的“研究归档包”，其中可包含计划、Evidence 清单、Sidecar、验证报告和运行事件。

---

## 十七、独立深度笔记工作区

深度笔记的规划、证据、章节预览、暂停、恢复和重规划超过普通弹窗承载能力，采用独立工作区。

建议布局：

```text
顶部：Run 状态、当前模型、预算、暂停/继续
左侧：来源、Source Chunk、Evidence 和冲突
中间：语义计划、章节正文、用户编辑
右侧：章节状态、验证、Ledger 和待处理问题
底部/折叠区：真实工作流、DAG、Skill 和模型活动
```

原则：

- Chat、文献阅读页、笔记页都可以进入同一个 Run。
- Run 拥有独立路由，离开页面后仍可继续或稍后恢复。
- 规划阶段默认展示语义计划，不暴露底层 DAG 细节。
- 完成后默认折叠工作流，突出正文和目录。
- 失败、取消、阻塞、预算耗尽和等待用户确认时保持过程入口可见。
- 小窗口使用标签页或抽屉，不强行保留三栏。

---

## 十八、开发里程碑

### 阶段 A：运行契约和第一版数据结构

目标：建立第一版正式数据模型，不考虑旧 Run 迁移。

- 定义 Plan、Plan Version、DagNode、Evidence、Ledger、Event、Checkpoint Schema。
- 增加状态转换、版本、Hash、幂等键和数据库索引。
- 实现输入快照和模型能力预检。
- 验收：非法依赖、循环、悬空来源、版本冲突和不支持附件都能被拒绝。

### 阶段 B：Plan Mode 与规划体验

- 独立工作区骨架。
- 快速规划、计划编辑、计划确认和取消。
- 规划访谈 Skill 的渐进加载。
- Structured Outputs 优先、严格 JSON 兼容和格式错误反馈。
- 验收：未确认计划不能创建最终笔记；规划阶段不能执行写入。

### 阶段 C：两阶段取证与 Evidence

- Source Chunk 确定性解析。
- PDF、DOCX、XLSX、对话和图片能力门禁。
- Evidence Artifact、Hash、页码/消息定位和冲突状态。
- 规划侦察产物可在执行阶段复用。
- 验收：伪造引用为 0；无法定位的证据不能进入最终 Sidecar。

### 阶段 D：Ledger 与 DAG 编译器

- 术语、事实、证据映射和章节覆盖账本。
- 模型语义计划编译为封闭节点 DAG。
- 默认串行，安全节点最多 2 路并行。
- 验收：循环、越权节点、未披露来源和绕过验证路径全部拒绝。

### 阶段 E：执行、验证和局部修订

- Draft、Validate、Review、Revise、Replan 节点。
- 规则检查优先，条件式 Reviewer。
- 章节最多 5 次语义修订，Plan 局部修订最多 4 次。
- 全局验证只产生局部 Patch。
- 验收：章节不能仅凭非空文本完成；核心证据缺口会阻塞或重规划。

### 阶段 F：检查点、恢复和资源治理

- 节点级事件、断点恢复、幂等写入。
- 输入变化、模型切换和用户编辑的版本流程。
- 重型 Artifact 按需加载和安全清理。
- 全局单执行 Run 和队列。
- 验收：崩溃、网络中断、迟到响应和重复恢复均不重复创建笔记。

### 阶段 G：输出、调试和评测

- Markdown + Sidecar。
- 普通导出和研究归档包。
- 开发者调试页：计划、DAG、Evidence、Ledger、预算和事件。
- 合成数据、真实 PDF 和故障注入评测。
- 验收：满足全部发布硬门禁后，才将深度笔记作为正式功能开放。

---

## 十九、测试与正式发布门禁

### 19.1 必测场景

- 纯文本短对话和长对话；
- 单 PDF、多 PDF 和扫描 PDF；
- DOCX、XLSX 和图片附件；
- 模型不支持 Tool、视觉或 Structured Outputs；
- 结构化计划格式连续失败；
- 多来源冲突；
- 核心证据缺失；
- 规划访谈、计划调整和实质性重规划；
- 章节失败、网络中断、应用重启；
- 用户编辑章节后继续生成；
- 执行过程中切换模型；
- 达到节点、修订、Plan 和全局预算；
- 导出 Markdown、PDF、JSON 和研究归档包。

### 19.2 不可绕过的硬门禁

1. 所有展示的引用都能定位到真实 Source Chunk，伪造引用必须为 `0`。
2. 核心成功标准未满足的章节不能标记 `completed`。
3. 没有用户计划确认不能写入最终笔记。
4. 失败不能静默生成简版笔记。
5. 崩溃恢复不能重复创建笔记、来源或副作用。
6. 用户手写内容不能被模型后台结果覆盖。
7. DAG 不得存在循环、悬空依赖或绕过验证的路径。
8. 任何预算耗尽都必须停止并保留检查点。
9. 普通导出不能调用模型。
10. 当前模型不支持附件时，上传/启动入口必须明确阻止或提示切换。
11. Workflow UI 只能展示真实 Plan、Evidence、Skill、Reasoning 和 Tool 事件。
12. 内存、延迟和流畅性不得因深度笔记引入不可接受回归，继续服从 Plan07 的 Release 门禁。

### 19.3 语义质量指标

自动检查与人工抽样共同评估：

- 结构完整性；
- 核心论断证据覆盖；
- 事实一致性；
- 术语统一；
- 章节重复率；
- 冲突是否诚实保留；
- AI 补充标记正确性；
- 学习和复习价值；
- 计划与最终正文的一致性。

LLM 评分只能作为辅助，不能替代来源验证、状态机测试、故障注入和幂等性测试。

---

## 二十、明确不采用的方案

以下方案不进入第一版：

1. 每次深度笔记都强制调用独立 Planner Model。
2. 在普通 Chat 中默认显示“正在规划”。
3. 从正文 Markdown 列表猜测结构化计划。
4. 每个章节、每一轮都强制调用 Critic。
5. 允许模型自行标记 Step Completed。
6. 将完整 PDF 或完整 Tool Output 复制进每个章节 Prompt。
7. 默认启用 LATS（Language Agent Tree Search）或 Tree of Thoughts 多分支搜索。
8. 第一版直接开放任意 DAG 并行和多 Run 并发。
9. 规划失败后静默降级为简版笔记。
10. 通过切换模型、降低证据标准或猜测附件内容来掩盖能力不足。
11. 让 Skill 修改运行层预算、权限、状态机或数据库。
12. 让前端状态替代 Rust 和持久化事件作为运行真相。

---

## 二十一、实施时的最终检查表

### 规划前

- [ ] 当前模型、Provider 和能力已解析。
- [ ] 附件能力门禁已通过。
- [ ] 来源范围已由用户确认。
- [ ] Input Snapshot 已创建。
- [ ] 规划阶段只读工具已启用，写入工具被阻止。

### 计划确认前

- [ ] Plan Schema 合法。
- [ ] 章节目标、依赖、Evidence Requirements 和 Success Criteria 完整。
- [ ] 计划没有引用不存在的来源或 Tool。
- [ ] 预计预算和模型信息已展示。
- [ ] 用户可编辑、调整、切换模型或取消。

### 执行中

- [ ] DAG 已由 Rust 编译并通过无环检查。
- [ ] 节点状态只由运行层推进。
- [ ] Evidence、Ledger 和事件已持久化。
- [ ] 规则验证先于语义审查。
- [ ] 章节修订和全局预算独立计数。
- [ ] 页面切换不会终止 Run。

### 完成前

- [ ] 所有核心论断 Evidence 已验证。
- [ ] 冲突、缺口和 AI 补充已正确标记。
- [ ] 全局验证已完成。
- [ ] 所有局部补丁已通过影响范围和来源校验。
- [ ] Markdown 与 Sidecar 使用同一最终版本。
- [ ] 持久化写入带幂等键且未重复。

### 导出后

- [ ] 普通导出未调用模型。
- [ ] 引用转换为可移植格式。
- [ ] 内部 ID 未泄漏到普通导出。
- [ ] 研究归档包仅在用户主动选择时生成。
- [ ] 无引用重型 Artifact 可安全清理。

---

## 二十二、最终共识

Mnemora 的深度笔记不是把通用 Chat Agent 再套一层“看起来像计划”的 Markdown，而是一个有明确边界的专用编排器：

```text
模型负责：理解材料、提出语义计划、筛选证据、撰写章节、提出修订

Skill 负责：规划方法、领域知识、澄清问题和评价策略

Rust 负责：能力门禁、Plan Schema、DAG 编译、状态、权限、预算、证据验证、恢复和写入

用户负责：确认目标、来源、计划、实质性变更和最终取舍
```

因此第一版的正确路线不是“增加更多模型调用”，而是让每一次模型调用都处在可解释、可验证、可恢复的合同之内。Plan-and-Execute 保证整体过程可控，DAG 为将来的安全并行保留结构，Plan Mode 保证用户在正式生成前拥有知情和确认权，Evidence 与 Ledger 则保证最终笔记不会把模型猜测伪装成材料事实。

