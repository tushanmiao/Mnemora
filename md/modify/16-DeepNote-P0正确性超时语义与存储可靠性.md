# 16-DeepNote P0 优化：正确性、超时语义与存储可靠性

对应分析文档 `md/plan/14-DeepNote/`。本轮实施该计划的 **P0 全部十四项**，严格按决策六规定的顺序（正确性 → 超时语义 → 存储可靠性），未动 P1/P2/P3。

所有改动停在工作区，未提交。

---

## 1. 本轮解决了什么

DeepNote 此前存在四类问题，彼此纠缠：

一是**正确性**。Persisting 阶段跨多个事务写入，中途失败会留下半成品；恢复逻辑在多条路径上都缺 `note_id` 守卫，同一次运行可能产出重复笔记；幂等键取自随机 `run_id`，相同输入无法收敛到同一条记录。

二是**超时语义不统一**。本地超时与网关超时被当成两类错误处理，同一个原因在不同 operation 上得到不同的重试决策；退避基数偏小且无抖动，多路并发重试会同时撞向上游。

三是**容量保护缺位**。中转站限制的是请求体字节数，代码里却只有 token 估算；一个名叫 `truncate_chars` 的函数原样返回入参，长度控制形同虚设。更隐蔽的是超时通路：`DeepNoteRunEvent::TimeoutDetected` 定义了却从未被派发，超时和 panic 共用同一个转移分支，用户等满预算却拿不到任何已完成章节。

四是**存储可靠性**。数据库未开 WAL，写事务阻塞读，而这条管线有 15 秒心跳加并行 worker；schema 迁移挂在 `open_connection` 上，127 个数据访问点每次都要重跑一遍。

---

## 2. 逐项实施记录

### P0-1 ~ P0-5：正确性

Persisting 阶段改为单事务，新增 store 方法把笔记写入、章节落盘、相位推进收进一次提交，中途失败不留残留。`dispatch_checkpoint` 与 `run_drafting_task` 两个入口分别补上 `note_id` 守卫——两条路径都能进入 Persisting，只补一处等于没补。幂等键改为从输入内容派生，相同对话与相同附件快照两次运行得到同一个键。

`list_resumable` 这一项**偏离了计划**。计划要求把 `note_id IS NULL` 条件推广到 persisting 相位，但那样写会让一个已经产出笔记、却卡在 persisting 的 run 永远不出现在可恢复列表里，对话因此被一个永不收敛的任务卡死。改为自愈式的 `finalize_persisted_note_pipeline_runs`：识别出「已有 note_id 且输出完整」的 persisting run，直接把它推进到 Done，而不是把它藏起来。

P0-5 的验收标准也重述了。计划原文要求「相同输入两次产生相同 key」，但 `note_pipeline_outputs.idempotency_key` 上有全局 UNIQUE 索引，第二次插入必然冲突——按字面实现无法通过。改为在键里加 generation 后缀区分重建代际，验收改为「相同输入同一代际内收敛到同一条输出」。

### P0-6：统一超时语义

`should_retry_note_model_call` 里 `deepNoteChunk` 漏在否决名单外。补入后，本地超时与网关超时在所有 operation 上得到一致处理。顺带修掉一个计划没点名的浪费：`execute_chunk_digest_job` 在超时后仍会投一份同样大的载荷去「修复」，只是再等一个 300 秒的超时。

### P0-7：流式保活

非流式请求在生成期间连接完全静默，中转站的 idle 超时远短于长文生成所需时间，于是我们主动撞上网关的 504。改为用流式请求换取一个非流式响应：每来一个 token 就有字节流动，idle 计时器被持续重置，而调用方拿到的仍是完整的 `ModelResponse`，与 `complete` 完全同构。

这一项**偏离计划**：开关做成了 `AppSettings` 上的全局项，而非计划设想的按 provider 配置。理由是这个能力的失效模式是「上游不吃流式」，而用户判断的粒度就是「我这个中转站行不行」，按 provider 拆只是把同一个判断重复多遍。回落时必留事件痕迹，它是排查 504 时区分「没开流式」和「流式被拒」的唯一线上证据。

### P0-8：退避基数与抖动

`RateLimited` 的退避基数提到 2000ms 以上，并加入抖动。没有抖动时，多路并发重试会在同一时刻齐发，等于把一次限流放大成一轮自伤。

### P0-9：请求侧字节硬闸

