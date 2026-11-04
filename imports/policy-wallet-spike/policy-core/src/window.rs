//! UTC-day spend windows.
//!
//! A window is one UTC calendar day. NEAR block timestamps are nanoseconds
//! since the Unix epoch, and the Unix epoch starts at a UTC midnight, so
//! integer division by the length of a day yields the UTC day number
//! directly. Leap seconds do not exist in Unix time, so every day is
//! exactly [`NANOS_PER_DAY`] long.

/// Nanoseconds in one UTC day.
pub const NANOS_PER_DAY: u64 = 86_400 * 1_000_000_000;

/// The UTC day number (days since 1970-01-01) containing `timestamp_ns`.
pub const fn window_index(timestamp_ns: u64) -> u64 {
    timestamp_ns / NANOS_PER_DAY
}

/// The first nanosecond of the UTC day after `timestamp_ns`, saturating at
/// `u64::MAX`. Useful for telling a caller when a spent window resets.
pub const fn next_window_start(timestamp_ns: u64) -> u64 {
    let next = window_index(timestamp_ns).saturating_add(1);
    next.saturating_mul(NANOS_PER_DAY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_is_day_zero() {
        assert_eq!(window_index(0), 0);
        assert_eq!(window_index(NANOS_PER_DAY - 1), 0);
    }

    #[test]
    fn midnight_starts_a_new_day() {
        assert_eq!(window_index(NANOS_PER_DAY), 1);
        // 2026-11-01T00:00:00Z is Unix second 1_793_491_200, day 20_758.
        let nov_first_ns = 1_793_491_200u64 * 1_000_000_000;
        assert_eq!(window_index(nov_first_ns), 20_758);
        assert_eq!(window_index(nov_first_ns - 1), 20_757);
    }

    #[test]
    fn next_window_start_is_next_midnight() {
        assert_eq!(next_window_start(0), NANOS_PER_DAY);
        assert_eq!(next_window_start(NANOS_PER_DAY - 1), NANOS_PER_DAY);
        assert_eq!(next_window_start(NANOS_PER_DAY), 2 * NANOS_PER_DAY);
    }

    #[test]
    fn next_window_start_saturates() {
        assert_eq!(next_window_start(u64::MAX), u64::MAX);
    }
}
