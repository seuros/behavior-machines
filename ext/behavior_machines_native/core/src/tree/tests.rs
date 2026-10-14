use alloc::boxed::Box;
use core::time::Duration;

use super::*;
use crate::test_support::Event::{Halt, Tick};
use crate::test_support::{Script, Scripted, at, leaf, parallel_all, race, tree};
use crate::{Limit, Threshold};

use Status::{Failure, Running, Success};

const NOW: Time = Time::ZERO;

#[test]
fn rejects_empty_composites_at_any_depth() {
    let cases: [(Node<Scripted>, Composite); 6] = [
        (Node::sequence([]), Composite::Sequence),
        (Node::reactive_sequence([]), Composite::ReactiveSequence),
        (Node::fallback([]), Composite::Fallback),
        (Node::reactive_fallback([]), Composite::ReactiveFallback),
        (parallel_all([]), Composite::Parallel),
        (
            Node::sequence([leaf("a"), Node::fallback([]).inverter()]),
            Composite::Fallback,
        ),
    ];
    for (root, kind) in cases {
        assert_eq!(
            Tree::new(root).err(),
            Some(BuildError::EmptyComposite(kind)),
            "{kind}"
        );
    }
    assert_eq!(
        BuildError::EmptyComposite(Composite::ReactiveFallback).to_string(),
        "ReactiveFallback needs at least one child"
    );
}

#[test]
fn rejects_parallel_thresholds_outside_the_child_count() {
    let two = || [leaf("a"), leaf("b")];
    let cases = [
        (Threshold::Count(0), Threshold::All, 0),
        (Threshold::All, Threshold::Count(3), 3),
    ];
    for (success, failure, threshold) in cases {
        let error = Tree::new(Node::parallel(two(), success, failure)).err();
        assert_eq!(
            error,
            Some(BuildError::ParallelThreshold {
                threshold,
                children: 2
            })
        );
    }
    assert_eq!(
        BuildError::ParallelThreshold {
            threshold: 0,
            children: 2
        }
        .to_string(),
        "Parallel threshold 0 must be between 1 and 2"
    );
}

#[test]
fn ordered_composites_advance_within_one_tick() {
    type Build = fn(Vec<Node<Scripted>>) -> Node<Scripted>;
    type Case = (
        Build,
        &'static [(&'static str, Status)],
        Status,
        &'static [&'static str],
    );
    let cases: [Case; 4] = [
        (
            Node::Sequence,
            &[("a", Success), ("b", Success), ("c", Success)],
            Success,
            &["a", "b", "c"],
        ),
        (
            Node::Sequence,
            &[("a", Success), ("b", Failure)],
            Failure,
            &["a", "b"],
        ),
        (
            Node::Fallback,
            &[("a", Failure), ("b", Failure), ("c", Failure)],
            Failure,
            &["a", "b", "c"],
        ),
        (
            Node::Fallback,
            &[("a", Failure), ("b", Success)],
            Success,
            &["a", "b"],
        ),
    ];
    for (build, outcomes, expected, ticked) in cases {
        let mut script = Script::default();
        for &(name, status) in outcomes {
            script.set(name, status);
        }
        let mut tree = tree(build(vec![leaf("a"), leaf("b"), leaf("c")]));

        assert_eq!(
            tree.tick(&mut script, NOW),
            expected,
            "outcomes: {outcomes:?}"
        );
        let expected_log: Vec<_> = ticked.iter().map(|name| Tick(name)).collect();
        assert_eq!(script.log, expected_log, "outcomes: {outcomes:?}");
        script.assert_idle();
    }
}

#[test]
fn sequence_resumes_the_running_child() {
    let mut script = Script::default();
    let mut tree = tree(Node::sequence([leaf("a"), leaf("b")]));

    assert_eq!(tree.tick(&mut script, NOW), Running);
    script.set("a", Success);
    assert_eq!(tree.tick(&mut script, NOW), Running);
    script.set("b", Success);
    assert_eq!(tree.tick(&mut script, NOW), Success);

    assert_eq!(script.log, [Tick("a"), Tick("a"), Tick("b"), Tick("b")]);
    script.assert_idle();
}

#[test]
fn parallel_all_succeeds_without_reticking_finished_children() {
    let mut script = Script::default();
    let mut tree = tree(parallel_all([leaf("a"), leaf("b")]));

    assert_eq!(tree.tick(&mut script, NOW), Running);
    script.set("a", Success);
    assert_eq!(tree.tick(&mut script, NOW), Running);
    script.set("b", Success);
    assert_eq!(tree.tick(&mut script, NOW), Success);

    assert_eq!(script.count(Tick("a")), 2);
    assert_eq!(script.count(Tick("b")), 3);
    script.assert_idle();
}