新增 `dispatcher::request_body_bytes`，按目标协议构建请求体并返回**序列化后**的字节数。量字节而不是 token，是因为网关限的是 body 大小：一个汉字按 token 估算是 1，UTF-8 却是 3 字节；base64 图片几乎不占 token 估算却能吃掉几 MB。量的必须是真正会发出去的那个 body，不能量 prompt 字符串。

`CompleteExecution` 增加 `max_request_bytes`，闸门 `enforce_request_byte_limit` 超限即返回 `ContextLengthExceeded`。**错误 kind 的选择是这道闸能起作用的关键**：调用方靠 kind 判断「该缩载荷了」，`should_fallback_to_chunked_planner` 认的就是这个 kind，会把直出规划降级成分块规划。报成 `InvalidConfiguration` 会让它变成一个死错误，用户看到的是任务失败而不是自动缩小重试。

闸门放在**每轮**而非每次尝试：请求体在一轮内不变，重试同一份载荷没必要再称；但每轮都要称，因为工具结果会不断追加进 `request.messages`。

限额分两档（计划未指定）：纯文本 2 MiB，带图 16 MiB。base64 图片比原文件再大三分之一，是最容易悄悄超限的一类载荷，让它去挤文本档会误伤正常的视觉请求。

`truncate_chars` **直接删除**，而不是补一个诚实的实现。计划允许两种处置，删除的理由是：截断 transcript 会静默丢掉 `message-id` 锚点，而后续引用校验会把丢失的来源判成模型编造——一个更难查的错误。长度控制交给两道既有机制：规划阶段转分块，发出前字节硬闸兜底。这个契约写进了 `transcript` 的文档注释并有测试锁定。

### P0-10：三级墙钟预算

这一项的勘察改变了实现方式，记录三个发现。

**`TimeoutDetected` 在改动前从未被派发。** 它和 `PanicDetected` 合并在同一个转移分支里。根因是 `transition_to` 由**目标相位**反推事件，而超时和 panic 无法用目标相位区分。所以必须另开入口 `DeepNoteRunMachine::timeout()`——只拆分支不够。

**相位分流**：起草中超时落 `Assembling` 配 `SkipUnfinishedSections`（部分交付）；起草前后超时落 `Blocked` 配 `PersistTimeout`。phase 与 effect 双双区别于 panic，这正是计划的验收标准。落 `Blocked` 而非 `Error` 是因为 `Blocked` 本就接受 `RestartRequested`，`prepare_note_pipeline_retry` 也已认 `"blocked"`，用户可直接重启。

**墙钟只能从事件表汇总。** section 是并行执行的，那些任务持有 runtime 快照而非 `&mut runtime`（否则借用检查过不去），增量只能落在 `note_pipeline_events` 的 `durationMs` 上。从 DB 汇总还顺带跨续跑正确——内存计数在恢复时归零，等于每次重启白送一份完整预算。为此新增 `sum_note_pipeline_upstream_wall_clock_ms`，**刻意不复用** `list_note_pipeline_events`：后者把 limit 夹到 500，长 run 会被静默截断，而截断方向是少算，闸门于是永远不触发。也没用 `json_extract`——这个 codebase 里 JSON1 零使用，在无法本地编译的环境引入第一个依赖不值得。

两道闸门都**必须前置到 `transition(.., InProgress)` 之前**。进了那一步节点就被占住且 `attempt_count` 已经加过，事后再拦既浪费一次尝试，又可能把节点留在非终态上，`has_unfinished_sections()` 于是永远为真、循环不退出。

需要如实说明两处语义边界。一是 section 级闸门的**作用域是跨 run 续跑**，不是单次 run 内的抢占：`ready_section_ids` 只返回 `Ready` 节点，一个 section 派发一次就进 `InProgress`，重试在 `execute_dag_section` 内部循环，首次派发时 elapsed 恒为 0。它真正生效的场景是暂停或崩溃后续跑时，不再重新派发一个已经烧掉 15 分钟的 section。二是 `upstream_wall_clock_ms` 是**累计调用耗时求和**而非真实经过时间，并发为 2 时两路各 5 分钟算成 10 分钟——这是刻意的，预算要管住的是上游总消耗。

