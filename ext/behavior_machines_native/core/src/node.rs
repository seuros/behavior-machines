use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt;
use core::time::Duration;

/// A declarative tree definition. Compile it with [`crate::Tree::new`].
///
/// Node names and semantics follow the behavior-tree pattern. Build nodes
/// with the variants directly, with the constructors, or with the decorator methods ([`Node::inverter`],
/// [`Node::timeout`], [`Node::repeat`], ...).
///
/// Decorators that loop (`Repeat`, `RetryUntilSuccessful`,
/// `KeepRunningUntilFailure`) finish at most one run of their child per tick,
/// so a child that finishes instantly cannot spin forever inside one tick.
/// Between runs they are due at once by [`Tree::next_deadline`](crate::Tree::next_deadline).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Node<L> {
    /// A condition check or action.
    Leaf(L),
    /// Succeeds immediately.
    AlwaysSuccess,
    /// Fails immediately.
    AlwaysFailure,
    /// Runs until the duration has passed since it started, then succeeds.
    /// A zero duration succeeds immediately.
    Sleep(Duration),
    /// Ticks children in order. Fails as soon as one fails and succeeds once
    /// all succeed. A running child is resumed on the next tick.
    Sequence(Vec<Self>),
    /// A [`Node::Sequence`] that restarts from its first child on every tick,
    /// so earlier children act as guards: once one stops succeeding, the
    /// running later child is halted.
    ReactiveSequence(Vec<Self>),
    /// Ticks children in order. Succeeds as soon as one succeeds and fails
    /// once all fail. A running child is resumed on the next tick.
    Fallback(Vec<Self>),
    /// A [`Node::Fallback`] that restarts from its first child on every tick,
    /// so a higher-priority child that starts succeeding or running preempts
    /// the running lower-priority child, which is halted.
    ReactiveFallback(Vec<Self>),
    /// Ticks every unfinished child, in order, on each tick. Succeeds once
    /// `success` children have succeeded; fails once `failure` children have
    /// failed or success can no longer be reached. Running children are
    /// halted when it decides, and the leftmost child decides ties.
    Parallel {
        /// The children, all running concurrently.
        children: Vec<Self>,
        /// How many children must succeed.
        success: Threshold,
        /// How many children may fail before the node fails.
        failure: Threshold,
    },
    /// Ticks `condition` until it finishes, then runs `then` if it succeeded
    /// or `otherwise` if it failed, and reports that branch's status. Without
    /// `otherwise`, a failed condition fails the node. The condition is not
    /// ticked again while the branch runs.
    IfThenElse {
        /// Decides the branch, once.
        condition: Box<Self>,
        /// Runs after the condition succeeds.
        then: Box<Self>,
        /// Runs after the condition fails.
        otherwise: Option<Box<Self>>,
    },
    /// The reactive [`Node::IfThenElse`]: ticks `condition` on every tick and
    /// runs `body` while it succeeds or `otherwise` while it fails, halting
    /// the other branch first when the condition flips. While the condition
    /// runs, the branches are paused: not ticked and not halted. Without
    /// `otherwise`, a failed condition fails the node.
    WhileDoElse {
        /// Re-evaluated on every tick.
        condition: Box<Self>,
        /// Runs while the condition succeeds.
        body: Box<Self>,
        /// Runs while the condition fails.
        otherwise: Option<Box<Self>>,
    },
    /// Swaps the child's [`Success`](crate::Status::Success) and
    /// [`Failure`](crate::Status::Failure).
    Inverter(Box<Self>),
    /// Reports any finished child as [`Success`](crate::Status::Success).
    ForceSuccess(Box<Self>),
    /// Reports any finished child as [`Failure`](crate::Status::Failure).
    ForceFailure(Box<Self>),
    /// Fails, halting the child, once the duration has passed since this node
    /// started. The child only ticks strictly before the deadline, so a zero
    /// duration fails without ticking it. To run without a limit, leave the
    /// decorator out.
    Timeout(Duration, Box<Self>),
    /// Runs the child again each time it succeeds, until it has succeeded
    /// `limit` times, then succeeds. Fails as soon as the child fails.
    Repeat(Limit, Box<Self>),
    /// Runs the child again each time it fails, until it has failed `limit`
    /// times, then fails. Succeeds as soon as the child succeeds.
    RetryUntilSuccessful(Limit, Box<Self>),
    /// Runs the child again each time it succeeds, forever. Fails as soon as
    /// the child fails.
    KeepRunningUntilFailure(Box<Self>),
}

