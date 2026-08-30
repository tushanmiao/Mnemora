# 18-DeepNote P1：物理请求预算与遥测基础

承接 `16`、`17`，本轮按 P1 顺序先处理 P1-1 的数据前提与可独立落地的 P1-7，同时补齐 P0-10 在 Analyzing 阶段的状态机接线。所有改动停在工作区，未提交。

## 1. 当时为什么没有直接实现 AIMD

现有生产记录不足以拟合 provider + model 包线：旧事件没有请求体字节数、物理 attempt 数和传输形态；目前可见样本又集中在单个 run 和较窄体积范围。直接从这些数据给 AIMD 设初值与步长，会把“保守猜测”包装成“离线拟合”。

这是当轮实现时的保守判断。后续确认中转站和模型目录持续变化后，P1-2 已改为“保守分层先验 + 在线学习”，不再等待每个 provider + model 达到固定样本门槛。当前实现见 `19-DeepNote-P1动态路由与AIMD.md`。

当时顺序拆成：

1. 先让每个真实 HTTP 请求产生可拟合、可恢复的遥测；
2. 收集覆盖成功、超时、限流、流式回落的真实样本；
3. 再执行 P1-1 离线拟合；
4. 拟合结果确认后进入 P1-2 控制器。

## 2. P1-7 已完成：上游请求数 + 累计墙钟

`semanticCallsUsed` 现在只保留为逻辑调用诊断值，不再充当 provider 配额。新增：

- `upstreamRequestLimit`：run 级物理 HTTP 请求硬上限，存量运行时默认 640；
- `upstreamRequestsUsed`：实际放行的物理请求数；
- `modelAttemptStarted`：每次普通重试、流式请求、流式回落后的非流式请求各写一条；
- `try_append_note_pipeline_upstream_attempt`：在 `BEGIN IMMEDIATE` 事务内完成计数、判限、写事件与更新 runtime JSON。

扣减点位于 dispatcher 真正发请求之前。并行 section 同时竞争最后一个名额时只有一个能成功；失败的请求既不写 attempt 事件，也不会到达 provider。崩溃恢复以事件表重建用量，不会重新获得预算。升级前没有 attempt 事件的调用，按终态事件至少折算一次；mock 事件不占物理预算。

旧的 `reserve_semantic_calls` 已删除。增量附件、chunk digest、提纲调整和人工恢复不再通过“需要多少就抬高多少”修改上限。

## 3. P1-1 遥测 v2

每条 `modelAttemptStarted` 现在包含：

- `providerId`、`modelId`、`operation`、`phase`；
- `requestIndex`、`retryIndex`、`maxRetries`；
- `transport`（streaming / nonStreaming）；
- `requestBytes`（按实际传输 JSON 序列化后计量）；
- `estimatedInputTokens`、`maxOutputTokens`。

终态 `modelCallCompleted` / `modelCallFailed` 增加：

- `actualAttemptCount`；
- `streamingAttemptCount` / `nonStreamingAttemptCount`；
- 本次调用最大 `requestBytes`；
- `estimatedInputTokens`；
- 成功时 provider 输入/输出 token 与 `timeToFirstTokenMs`。

前端诊断页改为展示“上游请求”预算，逻辑调用保留为辅助指标；运行记录能直接看到物理请求序号、传输方式、请求体积、TTFT 和预算耗尽原因。

## 4. P0.5：非 Drafting 阶段预算收敛

`DeepNoteRunMachine::timeout()` 已有生产调用点：

- Analyzing 在每个逻辑调用前及每个物理请求前检查双维度预算；耗尽后执行 `PersistTimeout`，落到 `Blocked`；
- Drafting 批次派发前检查同一预算；已有章节时执行 `SkipUnfinishedSections` 并部分交付；没有章节时执行 `PersistTimeout` 并落到 `Blocked`。

并行 chunk 流遇到预算错误会立即 drop 剩余 futures，避免把错误包装成普通“分块失败”后继续投放后续 chunk。

## 5. 验证

本轮验证结果：

| 检查 | 结果 |
| --- | --- |
| `cargo check --all-targets` | 通过，0 warning |
| `cargo test` | 408 passed / 0 failed |
| `npx tsc --noEmit` | 通过 |
| `npx vitest run` | 222 passed / 0 failed |

新增测试锁定：流式 body 按真实 JSON 称重；旧事件与新 attempt 不重复计数；请求上限不被抬高；两个并行 worker 竞争最后一个名额只能成功一个；前端可解释 attempt 与预算耗尽事件。

## 6. 下一步观测门槛

P1-2 开始前，至少按每个活跃 provider + model 收集：

- 不少于 100 个 `deepNoteChunk` 终态样本；
- 至少 3 个明显不同的请求体积区间；
- 成功样本与 timeout / rate-limit / stream-fallback 样本均非零；
- 能按 `callId` 将 attempt 与终态事件关联，确认没有漏记或重复计数。

这些数量现在只表示“成熟路由的参数复核置信度”，不是启动门槛。新路由以 8k 保守先验上线，随后在线学习；长期未使用或配置代际变化时自动回到保守档。