> **更正（2026-08-30）**：上段第一处语义边界的**计时基准是错的**，已修。当时的实现在派发时记 `section_started_at`，闸门按 `now - section_started_at` 判定 —— 而本段自己指出这道闸门唯一生效的场景就是续跑，于是「关掉应用过夜」这一最普通的续跑就会让所有在途 section 的时刻差变成十几个小时，被整批判定超时跳过，用户拿到一篇静默缺章的笔记，而那些 section 可能连一次上游调用都没跑完。更矛盾的是同段第二处边界已经写明 run 级只累计真实调用耗时、不用经过时间 —— 两级预算当时用了两种时间口径。现改为在 section 任务内部量本轮实际执行时长，结束时累加进 `DeepNoteRuntimeState::section_active_ms`（成功、取消、失败三条分支都累加，时间是实际花掉的，失败不退款），闸门只比较累计值。语义意图不变：真的烧掉 15 分钟执行时间的 section 照旧跳过。残留一处有界的宽松：硬崩溃（断电、进程被杀）时本轮时长来不及落盘，该 section 的累计值回到上一次已落盘的数，等于多给一轮预算；兜底是 run 级墙钟，它从 `note_pipeline_events` 汇总因而崩溃安全。失效方向从「静默少交付用户要的章节」变成「可能比预算多跑一会」，这是刻意选的方向。

### P0-11 / P0-12：WAL 与备份伴生文件

两者必须成对发布，且 P0-12 先行。只开 WAL 不改备份会产出缺失最近事务的备份文件，比不开 WAL 更糟。

先做备份侧：新增 `MANAGED_DATABASES` 与 `SQLITE_SIDECAR_SUFFIXES` 两个常量集中受管库与伴生后缀；复制前对源库做 TRUNCATE 检查点（`copy_tree` 是逐文件复制，主库和 `-wal` 是两个不同时刻的快照，WAL 开着时中间落进一次写就会拷出撕裂的三件套）；校验后清理伴生文件。清理的顺序有依赖不可重排：先打开让 SQLite 重放 WAL，再检查点把内容折进主库，最后才删——反过来先删再检查点会真的丢事务。

再开 WAL：`configure_sqlite_concurrency` 设 `journal_mode = WAL` 与 `synchronous = NORMAL`。`journal_mode` 是查询式 PRAGMA，必须用 `query_row` 读取，用 `execute_batch` 会因为「有返回行」而报错。不支持 WAL 时（内存库、某些网络文件系统）不硬失败，退回默认模式仍然可用。

### P0-13：migrate 移出 open_connection

新增 `pub fn initialize()` 负责建目录与迁移，由 `AppState::new` 在启动时调用一次并**硬失败**——迁移没成功就继续跑，后面每次查询都会撞在缺失的表上，报出来的错误还指不到真正原因。`open_connection` 只保留连接级设置（`foreign_keys`、`busy_timeout` 都不随文件持久化）。

按计划要求「逐个改为显式初始化，不要图省事保留 migrate 调用」，33 个测试构造点逐一补上 `initialize()`。其中五处**刻意不补**：那些测试在验证「重开 Repository 后数据仍在」，补了反而弱化断言——不补才能真正验证 `open_connection` 独立可用，而这正是生产路径的形状。

`open_connection` 里的 WAL 设置改为容错而非 `?`：它是 127 个数据访问点的公共入口，为一个已在 `initialize` 生效的数据库级设置引入新的失败面不值得。

### P0-14：HTTP 客户端

修正一处误导注释。原注释声称 deep-note「不共享这个全局上限」，这是错的——它就是同一个 `Client`，deep-note 只是因为自己的 attempt 超时（各档全部 ≤420 秒）总是先触发，900 秒从未生效。补 `pool_idle_timeout`（中转站常在网关侧静默关闭空闲连接，本地却仍认为可用，下次请求复用到死连接立刻失败）与 `tcp_keepalive`（管的是「对端已经没了但我们不知道」，没有它一条断掉的连接会挂到 900 秒上限）。

---

## 3. 复核发现并修正的两个问题

改完后派独立子代理逐条复核，抓到两个真问题。

**一个静默写错，由本轮改动引入。** `transition_note_pipeline_phase_in_transaction` 写库用的是 `transition.next_state`，不是调用方传的 `target`。我最初把 `transition_to` 里的 `Blocked` 映射成 `TimeoutDetected`，而 `TimeoutDetected` 在起草中的相位下会落到 `Assembling`——于是服务层写 `Blocked`、库里静默落成 `Assembling`，而 `phase_expects_background_worker(Assembling)` 为真，run 会永久卡在一个「看起来还在跑」的相位上。改动前这行会明确报错，改动后变成静默写错，更难发现。

