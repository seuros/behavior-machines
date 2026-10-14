use crate::observe::Observer;
use crate::{Composite, Leaf, Node, Status, Time};

mod composite;
mod control;
mod decorator;
mod slot;

use slot::{Run, Slot};

/// The reason a [`Node`] definition cannot be compiled into a [`Tree`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum BuildError {
    /// A composite has no children, so its result would be arbitrary.
    #[error("{0} needs at least one child")]
    EmptyComposite(Composite),
    /// A [`Node::Parallel`] threshold is zero or larger than its child count.
    #[error("Parallel threshold {threshold} must be between 1 and {children}")]
    ParallelThreshold {
        /// The requested threshold.
        threshold: usize,
        /// How many children the node has.
        children: usize,
    },
}

/// A compiled behavior tree together with its execution state.
///
/// Each [`Tree::tick`] advances as far as the leaves allow: a sequence that
/// sees a child succeed starts the next child in the same tick. The one
/// exception is a looping decorator, which finishes at most one run of its
/// child per tick.
///
/// When the tree stops ticking a running node (a parallel decided, a reactive
/// guard or `WhileDoElse` condition flipped, a timeout expired, or
/// [`Tree::halt`] was called), every active leaf under it is halted exactly
/// once before the call returns. See [`Leaf`] for the full contract.
///
/// Time comes only from the `now` passed to each tick. A host that sleeps
/// between ticks should wake by [`Tree::next_deadline`] so `Sleep`, `Timeout`,
/// and looping nodes act on time.
///
/// After the root finishes, the next tick starts a fresh run.
#[derive(Debug)]
pub struct Tree<L> {
    root: Slot<L>,
}

impl<L> Tree<L> {
    /// Compiles a definition.
    ///
    /// # Errors
    ///
    /// Returns [`BuildError::EmptyComposite`] if a composite has no children,
    /// or [`BuildError::ParallelThreshold`] if a parallel threshold is out of
    /// range.
    pub fn new(root: Node<L>) -> Result<Self, BuildError> {
        Ok(Self {
            root: Slot::compile(root)?,
        })
    }

    /// Ticks the tree once at time `now` and returns the root's status.
    pub fn tick<C: ?Sized>(&mut self, ctx: &mut C, now: Time) -> Status
    where
        L: Leaf<C>,
    {
        self.tick_observed(ctx, now, &mut ())
    }

    /// [`Tree::tick`], reporting every node ticked or halted to `observer`.
    pub fn tick_observed<C, O>(&mut self, ctx: &mut C, now: Time, observer: &mut O) -> Status
    where
        L: Leaf<C>,
        C: ?Sized,
        O: Observer + ?Sized,
    {
        self.root.tick(&mut Run { ctx, observer }, now)
    }

    /// Halts every active leaf and resets the tree. Does nothing if the tree
    /// is not running.
    pub fn halt<C: ?Sized>(&mut self, ctx: &mut C)
    where
        L: Leaf<C>,
    {
        self.halt_observed(ctx, &mut ());
    }

    /// [`Tree::halt`], reporting every node halted to `observer`.
    pub fn halt_observed<C, O>(&mut self, ctx: &mut C, observer: &mut O)
    where
        L: Leaf<C>,
        C: ?Sized,
        O: Observer + ?Sized,
    {
        self.root.halt(&mut Run { ctx, observer });
    }

    /// Whether the last tick returned [`Status::Running`] and the tree has not
    /// been halted since.
    #[must_use]
    pub const fn is_running(&self) -> bool {
        self.root.running
    }

    /// The earliest time a running `Sleep` or `Timeout` node will fire, or a
    /// looping decorator wants its next run (the time of the tick that
    /// finished the last one, so already due), if any. Ticking at or after it
    /// lets that node act; leaves wake the host through their own means.
    #[must_use]
    pub fn next_deadline(&self) -> Option<Time> {
        self.root.next_deadline()
    }
}

#[cfg(test)]
mod tests;
