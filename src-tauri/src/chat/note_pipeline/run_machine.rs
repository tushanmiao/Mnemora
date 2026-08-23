use crate::{
    library::types::NotePipelinePhase,
    task_runtime::{StateMachine, Transition, TransitionError},
};

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
                E::PanicDetected | E::TimeoutDetected,
            ) => ok(
                S::Error,
                vec![DeepNoteRunEffect::PersistFailure],
                "任务异常终止",
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
            _ => DeepNoteRunEvent::AdvanceTo(target),
        };
        Self::transition(current, &event, &())
    }
}

#[cfg(test)]
mod tests {
    use super::{DeepNoteRunEvent, DeepNoteRunMachine};
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
}
