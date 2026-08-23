//! Shared, persistence-agnostic state-machine primitives.
//!
//! Domain machines live next to their domain code. This module deliberately
//! contains no database or worker code: a transition is a pure decision that
//! can be checked before a CAS transaction performs any side effects.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionError {
    Invalid {
        state: String,
        event: String,
        reason: &'static str,
    },
    Stale {
        expected_version: u32,
        actual_version: u32,
    },
    Terminal {
        state: String,
    },
}

impl fmt::Display for TransitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid {
                state,
                event,
                reason,
            } => {
                write!(f, "非法状态转换：{state} + {event}：{reason}")
            }
            Self::Stale {
                expected_version,
                actual_version,
            } => write!(
                f,
                "状态版本已过期：期望 {expected_version}，实际 {actual_version}"
            ),
            Self::Terminal { state } => write!(f, "终态不可继续推进：{state}"),
        }
    }
}

impl std::error::Error for TransitionError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition<S, E> {
    pub next_state: S,
    pub effects: Vec<E>,
    pub reason: &'static str,
}

pub trait StateMachine {
    type State: Copy + Eq;
    type Event;
    type Context;
    type Effect;

    fn transition(
        state: Self::State,
        event: &Self::Event,
        context: &Self::Context,
    ) -> Result<Transition<Self::State, Self::Effect>, TransitionError>;
}

#[cfg(test)]
mod tests {
    use super::{StateMachine, Transition, TransitionError};

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum State {
        Idle,
        Done,
    }
    enum Event {
        Finish,
    }
    struct Machine;

    impl StateMachine for Machine {
        type State = State;
        type Event = Event;
        type Context = ();
        type Effect = ();

        fn transition(
            state: State,
            event: &Event,
            _: &(),
        ) -> Result<Transition<State, ()>, TransitionError> {
            match (state, event) {
                (State::Idle, Event::Finish) => Ok(Transition {
                    next_state: State::Done,
                    effects: vec![],
                    reason: "finished",
                }),
                (State::Done, Event::Finish) => Err(TransitionError::Terminal {
                    state: "done".into(),
                }),
            }
        }
    }

    #[test]
    fn transition_is_pure_and_rejects_terminal_state() {
        assert_eq!(
            Machine::transition(State::Idle, &Event::Finish, &())
                .unwrap()
                .next_state,
            State::Done
        );
        assert!(matches!(
            Machine::transition(State::Done, &Event::Finish, &()),
            Err(TransitionError::Terminal { .. })
        ));
    }
}
