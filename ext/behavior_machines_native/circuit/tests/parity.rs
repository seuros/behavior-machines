//! The circuit backend must agree with the engine on every tick.
//!
//! For each tree, every assignment of leaf statuses over several consecutive
//! ticks is run through both the engine and the circuit, and the three
//! observable results are compared: the root status, the sequence of ticked
//! nodes with their statuses, and the sequence of halted nodes. Both sequences
//! are compared in the engine's observer order, so a disagreement about *when*
//! a node was reported fails too.
//!
//! Exhaustive coverage is affordable here: three ticks of a three-leaf tree is
//! 3^9 = 19,683 runs, and the circuit evaluates a tree of that size in
//! microseconds.

use behavior_machines::{Leaf, Node, NodeId, Observer, Status, Time, Tree};
use behavior_machines_circuit::{Lowered, State};

/// A leaf that returns whatever the script says for this tick.
struct Scripted(usize);

/// Per-tick leaf statuses, plus a record of which leaves were halted.
struct Script {
    status: Vec<Status>,
    halted: Vec<usize>,
}

impl Leaf<Script> for Scripted {
    fn tick(&self, script: &mut Script, _now: Time) -> Status {
        script.status[self.0]
    }

    fn halt(&self, script: &mut Script) {
        script.halted.push(self.0);
    }
}

/// Records what the engine reports, to compare against the schedule.
#[derive(Default)]
struct Record {
    ticked: Vec<(usize, Status)>,
    halted: Vec<usize>,
}

impl Observer for Record {
    fn ticked(&mut self, node: NodeId, status: Status) {
        self.ticked.push((node.index(), status));
    }

    fn halted(&mut self, node: NodeId) {
        self.halted.push(node.index());
    }
}

const STATUSES: [Status; 3] = [Status::Success, Status::Failure, Status::Running];

/// Numbers a tree's leaves in the pre-order the engine assigns node ids in,
/// which is the order the circuit expects leaf statuses in.
fn scripted(tree: &Node<()>) -> Node<Scripted> {
    let mut next = 0;
    tree.clone().map(|()| {
        let slot = next;
        next += 1;
        Scripted(slot)
    })
}

fn leaf_count(tree: &Node<()>) -> usize {
    let mut count = 0;
    tree.clone().map(|()| count += 1);
    count
}

/// Runs one script through both backends and compares every tick.
///
/// `script` holds one status per leaf per tick, laid out tick-major.
fn compare(tree: &Node<()>, leaves: usize, ticks: usize, script: &[Status]) {
    let Ok(mut engine) = Tree::new(scripted(tree)) else {
        panic!("the tree is valid")
    };
    let Ok(lowered) = Lowered::new(tree) else {
        panic!("every node is supported")
    };
    let mut state = State::initial(&lowered);

    for tick in 0..ticks {
        let statuses = &script[tick * leaves..(tick + 1) * leaves];

        let mut context = Script {
            status: statuses.to_vec(),
            halted: Vec::new(),
        };
        let mut record = Record::default();
        let engine_root = engine.tick_observed(&mut context, Time::ZERO, &mut record);

        let Ok(schedule) = lowered.evaluate(&mut state, statuses) else {
            panic!("the state and statuses came from this tree")
        };

        let context = || format!("tree {tree:?}\nscript {script:?}\ntick {tick}");

        assert_eq!(
            Some(engine_root),
            schedule.root(),
            "root status disagrees\n{}",
            context(),
        );
        assert_eq!(
            record.ticked,
            schedule.ticked(lowered.plan()).collect::<Vec<_>>(),
            "ticked nodes disagree\n{}",
            context(),
        );
        assert_eq!(
            record.halted,
            schedule.halts(lowered.plan()).collect::<Vec<_>>(),
            "halted nodes disagree\n{}",
            context(),
        );
        assert_eq!(
            engine.is_running(),
            state.is_tree_running(),
            "running flag disagrees\n{}",
            context(),
        );
    }
}

/// Every assignment of statuses to `slots` positions, as a flat counter in
/// base 3.
fn each_assignment(slots: usize, mut visit: impl FnMut(&[Status])) {
    let mut assignment = vec![Status::Success; slots];
    let Ok(slots_exponent) = u32::try_from(slots) else {
        panic!("few enough slots")
    };
    let total = 3usize.pow(slots_exponent);
    for encoded in 0..total {
        let mut rest = encoded;
        for slot in &mut assignment {
            *slot = STATUSES[rest % 3];
            rest /= 3;
        }
        visit(&assignment);
    }
}

fn check_exhaustively(tree: &Node<()>, ticks: usize) {
    let leaves = leaf_count(tree);
    each_assignment(leaves * ticks, |script| {
        compare(tree, leaves, ticks, script);
    });
}

const fn leaf() -> Node<()> {
    Node::leaf(())
}

#[test]
fn sequence_matches_the_engine() {
    check_exhaustively(&Node::sequence([leaf(), leaf(), leaf()]), 3);
}

#[test]
fn fallback_matches_the_engine() {
    check_exhaustively(&Node::fallback([leaf(), leaf(), leaf()]), 3);
}

#[test]
fn reactive_sequence_matches_the_engine() {
    check_exhaustively(&Node::reactive_sequence([leaf(), leaf(), leaf()]), 3);
}

#[test]
fn reactive_fallback_matches_the_engine() {
    check_exhaustively(&Node::reactive_fallback([leaf(), leaf(), leaf()]), 3);
}

#[test]
fn decorators_match_the_engine() {
    check_exhaustively(
        &Node::sequence([
            leaf().inverter(),
            leaf().force_success(),
            leaf().force_failure(),
        ]),
        3,
    );
}

#[test]
fn constants_match_the_engine() {
    check_exhaustively(
        &Node::sequence([Node::AlwaysSuccess, leaf(), Node::AlwaysFailure, leaf()]),
        2,
    );
}

#[test]
fn reactive_sequence_halting_a_running_subtree_matches_the_engine() {
    // A guard that flips must halt the whole running subtree beneath the
    // later child, which is the case the halt propagation exists for.
    check_exhaustively(
        &Node::reactive_sequence([leaf(), Node::sequence([leaf(), leaf()])]),
        3,
    );
}

#[test]
fn nested_memory_composites_match_the_engine() {
    // Two cursors in one tree, the inner one advancing only on the ticks the
    // outer one reaches it.
    check_exhaustively(
        &Node::sequence([Node::sequence([leaf(), leaf()]), leaf()]),
        3,
    );
}

#[test]
fn memory_composite_under_a_reactive_guard_matches_the_engine() {
    // The canonical reactive shape: a guarded action with a fallback. The
    // guard can abandon a part-way sequence, which must reset its cursor; on
    // other ticks the sequence is simply not reached and must keep it.
    check_exhaustively(
        &Node::reactive_fallback([Node::sequence([leaf(), leaf()]), leaf()]),
        3,
    );
}

#[test]
fn a_memory_composite_keeps_its_cursor_over_many_ticks() {
    // Four ticks, so a cursor has to survive more than one unreached tick.
    check_exhaustively(
        &Node::reactive_sequence([leaf(), Node::sequence([leaf()])]),
        4,
    );
}

#[test]
fn deeply_nested_matches_the_engine() {
    check_exhaustively(
        &Node::reactive_fallback([
            Node::sequence([leaf(), leaf().inverter()]),
            Node::reactive_sequence([leaf(), Node::fallback([leaf()])]),
        ]),
        2,
    );
}