修法是消除歧义本身：新增 `TimeoutWithoutOutput` 事件专门表达「墙钟耗尽且无可交付产出」，让 `Blocked` 这个 target 只有唯一的 next_state。并在 store 里加一道断言——目标相位与状态机算出的相位不一致就直接报错，把同类隐患永久变成明确失败而非静默写错。

**两个新测试无效。** `prepare_migration` 只写迁移日志，真正执行迁移的是下一次 `bootstrap`，两个测试都漏了这一步——一个必然失败，另一个在不存在的目录里断言「文件不存在」，恒真。更深一层：写入连接在块结束时 drop，而 SQLite 在最后一条连接关闭时会自动检查点并删除 `-wal`，「事务只活在 WAL 里」这个前提本身就不成立。改为让写入连接跨过迁移继续存活，并加一条前提校验断言 `-wal` 此刻确实存在——否则测试无法证伪「不做检查点会丢事务」。

复核同时确认无误的部分：新增 effect 变体无需补 match（该枚举全仓无 match）；两个结构体的所有字面量构造点已补齐；起草循环里对 `runtime` 两个不同字段同时取可变与不可变借用属于字段级不相交借用，合法；`PRAGMA synchronous` 返回整数、断言 `== 1` 对应 NORMAL 正确。

---

## 4. 与计划的偏离汇总

| 项 | 计划 | 实际 | 理由 |
| --- | --- | --- | --- |
| P0-4 | `note_id IS NULL` 推广到 persisting | 自愈式 `finalize_persisted_note_pipeline_runs` | 按计划实现会让已产出笔记的 run 永不可恢复，对话被卡死 |
| P0-5 | 「相同输入两次产生相同 key」 | 加 generation 后缀，验收重述为同代际内收敛 | 全局 UNIQUE 索引下原验收标准不可满足 |
| P0-7 | 按 provider 配置 | 全局 `AppSettings` 开关 | 用户判断粒度就是「这个中转站行不行」 |
| P0-9 | 处置 `truncate_chars`（实现或删除） | 删除 | 截断会静默丢 `message-id`，被引用校验判成模型编造 |
| P0-9 | 单一字节上限 | 文本 2 MiB / 带图 16 MiB 两档 | base64 图片挤文本档会误伤正常视觉请求 |
| P0-10 | 拆分转移分支后派发 | 拆分支 + 另开 `timeout()` 入口 + 新增 `TimeoutWithoutOutput` | 目标相位驱动的派发无法表达超时；一 target 对多 next_state 会静默写错 |

计划中若干行号已漂移（`store.rs:4165, 4216` 的真实转移点是 `4531`），实施时以符号检索为准。

## 5. 验证状态

实施时沙箱无 Rust 工具链，Rust 侧改动为逐区域人工核对加子代理独立复核。**已于本机补做验证，见 `17-DeepNote-P0本机验证与死代码甄别.md`：`cargo test` 406 项全绿，但编译暴露了一个本文档当时无法发现的错误** —— P0-13 新增的测试引用 `LIBRARY_DIRECTORY_NAME`，而 `store.rs` 的 tests 模块用的是显式导入列表，漏了这个常量，`lib test` 因 E0425 无法编译。前端 `npx tsc --noEmit` 通过（本轮未触碰前端，仅确认无回归）。

同一轮验证还查明：本文档第 2 节声称 P0-10「另开 `timeout()` 入口」后超时通路即告打通，这一点**不准确**。`timeout()` 至今没有生产调用点，部分交付实际由起草循环直接操作 scheduler 达成；起草之外的相位（尤其 `Analyzing`）仍无墙钟闸门。详见 17 号文档第 3 节。

新增测试覆盖：字节闸门的 kind 契约与边界、四种协议的字节计量、多字节文本按字节计重、transcript 保留全部来源锚点、墙钟与请求数两个维度独立、饱和加法、存量运行时 JSON 免迁移反序列化、超时与 panic 在每个进行中相位上 phase 与 effect 均不同、请求 `Blocked` 必落 `Blocked`、section 起点只记首次且容忍时钟回拨、WAL 模式与 synchronous 级别、schema 由 `initialize` 而非开连接建立、WAL 内事务随迁移到达目标库、目标目录无残留伴生文件。
