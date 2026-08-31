# 17-DeepNote P0 本机验证与死代码甄别

承接 `16-DeepNote-P0正确性超时语义与存储可靠性.md`。16 号那一轮实施在无 Rust 工具链的沙箱里完成，Rust 侧只做了人工核对与子代理复核，验证一直悬着。本轮在 Windows 本机补齐验证，并清理 `cargo check` 的 13 个 dead_code 警告。

所有改动停在工作区，未提交。

---

## 1. 本轮解决了什么

三件事，后两件是第一件的副产品：

一是**补齐 P0 的本机验证**。16 号文档留的 TODO 是「需在本机执行 `cargo test` 确认」。执行后发现一个编译错误，修掉之后 406 项测试全绿。

二是**甄清 13 个 dead_code 警告**。清理过程本身不难，难在**判断每个警告是不是真的死代码** —— 结果只有 3 个是，其余 10 个分别属于状态机建模、测试专用、以及一个真实的未接线功能。

三是**修正 16 号文档一处不准确的自述**。它声称 P0-10 打通了超时通路，实际没有。

---

## 2. 本机验证

### 2.1 唯一的编译错误

```
src\library\store.rs:6739:48: error[E0425]: cannot find value `LIBRARY_DIRECTORY_NAME` in this scope
error: could not compile `mnemora` (lib test) due to 1 previous error
```

P0-13 新增的测试 `schema_is_created_by_initialize_not_by_opening_a_connection` 需要手工建库目录（因为 P0-13 的整个要点就是 `open_connection` 不再建目录），于是引用了模块级私有常量 `LIBRARY_DIRECTORY_NAME`。但 `store.rs` 的 tests 模块用的是**显式导入列表** `use super::{LibraryRepository, LIBRARY_SCHEMA_VERSION}`，不是 `use super::*`，所以这个常量不在作用域内。补进导入即修复。

值得记一笔的是**这个错误只出现在 `lib test` target 上**。`cargo check` 不带 `--all-targets` 时编译通过、退出码为 0 —— 只测 lib 会漏掉全部测试代码的编译错误。本轮起验证命令统一带 `--all-targets`。

另有一处观察陷阱：`cargo check ... 2>&1 | tail -80` 的退出码是 `tail` 的，恒为 0。误判为「编译通过」的风险就在这里，判断编译结果必须看 cargo 自己的输出，不能看管道退出码。

### 2.2 验证结果

| 检查 | 结果 |
| --- | --- |
| `cargo check --all-targets` | 通过，**0 警告**（清理前 13 个） |
| `cargo test` | **406 passed / 0 failed** |
| `npx tsc --noEmit` | 通过 |
| `npx vitest run` | **221 passed / 0 failed** |

P0 新增测试均在跑并通过，其中几个正是 16 号文档复核阶段重写过的：`migration_preserves_transactions_that_still_live_in_the_wal`（P0-12，原版测试恒真无效）、`recovers_when_commit_finished_before_the_phase_was_persisted`（P0-1/P0-4 的自愈收尾）、`schema_is_created_by_initialize_not_by_opening_a_connection`（P0-13）。

至此 16 号文档的 P0 全部十四项可视为验证完成。

---

## 3. 一个必须记录的发现：P0-10 的状态机建模没有接线

16 号文档第 2 节写 P0-10 时说「必须另开入口 `DeepNoteRunMachine::timeout()` —— 只拆分支不够」，读起来像是超时通路已经打通。**实际没有。**

`cargo check` 的证据很直接：`timeout()` 从未被调用，`TimeoutDetected` 从未被构造，effect `SkipUnfinishedSections` 与 `PersistTimeout` 在 `run_machine.rs` 之外零引用。它们只被单测覆盖。

真实的生产通路在起草循环里（`note_pipeline/service.rs` 的 `refresh_run_wall_clock` 调用点），走的是**直接操作 scheduler**：

```rust
if refresh_run_wall_clock(&state, &run_id, &mut runtime) {
    wall_clock_exhausted = true;
    scheduler.skip_unfinished_sections();
    // 写 runWallClockExhausted 事件、progress 提示「交付已完成的 N/M 个章节」
    break;
}
```

**功能效果是对的** —— 部分交付确实会发生，用户确实能拿到已完成章节。所以这不是一个用户可感知的缺陷，而是一处架构上的双轨：相位与 effect 的对应关系同时存在于状态机（被测试验证）和服务层（被真正执行）两个地方，而只有后者生效。

