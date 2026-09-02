//! Chat Agent 与 Tool Call 的持久化无关状态合同。
//!
//! 当前 Chat 的历史消息仍是兼容快照；审批和取消路径可以先使用这组纯转换，
//! 后续再把状态版本落到 agent_runs/agent_tool_calls 表中。

use crate::task_runtime::{StateMachine, Transition, TransitionError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRunState {
    Created,
    Running,
    Waiting,
    Stopping,
    Completed,
    Stopped,
    Failed,
    BudgetExhausted,
}

impl AgentRunState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::Stopping => "stopping",
            Self::Completed => "completed",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
            Self::BudgetExhausted => "budgetExhausted",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "created" => Ok(Self::Created),
            "running" => Ok(Self::Running),
            "waiting" => Ok(Self::Waiting),
            "stopping" => Ok(Self::Stopping),
            "completed" => Ok(Self::Completed),
            "stopped" => Ok(Self::Stopped),
            "failed" => Ok(Self::Failed),
            "budgetExhausted" => Ok(Self::BudgetExhausted),
            _ => Err(format!("未知 Agent Run 状态：{value}")),
        }
    }
}

/// Agent 运行的完整事件集。
///
/// `#[allow(dead_code)]`：部分事件（如 `UserInputRequired`、`BudgetExceeded`）目前
/// 没有生产构造点，但它们是 `transition` 的 match 臂与单测断言的一部分。删掉
/// variant 就要同时删掉转移规则，等于把状态机的合法转移表改小 —— 那是功能变更，
/// 不是清理。事件集在这里保持完整，是为了让「哪些转移是合法的」一眼可读。
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRunEvent {
    StartRequested,
    ModelCallStarted,
    ApprovalRequired,
    ApprovalsResolved,
    ToolBatchStarted,
    ToolBatchCompleted,
    UserInputRequired,
    FinalizationStarted,
    FinalizationCompleted,
    CancelRequested,
    WorkerStopped,
    BudgetExceeded,
    PanicDetected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRunEffect {
    StartWorker,
    StopWorker,
    ClosePendingApprovals,
}

pub struct AgentRunMachine;

impl StateMachine for AgentRunMachine {
    type State = AgentRunState;
    type Event = AgentRunEvent;
    type Context = ();
    type Effect = AgentRunEffect;

    fn transition(
        state: AgentRunState,
        event: &AgentRunEvent,
        _: &(),
    ) -> Result<Transition<AgentRunState, AgentRunEffect>, TransitionError> {
        use AgentRunEffect as F;
        use AgentRunEvent as E;
        use AgentRunState as S;
        let invalid = || {
            Err(TransitionError::Invalid {
                state: format!("{state:?}"),
                event: format!("{event:?}"),
                reason: "当前 Agent 运行不允许该事件",
            })
        };
        let terminal = || {
            Err(TransitionError::Terminal {
                state: format!("{state:?}"),
            })
        };
        match (state, event) {
            (S::Created, E::StartRequested) => Ok(Transition {
                next_state: S::Running,
                effects: vec![F::StartWorker],
                reason: "启动 Agent",
            }),
            (S::Running, E::ModelCallStarted) => Ok(Transition {
                next_state: S::Running,
                effects: vec![],
                reason: "开始模型调用",
            }),
            (S::Running, E::ApprovalRequired) => Ok(Transition {
                next_state: S::Waiting,
                effects: vec![],
                reason: "等待工具审批",
            }),
            (S::Waiting, E::ApprovalsResolved) => Ok(Transition {
                next_state: S::Running,
                effects: vec![F::StartWorker],
                reason: "审批已解决",
            }),
            (S::Running, E::ToolBatchStarted) => Ok(Transition {
                next_state: S::Running,
                effects: vec![],
                reason: "开始工具批次",
            }),
            (S::Running, E::ToolBatchCompleted) => Ok(Transition {
                next_state: S::Running,
                effects: vec![],
                reason: "工具批次完成",
            }),
            (S::Running, E::UserInputRequired) => Ok(Transition {
                next_state: S::Waiting,
                effects: vec![],
                reason: "等待用户输入",
            }),
            (S::Running, E::FinalizationStarted) => Ok(Transition {
                next_state: S::Running,
                effects: vec![],
                reason: "开始整理最终回答",
            }),
            (S::Running, E::FinalizationCompleted) => Ok(Transition {
                next_state: S::Completed,
                effects: vec![],
                reason: "Agent 回复完成",
            }),
            (S::Running | S::Waiting, E::CancelRequested) => Ok(Transition {
                next_state: S::Stopping,
                effects: vec![F::StopWorker, F::ClosePendingApprovals],
                reason: "请求停止 Agent",
            }),
            (S::Stopping, E::WorkerStopped) => Ok(Transition {
                next_state: S::Stopped,
                effects: vec![],
                reason: "Agent Worker 已停止",
            }),
            (S::Running, E::BudgetExceeded) => Ok(Transition {
                next_state: S::BudgetExhausted,
                effects: vec![],
                reason: "Agent 预算耗尽",
            }),
            (S::Created | S::Running | S::Waiting | S::Stopping, E::PanicDetected) => {
                Ok(Transition {
                    next_state: S::Failed,
                    effects: vec![F::ClosePendingApprovals],
                    reason: "Agent 异常终止",
                })
            }
            (S::Completed | S::Stopped | S::Failed | S::BudgetExhausted, _) => terminal(),
            _ => invalid(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallState {
    Proposed,
    AwaitingApproval,
    Approved,
    /// 用户回答了提问工具。刻意不复用 `Approved`：审计上「批准一次删库」和
    /// 「在两个方案里选了 B」不是同一件事，混成一个状态就查不出区别了。
    Answered,
    Queued,
    Running,
    Completed,
    Rejected,
    Failed,
    Cancelled,
    TimedOut,
}

impl ToolCallState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::AwaitingApproval => "awaitingApproval",
            Self::Approved => "approved",
            Self::Answered => "answered",
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Rejected => "rejected",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timedOut",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "proposed" => Ok(Self::Proposed),
            "awaitingApproval" => Ok(Self::AwaitingApproval),
            "approved" => Ok(Self::Approved),
            "answered" => Ok(Self::Answered),
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "rejected" => Ok(Self::Rejected),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "timedOut" => Ok(Self::TimedOut),
            _ => Err(format!("未知 Tool Call 状态：{value}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallEvent {
    ApprovalRequired,
    Approved,
    Answered,
    Rejected,
    Enqueued,
    Started,
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
}

pub struct ToolCallMachine;

impl StateMachine for ToolCallMachine {
    type State = ToolCallState;
    type Event = ToolCallEvent;
    type Context = ();
    type Effect = ();

    fn transition(
        state: ToolCallState,
        event: &ToolCallEvent,
        _: &(),
    ) -> Result<Transition<ToolCallState, ()>, TransitionError> {
        use ToolCallEvent as E;
        use ToolCallState as S;
        let next = match (state, event) {
            (S::Proposed, E::ApprovalRequired) => S::AwaitingApproval,
            (S::Proposed, E::Enqueued) => S::Queued,
            (S::AwaitingApproval, E::Approved) => S::Approved,
            (S::AwaitingApproval, E::Answered) => S::Answered,
            (S::AwaitingApproval, E::Rejected) => S::Rejected,
            (S::AwaitingApproval, E::TimedOut) => S::TimedOut,
            (S::Approved, E::Enqueued) => S::Queued,
            // 提问工具的答案就是它的产出，回答完直接进队列执行（handler 只做回显）。
            (S::Answered, E::Enqueued) => S::Queued,
            (S::Queued, E::Started) => S::Running,
            (S::Running, E::Succeeded) => S::Completed,
            (S::Running, E::Failed) => S::Failed,
            (
                S::Proposed
                | S::AwaitingApproval
                | S::Approved
                | S::Answered
                | S::Queued
                | S::Running,
                E::Cancelled,
            ) => S::Cancelled,
            _ => {
                return Err(TransitionError::Invalid {
                    state: format!("{state:?}"),
                    event: format!("{event:?}"),
                    reason: "当前 Tool Call 不允许该事件",
                })
            }
        };
        Ok(Transition {
            next_state: next,
            effects: vec![],
            reason: "Tool Call 状态转换",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answering_a_question_is_distinct_from_approving_a_tool() {
        // 回答走自己的状态，不能悄悄落到 Approved 上，否则审计查不出区别。
        let answered = ToolCallMachine::transition(
            ToolCallState::AwaitingApproval,
            &ToolCallEvent::Answered,
            &(),
        )
        .unwrap();
        assert_eq!(answered.next_state, ToolCallState::Answered);
        assert_eq!(ToolCallState::Answered.as_str(), "answered");
        assert_eq!(
            ToolCallState::parse("answered").unwrap(),
            ToolCallState::Answered
        );
        // 回答完要能继续执行，否则 handler 永远拿不到答案。
        assert_eq!(
            ToolCallMachine::transition(answered.next_state, &ToolCallEvent::Enqueued, &())
                .unwrap()
                .next_state,
            ToolCallState::Queued
        );
        // 回答后取消仍然合法：用户可以答完又中止整个 run。
        assert_eq!(
            ToolCallMachine::transition(ToolCallState::Answered, &ToolCallEvent::Cancelled, &())
                .unwrap()
                .next_state,
            ToolCallState::Cancelled
        );
        // 没在等待用户时收到回答是非法的，别让乱序事件把状态带歪。
        assert!(
            ToolCallMachine::transition(ToolCallState::Running, &ToolCallEvent::Answered, &())
                .is_err()
        );
    }

    #[test]
    fn approval_and_cancel_race_is_safe() {
        let waiting = AgentRunMachine::transition(
            AgentRunState::Running,
            &AgentRunEvent::ApprovalRequired,
            &(),
        )
        .unwrap();
        assert_eq!(waiting.next_state, AgentRunState::Waiting);
        let stopping =
            AgentRunMachine::transition(waiting.next_state, &AgentRunEvent::CancelRequested, &())
                .unwrap();
        assert_eq!(stopping.next_state, AgentRunState::Stopping);
        assert!(ToolCallMachine::transition(
            ToolCallState::AwaitingApproval,
            &ToolCallEvent::Approved,
            &(),
        )
        .is_ok());
        assert_eq!(
            ToolCallMachine::transition(
                ToolCallState::AwaitingApproval,
                &ToolCallEvent::Cancelled,
                &(),
            )
            .unwrap()
            .next_state,
            ToolCallState::Cancelled
        );
    }
}
