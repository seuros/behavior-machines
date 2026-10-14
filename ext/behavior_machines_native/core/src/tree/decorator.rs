//! Single-child decorators and timed nodes.

use core::time::Duration;

use super::slot::{Run, Slot};
use crate::observe::Observer;
use crate::{Leaf, Status, Time};

/// A status rewrite applied to the child's result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Map {
    Inverter,
    /// `ForceSuccess` and `ForceFailure`.
    Force(Status),
}

impl Map {
    pub(super) const fn apply(self, status: Status) -> Status {
        match (self, status) {
            (_, Status::Running) => Status::Running,
            (Self::Inverter, Status::Success) => Status::Failure,
            (Self::Inverter, Status::Failure) => Status::Success,
            (Self::Force(forced), _) => forced,
        }
    }
}

/// The deadline is fixed when the node starts. The child only ticks strictly
/// before it; at or after it the child is halted and the timeout fails.
pub(super) fn timeout<L, C, O>(
    child: &mut Slot<L>,
    limit: Duration,
    deadline: &mut Option<Time>,
    starting: bool,
    run: &mut Run<'_, C, O>,
    now: Time,
) -> Status
where
    L: Leaf<C>,
    C: ?Sized,
    O: Observer + ?Sized,
{
    if starting {
        *deadline = now.checked_add(limit);
    }
    if reached(*deadline, now) {
        child.halt(run);
        return Status::Failure;
    }
    child.tick(run, now)
}

pub(super) fn sleep(
    duration: Duration,
    deadline: &mut Option<Time>,
    starting: bool,
    now: Time,
) -> Status {
    if starting {
        *deadline = now.checked_add(duration);
    }
    if reached(*deadline, now) {
        Status::Success
    } else {
        Status::Running
    }
}

/// `None` is a deadline past the end of the clock: it never fires.
fn reached(deadline: Option<Time>, now: Time) -> bool {
    deadline.is_some_and(|deadline| now >= deadline)
}

/// `Repeat`, `RetryUntilSuccessful`, and `KeepRunningUntilFailure`. A child
/// run that ends with `again_on` counts toward `limit`; reaching it finishes
/// with that status, otherwise the next run starts on the next tick, which
/// `due` marks as already due. The other terminal status finishes immediately.
pub(super) fn looped<L, C, O>(
    child: &mut Slot<L>,
    again_on: Status,
    limit: Option<usize>,
    runs: &mut usize,
    due: &mut Option<Time>,
    run: &mut Run<'_, C, O>,
    now: Time,
) -> Status
where
    L: Leaf<C>,
    C: ?Sized,
    O: Observer + ?Sized,
{
    *due = None;
    if limit == Some(0) {
        return again_on;
    }
    match child.tick(run, now) {
        Status::Running => Status::Running,
        // Unlimited loops never finish this way, so they do not count runs.
        status if status == again_on => match limit {
            Some(limit) if *runs + 1 == limit => {
                *runs = 0;
                status
            }
            Some(_) => {
                *runs += 1;
                again(due, now)
            }
            None => again(due, now),
        },
        decisive => {
            *runs = 0;
            decisive
        }
    }
}

/// A run finished and the next waits for a tick: due at once.
const fn again(due: &mut Option<Time>, now: Time) -> Status {
    *due = Some(now);
    Status::Running
}

#[cfg(test)]
mod tests;
