use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use core::time::Duration;

use super::composite;
use super::control::{self, Branch};
use super::decorator::{self, Map};
use crate::observe::{NodeId, Observer};
use crate::{BuildError, Composite, Leaf, Limit, Node, Status, Threshold, Time};

/// What a tick or halt needs besides the node itself.
pub(super) struct Run<'a, C: ?Sized, O: ?Sized> {
    pub ctx: &'a mut C,
    pub observer: &'a mut O,
}

/// One compiled node. `running` is set while the node's last tick returned
/// `Running` and it has not been halted since; only running nodes are halted.
#[derive(Debug)]
pub(super) struct Slot<L> {
    id: NodeId,
    pub running: bool,
    kind: Kind<L>,
}

#[derive(Debug)]
enum Kind<L> {
    Leaf(L),
    /// `AlwaysSuccess` and `AlwaysFailure`.
    Constant(Status),
    Sleep {
        duration: Duration,
        /// `None` until started, or when the deadline is past the clock's end.
        deadline: Option<Time>,
    },
    /// Sequence advances past children that succeed, fallback past children
    /// that fail. A memory node resumes at `cursor`; a reactive one restarts
    /// from the first child every tick.
    Ordered {
        children: Vec<Slot<L>>,
        cursor: usize,
        advance_on: Status,
        reactive: bool,
    },
    /// Thresholds resolved to counts; `outcomes` holds each finished child's
    /// result for the current activation.
    Parallel {
        children: Vec<Slot<L>>,
        outcomes: Vec<Option<Status>>,
        success: usize,
        failure: usize,
    },
    /// `IfThenElse` (`reactive: false`) and `WhileDoElse` (`reactive: true`).
    Conditional {
        condition: Box<Slot<L>>,
        then: Box<Slot<L>>,
        otherwise: Option<Box<Slot<L>>>,
        reactive: bool,
        branch: Branch,
    },
    Map {
        child: Box<Slot<L>>,
        map: Map,
    },
    Timeout {
        child: Box<Slot<L>>,
        limit: Duration,
        /// `None` until started, or when the deadline is past the clock's end.
        deadline: Option<Time>,
    },
    /// `Repeat` and `KeepRunningUntilFailure` loop on `Success`,
    /// `RetryUntilSuccessful` on `Failure`; `runs` counts finished runs.
    Loop {
        child: Box<Slot<L>>,
        again_on: Status,
        limit: Option<usize>,
        runs: usize,
    },
}

impl<L> Slot<L> {
    pub fn compile(root: Node<L>) -> Result<Self, BuildError> {
        Compiler { next_id: 0 }.node(root)
    }

