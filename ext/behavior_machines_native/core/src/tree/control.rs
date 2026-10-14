//! Conditional control nodes.

use super::slot::{Run, Slot};
use crate::observe::Observer;
use crate::{Leaf, Status, Time};

/// Which part of an `IfThenElse` runs next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Branch {
    Condition,
    Then,
    Otherwise,
}

pub(super) fn if_then_else<L, C, O>(
    condition: &mut Slot<L>,
    then: &mut Slot<L>,
    otherwise: Option<&mut Slot<L>>,
    branch: &mut Branch,
    run: &mut Run<'_, C, O>,
    now: Time,
) -> Status
where
    L: Leaf<C>,
    C: ?Sized,
    O: Observer + ?Sized,
{
    if *branch == Branch::Condition {
        *branch = match condition.tick(run, now) {
            Status::Running => return Status::Running,
            Status::Success => Branch::Then,
            Status::Failure => Branch::Otherwise,
        };
    }
    let chosen = match (*branch, otherwise) {
        (Branch::Otherwise, Some(otherwise)) => otherwise,
        (Branch::Otherwise, None) => {
            *branch = Branch::Condition;
            return Status::Failure;
        }
        _ => then,
    };
    let status = chosen.tick(run, now);
    if status != Status::Running {
        *branch = Branch::Condition;
    }
    status
}

/// The condition is ticked every tick. While it runs, the branches are
/// paused: neither ticks nor is halted. Otherwise the branch it does not
/// select is halted before the selected one ticks.
pub(super) fn while_do_else<L, C, O>(
    condition: &mut Slot<L>,
    body: &mut Slot<L>,
    otherwise: Option<&mut Slot<L>>,
    run: &mut Run<'_, C, O>,
    now: Time,
) -> Status
where
    L: Leaf<C>,
    C: ?Sized,
    O: Observer + ?Sized,
{
    match (condition.tick(run, now), otherwise) {
        (Status::Running, _) => Status::Running,
        (Status::Success, otherwise) => {
            if let Some(otherwise) = otherwise {
                otherwise.halt(run);
            }
            body.tick(run, now)
        }
        (Status::Failure, Some(otherwise)) => {
            body.halt(run);
            otherwise.tick(run, now)
        }
        (Status::Failure, None) => {
            body.halt(run);
            Status::Failure
        }
    }
}

#[cfg(test)]
mod tests;