顺带确认一件好事：**P3-1 已经被 P0-10 顺手做掉了**。`md/plan/14-DeepNote/05-分阶段实施计划.md` 把 P3-1 描述为「接线 `skip_unfinished_sections`（`scheduler.rs:183-199` 当前零调用）」，现在它在起草循环里有了调用点。该计划项可以划掉。

### 3.1 剩下的真实缺口

**起草之外的相位没有墙钟闸门。** run 级闸门只在起草循环内检查，而 `Analyzing` 阶段要为每个来源分块各发一次摘要请求，是除起草外最能烧掉墙钟的阶段。那里超时既不落 `Blocked` 也不产生 `PersistTimeout`，只能等各档 attempt 超时自然堆叠。

补这个缺口时应当**从 `timeout()` 派发事件**，而不是再复制一份直接操作 scheduler 的代码 —— 相位与 effect 的对应关系只应有一处真相。这一项未做，记在此处。

### 3.2 更新（2026-08-30）：双轨已合并

本节记录的架构双轨**已消除**，按 3.1 建议的方向做的 —— 从 `timeout()` 派发，没有再复制一份直接操作 scheduler 的代码：

- `DeepNoteRunMachine::timeout()` 现有两处生产调用：`note_pipeline/service.rs:4716`、`:5638`
- `TimeoutDetected` 在 `run_machine.rs:206` 有独立分支与独立 effect，不再只被单测覆盖

因此本文与 `md/plan/14-DeepNote/05-分阶段实施计划.md` 第 161 行里「`timeout()` 至今零调用」的表述都已过期，两处一并更正。第 7 节表格中「`Analyzing` 等非起草相位的墙钟闸门」一项的补法指引依然有效。

---

## 4. 13 个 dead_code 警告的甄别

清理的前提是分类。逐个查证后（对比 `git grep HEAD` 确认是否本轮引入、读注释确认是否刻意保留、查计划文档确认后续是否要用），13 个警告分成四类，**只有 3 个该删**。

### 4.1 真死代码：删除（3 处）

| 符号 | 位置 | 判据 |
| --- | --- | --- |
| `ledger_analysis_prompt` | `note_pipeline/service.rs` | 生产用的是 `compact_ledger_analysis_prompt`（另一个函数，grep 时因子串匹配容易看漏）；本函数零调用，连测试都没有 |
| `LibraryNoteVersion` | `library/types.rs` | 带 `Serialize` + camelCase 的 DTO，全仓零引用 —— 为前端准备但从未接线 |
| `DeepNoteNodeType::as_str` / `parse` | `note_pipeline/types.rs` | 枚举本身有 `Serialize`/`Deserialize` derive，序列化由 serde 承担；这两个手写方法零调用。枚举本体被 `scheduler.rs` 大量使用，只删方法 |

### 4.2 状态机与错误类型的完整建模：加 `#[allow(dead_code)]` + 理由（3 处）

`AgentRunEvent` 的 `UserInputRequired` / `BudgetExceeded`、`DeepNoteRunEvent` 的十个 variant、`TransitionError::Stale`。

这些 variant 零构造的根因是设计使然：服务层统一走 `transition_to`，由**目标相位**反推事件、大部分落到 `AdvanceTo(target)`，具名事件因此没有生产构造点，但仍被 `transition` 的 match 臂和单测使用。**删 variant 就要同时删转移规则，等于把状态机的合法转移表改小 —— 那是功能变更，不是清理。**

`TransitionError::Stale` 对应 CAS 事务的版本冲突，现有各域状态机都在同一把锁内完成读—判—写，还没有并发的 CAS 写入方。保留它是因为 `task_runtime` 模块的契约就是「转移是纯决策，可在 CAS 事务产生副作用前先校验」，版本冲突是那个契约里的一等错误。

### 4.3 刻意的测试专用：加 `#[allow(dead_code)]` + 理由（4 处）

