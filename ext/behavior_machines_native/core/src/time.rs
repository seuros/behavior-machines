use core::time::Duration;

/// A reading of the tree's monotonic clock, in nanoseconds from an origin the
/// host chooses.
///
/// The tree never reads a clock itself. The host passes the current `Time` to
/// every tick, so simulations, replays, and tests control time completely.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Time(u64);

impl Time {
    /// The clock origin.
    pub const ZERO: Self = Self(0);
    /// The latest representable time, roughly 584 years after the origin.
    pub const MAX: Self = Self(u64::MAX);

    /// Creates a time `nanos` nanoseconds after the origin.
    #[must_use]
    pub const fn from_nanos(nanos: u64) -> Self {
        Self(nanos)
    }

    /// Nanoseconds since the origin.
    #[must_use]
    pub const fn as_nanos(self) -> u64 {
        self.0
    }

    /// Creates a time `since_origin` after the origin, saturating at
    /// [`Time::MAX`].
    #[must_use]
    pub fn from_duration(since_origin: Duration) -> Self {
        Self(u64::try_from(since_origin.as_nanos()).unwrap_or(u64::MAX))
    }

    /// Time elapsed since `earlier`, or zero if `earlier` is later than `self`.
    #[must_use]
    pub const fn saturating_duration_since(self, earlier: Self) -> Duration {
        Duration::from_nanos(self.0.saturating_sub(earlier.0))
    }

    /// `self + duration`, or `None` if the result is past [`Time::MAX`].
    #[must_use]
    pub fn checked_add(self, duration: Duration) -> Option<Self> {
        let nanos = u64::try_from(duration.as_nanos()).ok()?;
        self.0.checked_add(nanos).map(Self)
    }
}

#[cfg(test)]
mod tests;
