use crate::{controller::GameCommand, discord::UserID};

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionKind {
    Vote,
    Retract,
    Reveal,
    Target,
    Mark,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action<U: RawPID> {
    Vote { voter: U, ballot: Option<Choice<U>> },
    Reveal { celeb: U },
    Target { actor: U, target: Choice<U> },
    Mark { killer: U, mark: Choice<U> },
}
impl<U: RawPID> Action<U> {
    pub fn kind(&self) -> ActionKind {
        match self {
            Action::Vote { .. } => ActionKind::Vote,
            Action::Reveal { .. } => ActionKind::Reveal,
            Action::Target { .. } => ActionKind::Target,
            Action::Mark { .. } => ActionKind::Mark,
        }
    }
}