    pub fn tick<C, O>(&mut self, run: &mut Run<'_, C, O>, now: Time) -> Status
    where
        L: Leaf<C>,
        C: ?Sized,
        O: Observer + ?Sized,
    {
        let starting = !self.running;
        let status = match &mut self.kind {
            Kind::Leaf(leaf) => leaf.tick(run.ctx, now),
            Kind::Constant(status) => *status,
            Kind::Sleep { duration, deadline } => {
                decorator::sleep(*duration, deadline, starting, now)
            }
            Kind::Ordered {
                children,
                cursor,
                advance_on,
                reactive: false,
            } => composite::ordered(children, cursor, *advance_on, run, now),
            Kind::Ordered {
                children,
                advance_on,
                reactive: true,
                ..
            } => composite::reactive(children, *advance_on, run, now),
            Kind::Parallel {
                children,
                outcomes,
                success,
                failure,
            } => composite::parallel(children, outcomes, *success, *failure, run, now),
            Kind::Conditional {
                condition,
                then,
                otherwise,
                reactive: false,
                branch,
            } => control::if_then_else(condition, then, otherwise.as_deref_mut(), branch, run, now),
            Kind::Conditional {
                condition,
                then,
                otherwise,
                reactive: true,
                ..
            } => control::while_do_else(condition, then, otherwise.as_deref_mut(), run, now),
            Kind::Map { child, map } => map.apply(child.tick(run, now)),
            Kind::Timeout {
                child,
                limit,
                deadline,
            } => decorator::timeout(child, *limit, deadline, starting, run, now),
            Kind::Loop {
                child,
                again_on,
                limit,
                runs,
            } => decorator::looped(child, *again_on, *limit, runs, run, now),
        };
        self.running = status == Status::Running;
        run.observer.ticked(self.id, status);
        status
    }

    pub fn halt<C, O>(&mut self, run: &mut Run<'_, C, O>)
    where
        L: Leaf<C>,
        C: ?Sized,
        O: Observer + ?Sized,
    {
        if !self.running {
            return;
        }
        self.running = false;
        match &mut self.kind {
            Kind::Leaf(leaf) => leaf.halt(run.ctx),
            Kind::Constant(_) | Kind::Sleep { .. } => {}
            Kind::Ordered {
                children, cursor, ..
            } => {
                composite::halt_all(children, run);
                *cursor = 0;
            }
            Kind::Parallel {
                children, outcomes, ..
            } => {
                composite::halt_all(children, run);
                outcomes.fill(None);
            }
            Kind::Conditional {
                condition,
                then,
                otherwise,
                branch,
                ..
            } => {
                condition.halt(run);
                then.halt(run);
                if let Some(otherwise) = otherwise {
                    otherwise.halt(run);
                }
                *branch = Branch::Condition;
            }
            Kind::Map { child, .. } | Kind::Timeout { child, .. } => child.halt(run),
            Kind::Loop { child, runs, .. } => {
                child.halt(run);
                *runs = 0;
            }
        }
        run.observer.halted(self.id);
    }

    /// The earliest deadline of a running timed node in this subtree.
    pub fn next_deadline(&self) -> Option<Time> {
        if !self.running {
            return None;
        }
        match &self.kind {
            Kind::Leaf(_) | Kind::Constant(_) => None,
            Kind::Sleep { deadline, .. } => *deadline,
            Kind::Timeout {
                child, deadline, ..
            } => earliest([*deadline, child.next_deadline()]),
            Kind::Map { child, .. } | Kind::Loop { child, .. } => child.next_deadline(),
            Kind::Ordered { children, .. } | Kind::Parallel { children, .. } => {
                earliest(children.iter().map(Self::next_deadline))
            }
            // A running `WhileDoElse` condition pauses the branches, so their
            // timers cannot act until it finishes.
            Kind::Conditional {
                condition,
                reactive: true,
                ..
            } if condition.running => condition.next_deadline(),
            Kind::Conditional {
                condition,
                then,
                otherwise,
                ..
            } => earliest(
                [Some(condition), Some(then), otherwise.as_ref()]
                    .into_iter()
                    .flatten()
                    .map(|slot| slot.next_deadline()),
            ),
        }
    }
}

fn earliest(deadlines: impl IntoIterator<Item = Option<Time>>) -> Option<Time> {
    deadlines.into_iter().flatten().min()
}

/// Assigns pre-order [`NodeId`]s while compiling.
struct Compiler {
    next_id: usize,
}

impl Compiler {
    fn node<L>(&mut self, node: Node<L>) -> Result<Slot<L>, BuildError> {
        let id = NodeId::new(self.next_id);
        self.next_id += 1;
        let kind = match node {
            Node::Leaf(leaf) => Kind::Leaf(leaf),
            Node::AlwaysSuccess => Kind::Constant(Status::Success),
            Node::AlwaysFailure => Kind::Constant(Status::Failure),
            Node::Sleep(duration) => Kind::Sleep {
                duration,
                deadline: None,
            },
            Node::Sequence(nodes) => {
                self.ordered(Composite::Sequence, nodes, Status::Success, false)?
            }
            Node::ReactiveSequence(nodes) => {
                self.ordered(Composite::ReactiveSequence, nodes, Status::Success, true)?
            }
            Node::Fallback(nodes) => {
                self.ordered(Composite::Fallback, nodes, Status::Failure, false)?
            }
            Node::ReactiveFallback(nodes) => {
                self.ordered(Composite::ReactiveFallback, nodes, Status::Failure, true)?
            }
            Node::Parallel {
                children,
                success,
                failure,
            } => self.parallel(children, success, failure)?,
            Node::IfThenElse {
                condition,
                then,
                otherwise,
            } => self.conditional(*condition, *then, otherwise, false)?,
            Node::WhileDoElse {
                condition,
                body,
                otherwise,
            } => self.conditional(*condition, *body, otherwise, true)?,
            Node::Inverter(child) => self.map(*child, Map::Inverter)?,
            Node::ForceSuccess(child) => self.map(*child, Map::Force(Status::Success))?,
            Node::ForceFailure(child) => self.map(*child, Map::Force(Status::Failure))?,
            Node::Timeout(limit, child) => Kind::Timeout {
                child: self.boxed(*child)?,
                limit,
                deadline: None,
            },
            Node::Repeat(limit, child) => self.looped(*child, Status::Success, limit)?,
            Node::RetryUntilSuccessful(limit, child) => {
                self.looped(*child, Status::Failure, limit)?
            }
            Node::KeepRunningUntilFailure(child) => {
                self.looped(*child, Status::Success, Limit::Unlimited)?
            }
        };
        Ok(Slot {
            id,
            running: false,
            kind,
        })
    }

    fn children<L>(
        &mut self,
        kind: Composite,
        nodes: Vec<Node<L>>,
    ) -> Result<Vec<Slot<L>>, BuildError> {
        if nodes.is_empty() {
            return Err(BuildError::EmptyComposite(kind));
        }
        nodes.into_iter().map(|node| self.node(node)).collect()
    }

    fn boxed<L>(&mut self, node: Node<L>) -> Result<Box<Slot<L>>, BuildError> {
        self.node(node).map(Box::new)
    }

    fn ordered<L>(
        &mut self,
        kind: Composite,
        nodes: Vec<Node<L>>,
        advance_on: Status,
        reactive: bool,
    ) -> Result<Kind<L>, BuildError> {
        Ok(Kind::Ordered {
            children: self.children(kind, nodes)?,
            cursor: 0,
            advance_on,
            reactive,
        })
    }

    fn parallel<L>(
        &mut self,
        nodes: Vec<Node<L>>,
        success: Threshold,
        failure: Threshold,
    ) -> Result<Kind<L>, BuildError> {
        let children = self.children(Composite::Parallel, nodes)?;
        let count = |threshold| match threshold {
            Threshold::All => Ok(children.len()),
            Threshold::Count(threshold) if (1..=children.len()).contains(&threshold) => {
                Ok(threshold)
            }
            Threshold::Count(threshold) => Err(BuildError::ParallelThreshold {
                threshold,
                children: children.len(),
            }),
        };
        let (success, failure) = (count(success)?, count(failure)?);
        Ok(Kind::Parallel {
            outcomes: vec![None; children.len()],
            children,
            success,
            failure,
        })
    }

    fn conditional<L>(
        &mut self,
        condition: Node<L>,
        then: Node<L>,
        otherwise: Option<Box<Node<L>>>,
        reactive: bool,
    ) -> Result<Kind<L>, BuildError> {
        Ok(Kind::Conditional {
            condition: self.boxed(condition)?,
            then: self.boxed(then)?,
            otherwise: otherwise.map(|node| self.boxed(*node)).transpose()?,
            reactive,
            branch: Branch::Condition,
        })
    }

    fn map<L>(&mut self, child: Node<L>, map: Map) -> Result<Kind<L>, BuildError> {
        Ok(Kind::Map {
            child: self.boxed(child)?,
            map,
        })
    }

    fn looped<L>(
        &mut self,
        child: Node<L>,
        again_on: Status,
        limit: Limit,
    ) -> Result<Kind<L>, BuildError> {
        let limit = match limit {
            Limit::Times(times) => Some(times),
            Limit::Unlimited => None,
        };
        Ok(Kind::Loop {
            child: self.boxed(child)?,
            again_on,
            limit,
            runs: 0,
        })
    }
}