| 符号 | 为什么留 |
| --- | --- |
| `DeepNoteBudget::record_upstream_wall_clock` | 注释早已写明：生产从事件表汇总后整体赋值（并行 section 持有 runtime 快照而非 `&mut`），这个方法让预算的耗尽语义能被单测独立验证，不必拼 AppState 和事件表 |
| `create_note_with_sources_and_coverage` | 生产已改走 P0-1 的 `commit_deep_note_and_complete_run`；保留作为「建笔记 + 写来源 + 写覆盖快照」的最小可测单元 |
| `create_rebuilt_note_with_sources_and_coverage` | 同上，重建在新方法里走 `force_rebuild` 参数；保留供测试锁定「重建后旧笔记不再是更新锚点」 |
| `save_note_pipeline_section` | 生产统一走 `save_note_pipeline_section_checkpoint`（多写尝试计数、修订计数、证据与校验 JSON）；保留供测试在不构造那些 JSON 的情况下验证章节持久化 |
| `NotePipelinePhase::is_resumable` | 生产判定走 SQL（`list_resumable_note_pipeline_runs` 的相位白名单还要叠加 `cancelled` 的 `note_id IS NULL` 守卫与「排除更新的同会话 run」，纯相位谓词表达不了）；保留让「相位本身是否可恢复」有一处可单测的定义，不至于只存在于 SQL 字符串里 |

这里附带一个**值得后续处理的发现**：前三个方法说明测试与生产已经错位 —— 测试验证的是不再被生产调用的路径。`commit_deep_note_and_complete_run` 目前只被 `recovers_when_commit_finished_before_the_phase_was_persisted` 一个测试覆盖，而被它替代的旧方法反而有三四个测试。补强方向是让来源校验、覆盖快照、重建锚点这几条不变量改由新方法验证。本轮未做。

### 4.4 字段未读但不该删：加 `#[allow(dead_code)]` + 理由（2 处）

`NotePipelineSection` 的 `run_id` / `section_json` / `evidence_ids` / `validation_json` / `input_hash`，以及 `NotePipelineChunkDigest` 的 `semantic_calls` / `updated_at`。

两者都是 DB 行的忠实映射，字段与 `SELECT` 列表一一对应，删字段要同时改 SQL。而且它们**在计划里已有明确的将来用途**：P3-4 的章节内检查点要比对 `input_hash`；P3-3 要把 digest 缓存从「按 run_id 隔离」改成「全局 + TTL/LRU」，过期判据正是 `updated_at`。现在删掉，做 P3 时要连 SQL 一起改回来。

---

## 5. 处置方式的取舍

对 4.2–4.4 这 10 处，选择加 `#[allow(dead_code)]` 而不是删除或改 `#[cfg(test)]`，理由如下。

**不删**：上面每一处都写了具体判据，删除会丢失状态机的转移表、DB 行的完整映射，或让 P3 返工。

**不用 `#[cfg(test)]`**：技术上可行，但语义是错的。`#[cfg(test)]` 说的是「这是测试基础设施」，而这些符号说的是「这是产品代码的一部分，当前只有测试在调用它」—— 前者会误导下一个读代码的人，以为它们从来不属于生产 API。

**每一处都写了为什么留**。`#[allow(dead_code)]` 不加解释就是一个静音开关，下一个人只能重新做一遍本轮的甄别。注释里写清「生产路径改走了哪里」「将来哪一项计划要用」，才是这次清理真正的产物 —— 警告数从 13 归零只是副产品。

---

## 6. 验证状态

本轮全部命令在 Windows 本机执行：

```
cargo check --manifest-path src-tauri/Cargo.toml --all-targets   # 0 警告 0 错误
cargo test --manifest-path src-tauri/Cargo.toml                  # 406 passed / 0 failed
npx tsc --noEmit                                                 # 通过
npx vitest run                                                   # 221 passed / 0 failed
```

清理前后测试数一致（406），无新增无减少 —— 删除的三处本就零测试覆盖，加注解的十处行为未变。

---

## 7. 未做的事

| 项 | 说明 |
| --- | --- |
| `Analyzing` 等非起草相位的墙钟闸门 | 见第 3.1 节。补时应从 `DeepNoteRunMachine::timeout()` 派发，不要复制服务层的直接操作 |
| 测试与生产的错位 | 见 4.3 节末。让来源校验、覆盖快照、重建锚点改由 `commit_deep_note_and_complete_run` 验证 |
| P1 / P2 / P3 | 均未开始。P3-1 已由 P0-10 顺带完成，可从计划中划掉 |

> 本表是当轮快照。其中「状态机双轨」已于 2026-08-30 解决（见 3.2 节）；P1、P2、P3-1 至 P3-6 亦已完成，最新状态以 `md/plan/14-DeepNote/05-分阶段实施计划.md` 为准。
