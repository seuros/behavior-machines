use super::*;

const ALL: [Status; 3] = [Status::Success, Status::Failure, Status::Running];

/// The constant status a triple holds, or `None` if it is unticked.
fn decode(bits: &StatusBits) -> Option<Status> {
    ALL.into_iter()
        .find(|status| bits.bit(*status).as_constant() == Some(true))
}

#[test]
fn a_constant_status_round_trips() {
    for status in ALL {
        let bits = StatusBits::constant(status);
        assert_eq!(decode(&bits), Some(status));
        assert_eq!(bits.ticked().as_constant(), Some(true));
    }
}

#[test]
fn unticked_sets_nothing() {
    let bits = StatusBits::unticked();
    assert_eq!(decode(&bits), None);
    assert_eq!(bits.ticked().as_constant(), Some(false));
    assert_eq!(bits.finished().as_constant(), Some(false));
}

#[test]
fn finished_excludes_running() {
    const CASES: &[(Status, bool)] = &[
        (Status::Success, true),
        (Status::Failure, true),
        (Status::Running, false),
    ];
    for (status, expected) in CASES {
        let bits = StatusBits::constant(*status);
        assert_eq!(
            bits.finished().as_constant(),
            Some(*expected),
            "status {status:?}"
        );
    }
}

#[test]
fn inverter_swaps_the_finished_statuses_and_is_free() {
    const CASES: &[(Status, Status)] = &[
        (Status::Success, Status::Failure),
        (Status::Failure, Status::Success),
        (Status::Running, Status::Running),
    ];
    for (from, to) in CASES {
        let inverted = StatusBits::constant(*from).inverted();
        assert_eq!(decode(&inverted), Some(*to), "inverting {from:?}");
    }
}

#[test]
fn force_rewrites_only_finished_statuses() {
    for forced in [Status::Success, Status::Failure] {
        for from in ALL {
            let bits = StatusBits::constant(from).forced(forced);
            let expected = if from == Status::Running {
                Status::Running
            } else {
                forced
            };
            assert_eq!(
                decode(&bits),
                Some(expected),
                "forcing {from:?} to {forced:?}"
            );
        }
    }
}