/// How many children of a [`Node::Parallel`] a decision needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Threshold {
    /// Every child.
    All,
    /// This many children, between 1 and the number of children. Zero is
    /// rejected when the tree is built, since it would decide before any child
    /// finishes.
    Count(usize),
}

/// How many runs a looping decorator allows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Limit {
    /// This many runs. Zero finishes immediately without ticking the child.
    Times(usize),
    /// No limit.
    Unlimited,
}

impl<L> Node<L> {
    /// Creates a [`Node::Leaf`].
    pub const fn leaf(leaf: L) -> Self {
        Self::Leaf(leaf)
    }

    /// Creates a [`Node::Sleep`].
    #[must_use]
    pub const fn sleep(duration: Duration) -> Self {
        Self::Sleep(duration)
    }

    /// Creates a [`Node::Sequence`].
    pub fn sequence(children: impl IntoIterator<Item = Self>) -> Self {
        Self::Sequence(children.into_iter().collect())
    }

    /// Creates a [`Node::ReactiveSequence`].
    pub fn reactive_sequence(children: impl IntoIterator<Item = Self>) -> Self {
        Self::ReactiveSequence(children.into_iter().collect())
    }

    /// Creates a [`Node::Fallback`].
    pub fn fallback(children: impl IntoIterator<Item = Self>) -> Self {
        Self::Fallback(children.into_iter().collect())
    }

    /// Creates a [`Node::ReactiveFallback`].
    pub fn reactive_fallback(children: impl IntoIterator<Item = Self>) -> Self {
        Self::ReactiveFallback(children.into_iter().collect())
    }

    /// Creates a [`Node::Parallel`].
    pub fn parallel(
        children: impl IntoIterator<Item = Self>,
        success: Threshold,
        failure: Threshold,
    ) -> Self {
        Self::Parallel {
            children: children.into_iter().collect(),
            success,
            failure,
        }
    }

    /// Creates a [`Node::IfThenElse`].
    pub fn if_then_else(condition: Self, then: Self, otherwise: Self) -> Self {
        Self::IfThenElse {
            condition: Box::new(condition),
            then: Box::new(then),
            otherwise: Some(Box::new(otherwise)),
        }
    }

    /// Creates a [`Node::IfThenElse`] without an `otherwise` branch.
    pub fn if_then(condition: Self, then: Self) -> Self {
        Self::IfThenElse {
            condition: Box::new(condition),
            then: Box::new(then),
            otherwise: None,
        }
    }

    /// Creates a [`Node::WhileDoElse`].
    pub fn while_do_else(condition: Self, body: Self, otherwise: Self) -> Self {
        Self::WhileDoElse {
            condition: Box::new(condition),
            body: Box::new(body),
            otherwise: Some(Box::new(otherwise)),
        }
    }

    /// Creates a [`Node::WhileDoElse`] without an `otherwise` branch.
    pub fn while_do(condition: Self, body: Self) -> Self {
        Self::WhileDoElse {
            condition: Box::new(condition),
            body: Box::new(body),
            otherwise: None,
        }
    }

    /// Wraps this node in a [`Node::Inverter`].
    #[must_use]
    pub fn inverter(self) -> Self {
        Self::Inverter(Box::new(self))
    }

    /// Wraps this node in a [`Node::ForceSuccess`].
    #[must_use]
    pub fn force_success(self) -> Self {
        Self::ForceSuccess(Box::new(self))
    }

    /// Wraps this node in a [`Node::ForceFailure`].
    #[must_use]
    pub fn force_failure(self) -> Self {
        Self::ForceFailure(Box::new(self))
    }

    /// Wraps this node in a [`Node::Timeout`] of `limit`.
    #[must_use]
    pub fn timeout(self, limit: Duration) -> Self {
        Self::Timeout(limit, Box::new(self))
    }

