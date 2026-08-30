# 19-DeepNote P1：动态路由与在线 AIMD

本轮完成 P1-2 至 P1-9，并把 P1-1 从“一次性离线拟合门槛”修正为“历史先验 + 持续在线校准”。中转站、账户和模型目录都可能变化，因此容量状态不再只按 `providerId + modelId` 永久复用。

## 1. 路由身份与生命周期

容量状态的 `routeKey` 由以下非敏感信息派生：

- provider ID、规范化 Base URL、协议和认证方式；
- `credentialRevision`：API Key 每次显式写入或删除时递增，不保存 Key 本身；
- model ID 与实际 `apiModel`；
- DeepNote 使用流式保活还是非流式。

SQLite v16 新增 `deep_note_route_profiles`。endpoint、协议、凭据代际、模型映射或传输策略变化都会建立新路由；旧代际被标记为 tombstoned，保留 30 天后回收。模型或 provider 禁用时状态为 disabled，再启用时回到 unknown。

运行态区分 unknown、available、degraded、circuitOpen、unsupported、disabled、tombstoned。`model_not_found` 进入有期限的 unsupported；限流、账户并发和显式无可用渠道进入短期熔断；网络与 5xx 需要连续失败才熔断。到期后允许半开探测，不会把模型永久判死。

## 2. 在线 AIMD 与冷启动

- 完全无历史的新路由从 8k Token 启动；
- 同模型另一传输路由、同 provider 其他模型、同协议同 API Model 依次作为分层先验；
- 先验使用低四分位，不使用平均值；
- 只有达到当前包线 75% 的成功 Chunk 才计入增长，连续 3 次后增加 1024 Token；
- 上下文超限立即折半；首次超时先临时折半 10 分钟，连续超时才固化到长期包线；
- 429、鉴权、模型下线、5xx 等可用性问题不修改长期载荷包线；
- 路由 24 小时没有样本时，高容量状态衰减回不高于 8k 的保守档。

历史样本因此仍有价值，但只用于更好的初值、步长和窗口复核。没有 100 条样本的新中转站或新模型也可以安全启动。

## 3. 接入点

`context_budget` 现在对模型上下文可用量、静态安全档和动态路由包线取最小值；旧 run 恢复时允许收缩，但不会因其他 run 的成功突然放大并令已有检查点失效。

附件 Reader 产物先形成有界预分块，再只合并同一附件、同一消息、同一来源类型的连续窗口。合并结果不得超过动态包线，最终仍受 96 个来源 Chunk 硬上限约束。批量本地文件入口也从 100 对齐为 96，超过时明确拒绝，不静默丢弃。

Token 估算统一为一套底层标度：ASCII 字符 1 unit、非 ASCII 字符 4 units、4 units 为一个向上取整 Token。字符级切分保留 unit 精度，对外预算只使用 Token。

## 4. 全局并发与节点租约清理

所有模型物理请求在 chat 派发层按 provider 共享许可池。默认总并发为 4，后台请求最多占 3 个，因此 DeepNote、Agent 等后台工作满载时仍给交互式 chat 留 1 个席位。等待许可支持取消，而且许可取得后才扣物理请求预算，排队不会制造虚假的请求消耗。不同 provider 使用独立池，单个中转站拥堵不会冻结其他中转站。

节点级 `Leased` 状态在实际调度器中不可达，现已从节点状态机删除；节点续租、过期回收和 lease CAS 一并移除。run 级 `runtime_instance_id + heartbeat` 仍负责排除迟到 Worker。SQLite v16 会把遗留 `leased` 节点归一为 `ready` 并清空租约值；租约列暂留，等 P3 重建表时再 DROP。

## 5. 遥测与诊断

物理 attempt 和终态事件增加 `routeKey`、`providerConfigEpoch`、路由状态、动态上限和 profile 样本数。新增：

- `routeCallSuppressed`：熔断或模型暂不可用时，请求在本地被阻止；
- `routeProfileUpdateFailed`：状态写入失败，当前调用继续使用旧安全值；
- `attachmentChunksPacked`：记录打包前后 Chunk 数和目标包线。

DeepNote 诊断面板显示动态分块上限、路由状态与容量样本数。

## 6. 验证

- Rust 全量测试：418 passed；
- TypeScript：`npx tsc --noEmit` 通过；
- 前端测试：223 passed。

P1 已全部完成，下一阶段进入 P2 文件系统化。
