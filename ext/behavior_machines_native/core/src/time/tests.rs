use core::time::Duration;

use super::*;

#[test]
fn from_duration_saturates_at_max() {
    let cases = [
        (Duration::ZERO, Time::ZERO),
        (Duration::from_nanos(42), Time::from_nanos(42)),
        (Duration::from_secs(1), Time::from_nanos(1_000_000_000)),
        (Duration::MAX, Time::MAX),
    ];
    for (duration, expected) in cases {
        assert_eq!(
            Time::from_duration(duration),
            expected,
            "duration: {duration:?}"
        );
    }
}

#[test]
fn saturating_duration_since_never_goes_negative() {
    let cases = [
        (
            Time::from_nanos(10),
            Time::from_nanos(4),
            Duration::from_nanos(6),
        ),
        (Time::from_nanos(4), Time::from_nanos(10), Duration::ZERO),
        (Time::MAX, Time::ZERO, Duration::from_nanos(u64::MAX)),
    ];
    for (later, earlier, expected) in cases {
        assert_eq!(
            later.saturating_duration_since(earlier),
            expected,
            "{later:?} - {earlier:?}"
        );
    }
}

#[test]
fn checked_add_reports_overflow() {
    let cases = [
        (
            Time::ZERO,
            Duration::from_nanos(5),
            Some(Time::from_nanos(5)),
        ),
        (Time::MAX, Duration::from_nanos(1), None),
        (Time::ZERO, Duration::MAX, None),
    ];
    for (time, duration, expected) in cases {
        assert_eq!(
            time.checked_add(duration),
            expected,
            "{time:?} + {duration:?}"
        );
    }
}
