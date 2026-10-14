//! Composites over a list of children.

use super::slot::{Run, Slot};
use crate::observe::Observer;
use crate::{Leaf, Status, Time};

/// Sequence and fallback with memory. The cursor survives only while a child
/// runs, so a finished composite restarts from its first child.
pub(super) fn ordered<L, C, O>(
    children: &mut [Slot<L>],
    cursor: &mut usize,
    advance_on: Status,
    run: &mut Run<'_, C, O>,
    now: Time,
) -> Status
where
    L: Leaf<C>,
    C: ?Sized,
    O: Observer + ?Sized,
{
    while let Some(child) = children.get_mut(*cursor) {
        match child.tick(run, now) {
            Status::Running => return Status::Running,
            status if status == advance_on => *cursor += 1,
            decisive => {
                *cursor = 0;
                return decisive;
            }
        }
    }
    *cursor = 0;
    advance_on
}

/// Reactive sequence and fallback: re-walk from the first child. The child
/// that stops the walk keeps running or decides the result, and every later
/// child that was still running is halted.
pub(super) fn reactive<L, C, O>(
    children: &mut [Slot<L>],
    advance_on: Status,
    run: &mut Run<'_, C, O>,
    now: Time,
) -> Status
where
    L: Leaf<C>,
    C: ?Sized,
    O: Observer + ?Sized,
{
    let mut rest = children;
    while let Some((child, later)) = rest.split_first_mut() {
        let status = child.tick(run, now);
        if status != advance_on {
            halt_all(later, run);
            return status;
        }
        rest = later;
    }
    advance_on
}

/// Decides once `success` children succeeded, or once `failure` failed or
/// success became unreachable. Finished children are not ticked again within
/// an activation, and children decide in order, so the leftmost wins ties.
pub(super) fn parallel<L, C, O>(
    children: &mut [Slot<L>],
    outcomes: &mut [Option<Status>],
    success: usize,
    failure: usize,
    run: &mut Run<'_, C, O>,
    now: Time,
) -> Status
where
    L: Leaf<C>,
    C: ?Sized,
    O: Observer + ?Sized,
{
    let total = children.len();
    let count = |outcomes: &[Option<Status>], status| {
        outcomes
            .iter()
            .filter(|outcome| **outcome == Some(status))
            .count()
    };
    let (mut succeeded, mut failed) = (
        count(outcomes, Status::Success),
        count(outcomes, Status::Failure),
    );

    let mut decided = None;
    for (child, outcome) in children.iter_mut().zip(outcomes.iter_mut()) {
        if outcome.is_some() {
            continue;
        }
        let status = child.tick(run, now);
        match status {
            Status::Running => continue,
            Status::Success => succeeded += 1,
            Status::Failure => failed += 1,
        }
        *outcome = Some(status);
        if succeeded >= success {
            decided = Some(Status::Success);
        } else if failed >= failure || total - failed < success {
            decided = Some(Status::Failure);
        }
        if decided.is_some() {
            break;
        }
    }

    let Some(status) = decided else {
        return Status::Running;
    };
    halt_all(children, run);
    outcomes.fill(None);
    status
}

/// Halts every running child. Finished and never-started children are not
/// running, so they are skipped.
pub(super) fn halt_all<L, C, O>(children: &mut [Slot<L>], run: &mut Run<'_, C, O>)
where
    L: Leaf<C>,
    C: ?Sized,
    O: Observer + ?Sized,
{
    for child in children {
        child.halt(run);
    }
}

#[cfg(test)]
mod tests;