    /// Wraps this node in a [`Node::Repeat`].
    #[must_use]
    pub fn repeat(self, limit: Limit) -> Self {
        Self::Repeat(limit, Box::new(self))
    }

    /// Wraps this node in a [`Node::RetryUntilSuccessful`].
    #[must_use]
    pub fn retry_until_successful(self, limit: Limit) -> Self {
        Self::RetryUntilSuccessful(limit, Box::new(self))
    }

    /// Wraps this node in a [`Node::KeepRunningUntilFailure`].
    #[must_use]
    pub fn keep_running_until_failure(self) -> Self {
        Self::KeepRunningUntilFailure(Box::new(self))
    }

    /// The same definition with every leaf passed through `f`, in the
    /// pre-order of [`NodeId`](crate::NodeId)s, so a host can number or count
    /// its leaves.
    pub fn map<M>(self, mut f: impl FnMut(L) -> M) -> Node<M> {
        self.map_with(&mut f)
    }

    fn map_with<M, F: FnMut(L) -> M>(self, f: &mut F) -> Node<M> {
        let mut boxed = |node: Box<Self>| Box::new(node.map_with(f));
        match self {
            Self::Leaf(leaf) => Node::Leaf(f(leaf)),
            Self::AlwaysSuccess => Node::AlwaysSuccess,
            Self::AlwaysFailure => Node::AlwaysFailure,
            Self::Sleep(duration) => Node::Sleep(duration),
            Self::Sequence(children) => Node::Sequence(map_all(children, f)),
            Self::ReactiveSequence(children) => Node::ReactiveSequence(map_all(children, f)),
            Self::Fallback(children) => Node::Fallback(map_all(children, f)),
            Self::ReactiveFallback(children) => Node::ReactiveFallback(map_all(children, f)),
            Self::Parallel {
                children,
                success,
                failure,
            } => Node::Parallel {
                children: map_all(children, f),
                success,
                failure,
            },
            Self::IfThenElse {
                condition,
                then,
                otherwise,
            } => {
                let condition = boxed(condition);
                let then = boxed(then);
                Node::IfThenElse {
                    condition,
                    then,
                    otherwise: otherwise.map(boxed),
                }
            }
            Self::WhileDoElse {
                condition,
                body,
                otherwise,
            } => {
                let condition = boxed(condition);
                let body = boxed(body);
                Node::WhileDoElse {
                    condition,
                    body,
                    otherwise: otherwise.map(boxed),
                }
            }
            Self::Inverter(child) => Node::Inverter(boxed(child)),
            Self::ForceSuccess(child) => Node::ForceSuccess(boxed(child)),
            Self::ForceFailure(child) => Node::ForceFailure(boxed(child)),
            Self::Timeout(limit, child) => Node::Timeout(limit, boxed(child)),
            Self::Repeat(limit, child) => Node::Repeat(limit, boxed(child)),
            Self::RetryUntilSuccessful(limit, child) => {
                Node::RetryUntilSuccessful(limit, boxed(child))
            }
            Self::KeepRunningUntilFailure(child) => Node::KeepRunningUntilFailure(boxed(child)),
        }
    }
}

fn map_all<L, M, F: FnMut(L) -> M>(children: Vec<Node<L>>, f: &mut F) -> Vec<Node<M>> {
    children
        .into_iter()
        .map(|child| child.map_with(f))
        .collect()
}

/// The nodes that need at least one child, as reported by
/// [`crate::BuildError`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Composite {
    /// [`Node::Sequence`].
    Sequence,
    /// [`Node::ReactiveSequence`].
    ReactiveSequence,
    /// [`Node::Fallback`].
    Fallback,
    /// [`Node::ReactiveFallback`].
    ReactiveFallback,
    /// [`Node::Parallel`].
    Parallel,
}

impl fmt::Display for Composite {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Sequence => "Sequence",
            Self::ReactiveSequence => "ReactiveSequence",
            Self::Fallback => "Fallback",
            Self::ReactiveFallback => "ReactiveFallback",
            Self::Parallel => "Parallel",
        })
    }
}

#[cfg(test)]
mod tests;
