//! The machine's local time zone, which a person reads a clock in.

use std::time::SystemTime;

/// How many seconds local time is ahead of UTC at `at`, zone and daylight saving included; `0`
/// when the zone cannot be read.
pub fn utc_offset(at: SystemTime) -> i64 {
    let Ok(since) = at.duration_since(SystemTime::UNIX_EPOCH) else {
        return 0;
    };
    let Ok(seconds) = libc::time_t::try_from(since.as_secs()) else {
        return 0;
    };
    // SAFETY: `localtime_r` writes only to the `tm` it is given, and the `time_t` is a value.
    let mut local: libc::tm = unsafe { std::mem::zeroed() };
    let read = unsafe { libc::localtime_r(&raw const seconds, &raw mut local) };
    if read.is_null() { 0 } else { local.tm_gmtoff }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_offset_is_a_real_zone_offset() {
        // The epoch is a fixed instant: the offset is whatever zone the machine is in, within
        // the range zones come in.
        let offset = utc_offset(SystemTime::UNIX_EPOCH);
        assert!((-12 * 3600..=14 * 3600).contains(&offset), "{offset}");
    }
}
