use crate::{
    chat::note_pipeline::types::DeepNoteNodeStatus,
    task_runtime::{StateMachine, Transition, TransitionError},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DagNodeEvent {
    DependenciesSatisfied,
    DependencyFailed,
    ExecutionStarted,
    ExecutionSucceeded,
    ExecutionInterrupted,
    ValidationPassed,
    ValidationFailed,
    RevisionScheduled,
    RetryScheduled,
    AttemptLimitReached,
    PlanSuperseded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DagNodeEffect {
    StartWorker,
    MarkSuperseded,
}

pub struct DagNodeMachine;

impl DagNodeMachine {
    pub fn transition_to(
        current: DeepNoteNodeStatus,
        target: DeepNoteNodeStatus,
    ) -> Result<Transition<DeepNoteNodeStatus, DagNodeEffect>, TransitionError> {
        Self::transition_to_with_checkpoint(current, target, false)
    }

    /// 根据目标状态执行兼容转换。
    ///
    /// `checkpoint_present` 只为“已有权威产物，但节点状态尚未落盘”的恢复场景开放
    /// 直达 Completed。普通调度仍必须先进入 InProgress，避免把一个尚未执行的 Ready
    /// 节点误标为完成。
    pub fn transition_to_with_checkpoint(
        current: DeepNoteNodeStatus,
        target: DeepNoteNodeStatus,
        checkpoint_present: bool,
    ) -> Result<Transition<DeepNoteNodeStatus, DagNodeEffect>, TransitionError> {
        if current == target {
            return Ok(Transition {
                next_state: current,
                effects: Vec::new(),
                reason: "幂等节点状态写入",
            });
        }
        use DagNodeEvent as E;
        use DeepNoteNodeStatus as S;
        if target == S::Completed
            && checkpoint_present
            && matches!(
                current,
                S::Pending | S::Ready | S::Interrupted | S::NeedsReview | S::NeedsRevision
            )
        {
            return Ok(Transition {
                next_state: S::Completed,
                effects: Vec::new(),
                reason: "根据已有产物检查点完成节点",
            });
        }
        let event = match (current, target) {
            (S::Pending, S::Ready) => E::DependenciesSatisfied,
            (S::Pending, S::Blocked) => E::DependencyFailed,
            (S::Ready, S::InProgress) => E::ExecutionStarted,
            (S::Ready, S::Interrupted) => {
                return Ok(Transition {
                    next_state: target,
                    effects: Vec::new(),
                    reason: "调度在执行前被安全中断",
                })
            }
            (S::InProgress, S::NeedsReview) => E::ExecutionSucceeded,
            (S::InProgress, S::Completed) => {
                return Ok(Transition {
                    next_state: S::Completed,
                    effects: Vec::new(),
                    reason: "兼容原子执行与验证节点",
                })
            }
            (S::NeedsReview, S::Completed) => E::ValidationPassed,
            (S::NeedsReview, S::NeedsRevision) => E::ValidationFailed,
            (S::NeedsRevision, S::Ready) => E::RevisionScheduled,
            (S::InProgress, S::Ready) => E::RetryScheduled,
            (S::NeedsReview, S::InProgress) | (S::NeedsRevision, S::InProgress) => {
                return Ok(Transition {
                    next_state: target,
                    effects: vec![DagNodeEffect::StartWorker],
                    reason: "开始复核或修订",
                })
            }
            (S::InProgress, S::Failed) | (S::NeedsRevision, S::Failed) => E::AttemptLimitReached,
            (S::NeedsReview, S::Failed) => E::AttemptLimitReached,
            (S::InProgress, S::Interrupted) => E::ExecutionInterrupted,
            (
                S::Failed
                | S::Blocked
                | S::Interrupted
                | S::InProgress
                | S::NeedsReview
                | S::NeedsRevision,
                S::Pending,
            ) => {
                return Ok(Transition {
                    next_state: target,
                    effects: Vec::new(),
                    reason: "恢复节点检查点",
                })
            }
            (S::NeedsReview | S::NeedsRevision, S::Skipped)
            | (S::Pending | S::Ready | S::Blocked | S::Interrupted, S::Skipped) => {
                return Ok(Transition {
                    next_state: target,
                    effects: Vec::new(),
                    reason: "兼容调度器跳过未完成节点",
                })
            }
            (
                S::Pending | S::Ready | S::Interrupted | S::NeedsReview | S::NeedsRevision,
                S::Superseded,
            ) => E::PlanSuperseded,
            _ => {
                return Err(TransitionError::Invalid {
                    state: current.as_str().into(),
                    event: format!("target={}", target.as_str()),
                    reason: "当前节点不允许该目标状态",
                })
            }
        };
        Self::transition(current, &event, &())
    }
}

impl StateMachine for DagNodeMachine {
    type State = DeepNoteNodeStatus;
    type Event = DagNodeEvent;
    type Context = ();
    type Effect = DagNodeEffect;

    fn transition(
        state: DeepNoteNodeStatus,
        event: &DagNodeEvent,
        _: &(),
    ) -> Result<Transition<DeepNoteNodeStatus, DagNodeEffect>, TransitionError> {
        use DagNodeEvent as E;
        use DeepNoteNodeStatus as S;
        let invalid = || {
            Err(TransitionError::Invalid {
                state: state.as_str().into(),
                event: format!("{event:?}"),
                reason: "当前节点不允许该事件",
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
            (S::Pending, E::DependenciesSatisfied) => ok(S::Ready, vec![], "依赖已满足"),
            (S::Pending, E::DependencyFailed) => ok(S::Blocked, vec![], "依赖失败"),
            (S::Ready, E::ExecutionStarted) => ok(
                S::InProgress,
                vec![DagNodeEffect::StartWorker],
                "开始执行节点",
            ),
            (S::InProgress, E::ExecutionSucceeded) => {
                ok(S::NeedsReview, vec![], "执行完成等待验证")
            }
            (S::NeedsReview, E::ValidationPassed) => ok(S::Completed, vec![], "验证通过"),
            (S::NeedsReview, E::ValidationFailed) => ok(S::NeedsRevision, vec![], "需要修订"),
            (S::NeedsRevision, E::RevisionScheduled) => ok(S::Ready, vec![], "重新排队修订"),
            (S::InProgress, E::RetryScheduled) | (S::NeedsRevision, E::RetryScheduled) => {
                ok(S::Ready, vec![], "节点重试")
            }
            (S::InProgress | S::NeedsReview | S::NeedsRevision, E::AttemptLimitReached) => {
                ok(S::Failed, vec![], "达到尝试上限")
            }
            (S::InProgress, E::ExecutionInterrupted) => ok(S::Interrupted, vec![], "执行被中断"),
            (
                S::Pending | S::Ready | S::NeedsReview | S::NeedsRevision | S::Interrupted,
                E::PlanSuperseded,
            ) => ok(
                S::Superseded,
                vec![DagNodeEffect::MarkSuperseded],
                "计划版本已替代",
            ),
            _ => invalid(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DagNodeEvent, DagNodeMachine};
    use crate::{chat::note_pipeline::types::DeepNoteNodeStatus, task_runtime::StateMachine};

    #[test]
    fn node_must_be_validated_before_completion() {
        let review = DagNodeMachine::transition(
            DeepNoteNodeStatus::InProgress,
            &DagNodeEvent::ExecutionSucceeded,
            &(),
        )
        .unwrap();
        assert_eq!(review.next_state, DeepNoteNodeStatus::NeedsReview);
        let complete =
            DagNodeMachine::transition(review.next_state, &DagNodeEvent::ValidationPassed, &())
                .unwrap();
        assert_eq!(complete.next_state, DeepNoteNodeStatus::Completed);
    }

    #[test]
    fn interrupted_execution_and_compatibility_targets_match() {
        let interrupted = DagNodeMachine::transition(
            DeepNoteNodeStatus::InProgress,
            &DagNodeEvent::ExecutionInterrupted,
            &(),
        )
        .unwrap();
        assert_eq!(interrupted.next_state, DeepNoteNodeStatus::Interrupted);
        let skipped = DagNodeMachine::transition_to(
            DeepNoteNodeStatus::NeedsReview,
            DeepNoteNodeStatus::Skipped,
        )
        .unwrap();
        assert_eq!(skipped.next_state, DeepNoteNodeStatus::Skipped);
    }

    #[test]
    fn checkpoint_is_required_for_ready_to_completed_reconciliation() {
        assert!(DagNodeMachine::transition_to(
            DeepNoteNodeStatus::Ready,
            DeepNoteNodeStatus::Completed,
        )
        .is_err());
        let completed = DagNodeMachine::transition_to_with_checkpoint(
            DeepNoteNodeStatus::Ready,
            DeepNoteNodeStatus::Completed,
            true,
        )
        .unwrap();
        assert_eq!(completed.next_state, DeepNoteNodeStatus::Completed);
        assert_eq!(completed.reason, "根据已有产物检查点完成节点");
    }

    #[test]
    fn recovery_and_shutdown_transitions_match_scheduler_behavior() {
        let recovered = DagNodeMachine::transition_to(
            DeepNoteNodeStatus::InProgress,
            DeepNoteNodeStatus::Pending,
        )
        .unwrap();
        assert_eq!(recovered.next_state, DeepNoteNodeStatus::Pending);
        let skipped =
            DagNodeMachine::transition_to(DeepNoteNodeStatus::Blocked, DeepNoteNodeStatus::Skipped)
                .unwrap();
        assert_eq!(skipped.next_state, DeepNoteNodeStatus::Skipped);
    }
}