#[test]
fn parallel_failure_halts_running_siblings_on_both_sides() {
    let mut script = Script::default();
    let mut tree = tree(parallel_all([leaf("a"), leaf("b"), leaf("c")]));

    assert_eq!(tree.tick(&mut script, NOW), Running);
    script.set("b", Failure);
    assert_eq!(tree.tick(&mut script, NOW), Failure);

    assert_eq!(script.count(Halt("a")), 1);
    assert_eq!(script.count(Halt("b")), 0);
    assert_eq!(script.count(Halt("c")), 1);
    assert!(!tree.is_running());
    script.assert_idle();
}

#[test]
fn immediate_decisions_never_start_or_halt_later_children() {
    let cases: [(Node<Scripted>, Status); 2] = [
        (parallel_all([leaf("first"), leaf("later")]), Failure),
        (race([leaf("first"), leaf("later")]), Failure),
    ];
    for (root, expected) in cases {
        let mut script = Script::default();
        script.set("first", expected);
        let mut tree = tree(root);

        assert_eq!(tree.tick(&mut script, NOW), expected);
        assert_eq!(script.log, [Tick("first")]);
        script.assert_idle();
    }
}

#[test]
fn race_winner_decides_and_halts_losers_on_both_sides() {
    for winner_status in [Success, Failure] {
        let mut script = Script::default();
        let mut tree = tree(race([leaf("a"), leaf("b"), leaf("winner"), leaf("d")]));

        assert_eq!(tree.tick(&mut script, NOW), Running);
        script.set("winner", winner_status);
        assert_eq!(tree.tick(&mut script, NOW), winner_status);

        for loser in ["a", "b", "d"] {
            assert_eq!(
                script.count(Halt(loser)),
                1,
                "{loser} with {winner_status:?}"
            );
        }
        assert_eq!(script.count(Halt("winner")), 0);
        script.assert_idle();
    }
}

#[test]
fn race_tie_goes_to_the_leftmost_child() {
    let mut script = Script::default();
    let mut tree = tree(race([leaf("left"), leaf("right")]));

    assert_eq!(tree.tick(&mut script, NOW), Running);
    script.set("left", Failure);
    script.set("right", Success);
    assert_eq!(tree.tick(&mut script, NOW), Failure);

    assert_eq!(script.count(Halt("right")), 1);
    script.assert_idle();
}

#[test]
fn abandoned_nested_branch_is_halted_before_the_next_child_starts() {
    let mut script = Script::default();
    let mut tree = tree(Node::sequence([
        race([leaf("winner"), leaf("loser")]),
        leaf("next"),
    ]));

    assert_eq!(tree.tick(&mut script, NOW), Running);
    script.set("winner", Success);
    assert_eq!(tree.tick(&mut script, NOW), Running);

    let halted = script.position(Halt("loser")).unwrap();
    let started = script.position(Tick("next")).unwrap();
    assert!(halted < started, "log: {:?}", script.log);
    assert_eq!(script.count(Halt("loser")), 1);
}

#[test]
fn finished_branches_are_not_halted_again() {
    let mut script = Script::default();
    let mut tree = tree(parallel_all([
        race([leaf("winner"), leaf("loser")]),
        leaf("other"),
    ]));

    assert_eq!(tree.tick(&mut script, NOW), Running);
    script.set("winner", Success);
    assert_eq!(tree.tick(&mut script, NOW), Running);
    script.set("other", Failure);
    assert_eq!(tree.tick(&mut script, NOW), Failure);

    assert_eq!(script.count(Halt("loser")), 1);
    assert_eq!(script.count(Halt("winner")), 0);
    script.assert_idle();
}

#[test]
fn tree_halt_halts_every_active_leaf_once() {
    let mut script = Script::default();
    script.set("ready", Success);
    let mut tree = tree(Node::fallback([
        Node::sequence([leaf("ready"), parallel_all([leaf("a"), leaf("b")])]),
        leaf("unreached"),
    ]));

    assert_eq!(tree.tick(&mut script, NOW), Running);
    tree.halt(&mut script);
    tree.halt(&mut script);

    assert_eq!(script.count(Halt("a")), 1);
    assert_eq!(script.count(Halt("b")), 1);
    assert_eq!(script.count(Tick("unreached")), 0);
    assert!(!tree.is_running());
    script.assert_idle();
}

