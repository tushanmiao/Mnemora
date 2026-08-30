use crate::{
    library::types::NotePipelinePhase,
    task_runtime::{StateMachine, Transition, TransitionError},
};

/// 深度笔记运行的完整事件集。
///
/// `#[allow(dead_code)]`：多数具名事件没有生产构造点，因为服务层统一走
/// `transition_to`，由**目标相位**反推事件、大部分落到 `AdvanceTo(target)`。具名
/// variant 仍被 `transition` 的 match 臂与单测使用，删掉它们就要同时改转移表。
///
/// 一个例外值得单独记住：`TimeoutDetected` 只由 `DeepNoteRunMachine::timeout()`
/// 构造，而那个入口尚未接入生产路径（见 `timeout()` 的文档注释）。它零构造是
/// **未接线**，不是设计如此。
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeepNoteRunEvent {
    StartRequested,
    OutlineGenerated,
    OutlineAdjustmentRequested,
    OutlineConfirmed,
    PauseRequested,
    ResumeRequested,
    CancelRequested,
    WorkerStopped,
    DagCompleted,
    PersistenceCompleted,
    RetryRequested,
    RestartRequested,
    PanicDetected,
    TimeoutDetected,
    /// 墙钟耗尽且**尚无可交付产出**。
    ///
    /// 与 `TimeoutDetected` 分开，是因为 `transition_to` 由目标相位反推事件，而
    /// `TimeoutDetected` 有两个可能的目标（有产出去 `Assembling`，无产出去
    /// `Blocked`）。让一个 target 对应两个 next_state 会导致调用方写 `Blocked`、
    /// 数据库里却落成 `Assembling` —— 静默写错，比报错难查得多。
    TimeoutWithoutOutput,
    AdvanceTo(NotePipelinePhase),
    ForceCancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeepNoteRunEffect {
    StartAnalysis,
    StartDrafting,
    RequestWorkerStop,
    ResumeWorker,
    MarkCancelled,
    PersistFailure,
    /// 墙钟耗尽但已有起草产出：把未完成的 section 标记跳过，交付已完成部分。
    ///
    /// 与 `PersistFailure` 分开是这次修复的核心 —— 两者合并时，超时和 panic 都
    /// 走「整体失败」，用户已经等了 90 分钟却什么都拿不到。
    SkipUnfinishedSections,
    /// 墙钟耗尽且尚无可交付产出：记录超时原因，停在可重启状态。
    ///
    /// 不复用 `PersistFailure`：调用方要能区分「代码炸了」和「上游太慢」，
    /// 前者该看堆栈，后者该提示用户缩小范围或换模型。
    PersistTimeout,
}

pub struct DeepNoteRunMachine;

impl StateMachine for DeepNoteRunMachine {
    type State = NotePipelinePhase;
    type Event = DeepNoteRunEvent;
    type Context = ();
    type Effect = DeepNoteRunEffect;

    fn transition(
        state: NotePipelinePhase,
        event: &DeepNoteRunEvent,
        _: &(),
    ) -> Result<Transition<NotePipelinePhase, DeepNoteRunEffect>, TransitionError> {
        use DeepNoteRunEvent as E;
        use NotePipelinePhase as S;
        let invalid = |reason| {
            Err(TransitionError::Invalid {
                state: state.as_str().into(),
                event: format!("{event:?}"),
                reason,
            })
        };
        let terminal = || {
            Err(TransitionError::Terminal {
                state: state.as_str().into(),
            })
        };
        let ok = |next_state, effects, reason| {
            Ok(Transition {
                next_state,
                effects,
                reason,
            })
        };

        match (state, event) {
            (S::Preflight, E::StartRequested) => ok(
                S::Analyzing,
                vec![DeepNoteRunEffect::StartAnalysis],
                "开始分析",
            ),
            (S::Analyzing, E::OutlineGenerated) => ok(S::AwaitingOutline, vec![], "提纲已生成"),
            (S::AwaitingOutline, E::OutlineAdjustmentRequested) => ok(
                S::Analyzing,
                vec![DeepNoteRunEffect::StartAnalysis],
                "重新分析提纲",
            ),
            (S::AwaitingOutline, E::OutlineConfirmed) => ok(
                S::Compiling,
                vec![DeepNoteRunEffect::StartDrafting],
                "确认提纲",
            ),
            (
                S::Drafting
                | S::Validating
                | S::Compiling
                | S::Queued
                | S::Replanning
                | S::Analyzing,
                E::PauseRequested,
            ) => ok(
                S::Paused,
                vec![DeepNoteRunEffect::RequestWorkerStop],
                "暂停任务",
            ),
            (S::Paused, E::ResumeRequested) => ok(
                S::Drafting,
                vec![DeepNoteRunEffect::ResumeWorker],
                "恢复任务",
            ),
            (
                S::Preflight
                | S::Analyzing
                | S::AwaitingOutline
                | S::Compiling
                | S::Queued
                | S::Drafting
                | S::Validating
                | S::Replanning
                | S::Assembling
                | S::Persisting
                | S::Paused
                | S::Blocked
                | S::Error,
                E::CancelRequested,
            ) => ok(
                S::Cancelling,
                vec![DeepNoteRunEffect::RequestWorkerStop],
                "请求停止任务",
            ),
            (S::Cancelling, E::WorkerStopped) => ok(
                S::Cancelled,
                vec![DeepNoteRunEffect::MarkCancelled],
                "后台任务已停止",
            ),
            (S::Drafting | S::Validating | S::Compiling, E::DagCompleted) => {
                ok(S::Assembling, vec![], "DAG 已完成")
            }
            (S::Persisting, E::PersistenceCompleted) => ok(S::Done, vec![], "笔记已保存"),
            // 兼容历史恢复/仓储测试中直接写入最终产物的调用点；新 Worker 应走 Persisting。
            (
                S::Preflight
                | S::Analyzing
                | S::AwaitingOutline
                | S::Compiling
                | S::Queued
                | S::Drafting
                | S::Validating
                | S::Replanning
                | S::Assembling,
                E::AdvanceTo(S::Done),
            ) => ok(S::Done, vec![], "兼容直接完成写入"),
            (S::Error | S::Blocked, E::RetryRequested) => ok(
                S::Drafting,
                vec![DeepNoteRunEffect::ResumeWorker],
                "重试失败步骤",
            ),
            (S::Error | S::Blocked | S::Cancelled, E::RestartRequested) => ok(
                S::Preflight,
                vec![DeepNoteRunEffect::StartAnalysis],
                "重新生成任务",
            ),
            (
                S::Preflight
                | S::Analyzing
                | S::Compiling
                | S::Queued
                | S::Drafting
                | S::Validating
                | S::Replanning
                | S::Assembling
                | S::Persisting,
                E::PanicDetected,
            ) => ok(
                S::Error,
                vec![DeepNoteRunEffect::PersistFailure],
                "任务异常终止",
            ),
            // 超时发生在已经有起草产出之后：走部分交付。
            //
            // 相位落在 `Assembling` 而不是 `Error`，效果是 `SkipUnfinishedSections`
            // 而不是 `PersistFailure` —— phase 和 effect 双双区别于 panic，
            // 这正是本次修复要锁住的性质。
            (S::Drafting | S::Validating | S::Replanning, E::TimeoutDetected) => ok(
                S::Assembling,
                vec![DeepNoteRunEffect::SkipUnfinishedSections],
                "墙钟预算耗尽，交付已完成部分",
            ),
            // 超时发生在起草之前（预检、分析、编排、排队）或收尾阶段：没有可交付的
            // section，只能停下。落在 `Blocked` 而不是 `Error`：`Blocked` 已经允许
            // `RestartRequested` 与 `AdvanceTo`，用户可以直接重启而不必先清错误态。
            //
            // `TimeoutWithoutOutput` 也收在这里，且额外接受起草中的相位 —— 它表达的
            // 是「墙钟耗尽且一个 section 都没做出来」，此时起草阶段同样无可交付。
            (
                S::Preflight
                | S::Analyzing
                | S::Compiling
                | S::Queued
                | S::Assembling
                | S::Persisting,
                E::TimeoutDetected,
            )
            | (
                S::Preflight
                | S::Analyzing
                | S::Compiling
                | S::Queued
                | S::Drafting
                | S::Validating
                | S::Replanning
                | S::Assembling
                | S::Persisting,
                E::TimeoutWithoutOutput,
            ) => ok(
                S::Blocked,
                vec![DeepNoteRunEffect::PersistTimeout],
                "墙钟预算耗尽且无可交付产出",
            ),
            (_, E::ForceCancelled) if !matches!(state, S::Done) => ok(
                S::Cancelled,
                vec![DeepNoteRunEffect::MarkCancelled],
                "强制收敛为已取消",
            ),
            (S::Preflight, E::AdvanceTo(S::Analyzing))
            | (S::Preflight, E::AdvanceTo(S::AwaitingOutline))
            | (S::Analyzing, E::AdvanceTo(S::AwaitingOutline))
            | (S::AwaitingOutline, E::AdvanceTo(S::Analyzing))
            | (S::AwaitingOutline, E::AdvanceTo(S::Compiling))
            | (S::Compiling, E::AdvanceTo(S::Queued))
            | (S::Compiling, E::AdvanceTo(S::Drafting))
            | (S::Queued, E::AdvanceTo(S::Drafting))
            | (S::Drafting, E::AdvanceTo(S::Validating))
            | (S::Validating, E::AdvanceTo(S::Drafting))
            | (S::Drafting, E::AdvanceTo(S::Replanning))
            | (S::Replanning, E::AdvanceTo(S::Drafting))
            | (S::Drafting, E::AdvanceTo(S::Assembling))
            | (S::Validating, E::AdvanceTo(S::Assembling))
            | (S::Assembling, E::AdvanceTo(S::Persisting))
            | (S::Persisting, E::AdvanceTo(S::Done))
            | (S::Paused, E::AdvanceTo(S::Analyzing))
            | (S::Paused, E::AdvanceTo(S::AwaitingOutline))
            | (S::Paused, E::AdvanceTo(S::Drafting))
            | (S::Error, E::AdvanceTo(S::Analyzing))
            | (S::Error, E::AdvanceTo(S::AwaitingOutline))
            | (S::Error, E::AdvanceTo(S::Drafting))
            | (S::Blocked, E::AdvanceTo(S::Analyzing))
            | (S::Blocked, E::AdvanceTo(S::AwaitingOutline))
            | (S::Blocked, E::AdvanceTo(S::Drafting))
            | (S::Cancelled, E::AdvanceTo(S::Analyzing))
            | (S::Cancelled, E::AdvanceTo(S::AwaitingOutline))
            | (S::Cancelled, E::AdvanceTo(S::Drafting)) => {
                let E::AdvanceTo(target) = event else {
                    unreachable!()
                };
                ok(*target, vec![], "管线阶段推进")
            }
            (S::Done, _) => terminal(),
            _ => invalid("当前阶段不允许该操作"),
        }
    }
}

impl DeepNoteRunMachine {
    pub fn transition_to(
        current: NotePipelinePhase,
        target: NotePipelinePhase,
    ) -> Result<Transition<NotePipelinePhase, DeepNoteRunEffect>, TransitionError> {
        if current == target {
            return Ok(Transition {
                next_state: current,
                effects: Vec::new(),
                reason: "幂等状态写入",
            });
        }
        let event = match target {
            NotePipelinePhase::Cancelling => DeepNoteRunEvent::CancelRequested,
            NotePipelinePhase::Cancelled if current == NotePipelinePhase::Cancelling => {
                DeepNoteRunEvent::WorkerStopped
            }
            NotePipelinePhase::Cancelled => DeepNoteRunEvent::ForceCancelled,
            NotePipelinePhase::Paused => DeepNoteRunEvent::PauseRequested,
            NotePipelinePhase::Error => DeepNoteRunEvent::PanicDetected,
            // `Blocked` 只有超时一条入路，映射不存在歧义（对照 `Error` 只有 panic
            // 一条入路）。这样服务层沿用现有的 `update_note_pipeline_phase(.., Blocked, ..)`
            // 就能把超时写进库，不必给 store 再开一个并行入口。
            //
            // 必须用 `TimeoutWithoutOutput` 而不是 `TimeoutDetected`：后者在起草中的
            // 相位下会落到 `Assembling`，而 store 写库用的是 `transition.next_state`，
            // 于是调用方写 `Blocked`、库里却成了 `Assembling`，run 永久卡在一个
            // 「看起来还在跑」的相位上。
            NotePipelinePhase::Blocked => DeepNoteRunEvent::TimeoutWithoutOutput,
            _ => DeepNoteRunEvent::AdvanceTo(target),
        };
        Self::transition(current, &event, &())
    }

    /// 派发一次真实的超时事件。
    ///
    /// 必须是独立入口而不是复用 `transition_to`：后者由**目标相位**反推事件，
    /// 而超时和 panic 无法用目标相位区分（在起草前两者都想去一个非 Done 的
    /// 终止态）。事实上正因为缺了这个入口，`TimeoutDetected` 在此之前从未被
    /// 任何代码派发过 —— 整条超时通路都是死代码。
    ///
    /// **接线状态：已接入生产路径。** 起草批次派发前的双维度预算闸从这里取得
    /// `SkipUnfinishedSections`，Analyzing 每次逻辑调用前的预算闸从这里取得
    /// `PersistTimeout`。服务层只执行 effect，不再自行维护第二份相位分流规则。
    pub fn timeout(
        current: NotePipelinePhase,
    ) -> Result<Transition<NotePipelinePhase, DeepNoteRunEffect>, TransitionError> {
        Self::transition(current, &DeepNoteRunEvent::TimeoutDetected, &())
    }
}

#[cfg(test)]
mod tests {
    use super::{DeepNoteRunEffect, DeepNoteRunEvent, DeepNoteRunMachine};
    use crate::{library::types::NotePipelinePhase, task_runtime::StateMachine};

    #[test]
    fn cancellation_requires_stop_ack_before_terminal_state() {
        let next = DeepNoteRunMachine::transition(
            NotePipelinePhase::Drafting,
            &DeepNoteRunEvent::CancelRequested,
            &(),
        )
        .unwrap();
        assert_eq!(next.next_state, NotePipelinePhase::Cancelling);
        let done = DeepNoteRunMachine::transition(
            NotePipelinePhase::Cancelling,
            &DeepNoteRunEvent::WorkerStopped,
            &(),
        )
        .unwrap();
        assert_eq!(done.next_state, NotePipelinePhase::Cancelled);
    }

    #[test]
    fn completed_run_rejects_late_worker_result() {
        assert!(DeepNoteRunMachine::transition(
            NotePipelinePhase::Done,
            &DeepNoteRunEvent::CancelRequested,
            &()
        )
        .is_err());
    }

    /// 验收标准：run 级预算耗尽时 phase 与 effect 均可区分于 panic。
    ///
    /// 两者曾共用同一个转移分支，超时因此被当成崩溃处理 —— 用户等满整个预算
    /// 却拿不到任何已完成的 section。这个测试遍历所有进行中相位，逐一锁住
    /// 「相位不同」且「效果不同」。
    #[test]
    fn timeout_is_distinguishable_from_panic_in_every_in_flight_phase() {
        const IN_FLIGHT: [NotePipelinePhase; 9] = [
            NotePipelinePhase::Preflight,
            NotePipelinePhase::Analyzing,
            NotePipelinePhase::Compiling,
            NotePipelinePhase::Queued,
            NotePipelinePhase::Drafting,
            NotePipelinePhase::Validating,
            NotePipelinePhase::Replanning,
            NotePipelinePhase::Assembling,
            NotePipelinePhase::Persisting,
        ];
        for phase in IN_FLIGHT {
            let timeout = DeepNoteRunMachine::timeout(phase)
                .unwrap_or_else(|error| panic!("{phase:?} 必须能接受超时事件：{error:?}"));
            let panicked =
                DeepNoteRunMachine::transition(phase, &DeepNoteRunEvent::PanicDetected, &())
                    .unwrap_or_else(|error| panic!("{phase:?} 必须能接受 panic 事件：{error:?}"));
            assert_ne!(
                timeout.next_state, panicked.next_state,
                "{phase:?}：超时与 panic 落到了同一个相位，调用方无从区分"
            );
            assert_ne!(
                timeout.effects, panicked.effects,
                "{phase:?}：超时与 panic 产生了同样的效果"
            );
            assert_eq!(
                panicked.next_state,
                NotePipelinePhase::Error,
                "panic 的行为不应被本次改动影响"
            );
        }
    }

    /// 已经在起草的 run 超时后必须走部分交付：已完成的 section 是有价值的产出。
    #[test]
    fn timeout_after_drafting_delivers_completed_sections() {
        for phase in [
            NotePipelinePhase::Drafting,
            NotePipelinePhase::Validating,
            NotePipelinePhase::Replanning,
        ] {
            let transition = DeepNoteRunMachine::timeout(phase).unwrap();
            assert_eq!(transition.next_state, NotePipelinePhase::Assembling);
            assert_eq!(
                transition.effects,
                vec![DeepNoteRunEffect::SkipUnfinishedSections]
            );
        }
    }

    /// 起草之前超时没有任何可交付内容，落在 `Blocked` 而不是 `Error`：
    /// `Blocked` 本来就接受 `RestartRequested`，用户可以直接重启。
    #[test]
    fn timeout_before_drafting_blocks_with_a_timeout_specific_effect() {
        for phase in [
            NotePipelinePhase::Preflight,
            NotePipelinePhase::Analyzing,
            NotePipelinePhase::Compiling,
            NotePipelinePhase::Queued,
        ] {
            let transition = DeepNoteRunMachine::timeout(phase).unwrap();
            assert_eq!(transition.next_state, NotePipelinePhase::Blocked);
            assert_eq!(transition.effects, vec![DeepNoteRunEffect::PersistTimeout]);
        }
        assert!(DeepNoteRunMachine::transition(
            NotePipelinePhase::Blocked,
            &DeepNoteRunEvent::RestartRequested,
            &()
        )
        .is_ok());
    }

    /// `transition_to(.., Blocked)` 算出的相位必须**就是** `Blocked`。
    ///
    /// store 写库用的是 `transition.next_state`，不是调用方传的 target。所以只要
    /// 某个 target 反推出的事件存在第二个可能的目标相位，调用方写 `Blocked`、
    /// 库里就会静默落成别的相位 —— 这里曾经因为映射到 `TimeoutDetected` 而在
    /// 起草中的相位下落到 `Assembling`，run 会永久停在一个「看起来还在跑」的状态。
    #[test]
    fn requesting_the_blocked_phase_always_lands_on_blocked() {
        for phase in [
            NotePipelinePhase::Preflight,
            NotePipelinePhase::Analyzing,
            NotePipelinePhase::Compiling,
            NotePipelinePhase::Queued,
            NotePipelinePhase::Drafting,
            NotePipelinePhase::Validating,
            NotePipelinePhase::Replanning,
            NotePipelinePhase::Assembling,
            NotePipelinePhase::Persisting,
        ] {
            let transition = DeepNoteRunMachine::transition_to(phase, NotePipelinePhase::Blocked)
                .unwrap_or_else(|error| panic!("{phase:?} 应当能写入 Blocked：{error:?}"));
            assert_eq!(
                transition.next_state,
                NotePipelinePhase::Blocked,
                "{phase:?}：请求 Blocked 却算出了 {:?}，store 会把它静默写进库",
                transition.next_state
            );
            assert_eq!(transition.effects, vec![DeepNoteRunEffect::PersistTimeout]);
        }
    }

    /// 已完成的 run 不受超时影响：迟到的超时判定不能把成功的任务打回去。
    #[test]
    fn timeout_never_reopens_a_finished_run() {
        for phase in [
            NotePipelinePhase::Done,
            NotePipelinePhase::Cancelled,
            NotePipelinePhase::Error,
        ] {
            assert!(
                DeepNoteRunMachine::timeout(phase).is_err(),
                "{phase:?} 已经是终止态，不应接受超时事件"
            );
        }
    }
}
