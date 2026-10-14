//! MOCKED: scripted leaves and a recording context for tree tests.
//!
//! The context also enforces the halt contract on every test: halting a leaf
//! that is not active (never started, already finished, or already halted)
//! panics immediately.

use std::collections::{HashMap, HashSet};

use crate::{Leaf, Node, Status, Threshold, Time, Tree};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Tick(&'static str),
    Halt(&'static str),
}

/// A leaf whose result is whatever the test scripted for its name; unscripted
/// leaves keep running.
#[derive(Debug)]
pub struct Scripted(pub &'static str);

#[derive(Debug, Default)]
pub struct Script {
    outcomes: HashMap<&'static str, Status>,
    active: HashSet<&'static str>,
    pub log: Vec<Event>,
}

impl Script {
    /// Makes every later tick of `name` return `status`.
    pub fn set(&mut self, name: &'static str, status: Status) {
        self.outcomes.insert(name, status);
    }

    pub fn count(&self, event: Event) -> usize {
        self.log.iter().filter(|logged| **logged == event).count()
    }

    pub fn position(&self, event: Event) -> Option<usize> {
        self.log.iter().position(|logged| *logged == event)
    }

    /// Asserts that no leaf is left active, i.e. none leaked an activation.
    pub fn assert_idle(&self) {
        assert!(
            self.active.is_empty(),
            "leaves left active: {:?}",
            self.active
        );
    }
}

impl Leaf<Script> for Scripted {
    fn tick(&self, script: &mut Script, _now: Time) -> Status {
        script.log.push(Event::Tick(self.0));
        let status = script
            .outcomes
            .get(self.0)
            .copied()
            .unwrap_or(Status::Running);
        if status == Status::Running {
            script.active.insert(self.0);
        } else {
            script.active.remove(self.0);
        }
        status
    }

    fn halt(&self, script: &mut Script) {
        assert!(
            script.active.remove(self.0),
            "halted inactive leaf `{}`",
            self.0
        );
        script.log.push(Event::Halt(self.0));
    }
}

pub const fn leaf(name: &'static str) -> Node<Scripted> {
    Node::leaf(Scripted(name))
}

pub fn tree(root: Node<Scripted>) -> Tree<Scripted> {
    Tree::new(root).unwrap()
}

/// `ms` milliseconds after the clock origin.
pub const fn at(ms: u64) -> Time {
    Time::from_nanos(ms * 1_000_000)
}

/// `Parallel` that needs every child to succeed and fails on the first failure.
pub fn parallel_all<L>(children: impl IntoIterator<Item = Node<L>>) -> Node<L> {
    Node::parallel(children, Threshold::All, Threshold::Count(1))
}

/// `Parallel` that succeeds on the first success and fails once all fail.
pub fn parallel_any<L>(children: impl IntoIterator<Item = Node<L>>) -> Node<L> {
    Node::parallel(children, Threshold::Count(1), Threshold::All)
}

/// `Parallel` decided by the first child to finish either way.
pub fn race<L>(children: impl IntoIterator<Item = Node<L>>) -> Node<L> {
    Node::parallel(children, Threshold::Count(1), Threshold::Count(1))
}