#[test]
fn tree_restarts_after_finishing_or_halting() {
    let mut script = Script::default();
    script.set("a", Success);
    let mut tree = tree(Node::sequence([leaf("a"), leaf("b")]));

    assert_eq!(tree.tick(&mut script, NOW), Running);
    tree.halt(&mut script);
    assert_eq!(tree.tick(&mut script, NOW), Running);
    script.set("b", Success);
    assert_eq!(tree.tick(&mut script, NOW), Success);
    assert_eq!(tree.tick(&mut script, NOW), Success);

    assert_eq!(script.count(Tick("a")), 3);
    assert_eq!(script.count(Tick("b")), 4);
    assert_eq!(script.count(Halt("b")), 1);
    script.assert_idle();
}

#[test]
fn boxed_leaves_forward_tick_and_halt() {
    let mut script = Script::default();
    let boxed: Box<dyn Leaf<Script>> = Box::new(Scripted("boxed"));
    let mut tree = Tree::new(race([Node::leaf(boxed)])).unwrap();

    assert_eq!(tree.tick(&mut script, NOW), Running);
    tree.halt(&mut script);

    assert_eq!(script.log, [Tick("boxed"), Halt("boxed")]);
    script.assert_idle();
}

/// Every stateful node shape, over leaves `a`, `b`, `c`.
fn lifecycle_shapes() -> [Node<Scripted>; 21] {
    let abc = || [leaf("a"), leaf("b"), leaf("c")];
    let ms = Duration::from_millis;
    let two = Threshold::Count(2);
    [
        Node::sequence(abc()),
        Node::reactive_sequence(abc()),
        Node::fallback(abc()),
        Node::reactive_fallback(abc()),
        parallel_all(abc()),
        crate::test_support::parallel_any(abc()),
        race(abc()),
        Node::parallel(abc(), two, two),
        Node::if_then_else(leaf("a"), leaf("b"), leaf("c")),
        Node::if_then(leaf("a"), leaf("b")),
        Node::while_do_else(leaf("a"), leaf("b"), leaf("c")),
        Node::while_do(leaf("a"), leaf("b")),
        Node::sequence(abc()).inverter(),
        parallel_all(abc()).force_success(),
        Node::fallback(abc()).force_failure(),
        Node::sequence(abc()).timeout(ms(2)),
        Node::sequence(abc()).repeat(Limit::Times(2)),
        Node::fallback(abc()).retry_until_successful(Limit::Times(2)),
        Node::sequence(abc()).keep_running_until_failure(),
        Node::sequence([Node::sleep(ms(2)), leaf("a")]),
        Node::fallback([Node::AlwaysFailure, leaf("a"), Node::AlwaysSuccess]),
    ]
}

/// Scripts `a`, `b`, `c` from the base-3 digits of `code`.
fn script_outcomes(script: &mut Script, mut code: usize) {
    for name in ["a", "b", "c"] {
        script.set(name, [Running, Success, Failure][code % 3]);
        code /= 3;
    }
}

/// Every node shape, through every pair of leaf outcomes: no leaked
/// activations, no double halts (the script panics on those), and a tree that
/// finished or was halted behaves exactly like a freshly compiled one.
#[test]
fn every_shape_resets_to_fresh_after_finishing_or_halting() {
    for shape in 0..lifecycle_shapes().len() {
        for first in 0..27 {
            for second in 0..27 {
                let build = || lifecycle_shapes().into_iter().nth(shape).unwrap();
                let case = format!("shape={shape}, first={first}, second={second}");
                let mut script = Script::default();
                let mut reused = tree(build());

                for (ms, code) in [(0, first), (1, second)] {
                    script_outcomes(&mut script, code);
                    if reused.tick(&mut script, at(ms)) != Running {
                        script.assert_idle();
                        assert_eq!(reused.next_deadline(), None, "{case}");
                    }
                }
                reused.halt(&mut script);
                script.assert_idle();
                let halted = script.log.len();
                reused.halt(&mut script);
                assert_eq!(script.log.len(), halted, "{case}: second halt acted");
                assert_eq!(reused.next_deadline(), None, "{case}");

                script.log.clear();
                let mut fresh_script = Script::default();
                let mut fresh = tree(build());
                for (ms, code) in [(10, second), (11, first), (12, second)] {
                    script_outcomes(&mut script, code);
                    script_outcomes(&mut fresh_script, code);
                    assert_eq!(
                        reused.tick(&mut script, at(ms)),
                        fresh.tick(&mut fresh_script, at(ms)),
                        "{case}, ms={ms}"
                    );
                    assert_eq!(
                        reused.next_deadline(),
                        fresh.next_deadline(),
                        "{case}, ms={ms}"
                    );
                    assert_eq!(script.log, fresh_script.log, "{case}, ms={ms}");
                }
                reused.halt(&mut script);
                fresh.halt(&mut fresh_script);
                assert_eq!(script.log, fresh_script.log, "{case}");
                script.assert_idle();
            }
        }
    }
}
