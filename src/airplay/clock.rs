//! Local time base: CLOCK_MONOTONIC in ns (same clock as MediaCodec / AAudio timestamps).

pub const SEC: u64 = 1_000_000_000;
/// NTP epoch (1900) to Unix epoch (1970).
pub const SECONDS_1900_TO_1970: u64 = 2_208_988_800;

pub fn now_ns() -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: valid pointer to a timespec on the stack.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as u64 * SEC + ts.tv_nsec as u64
}

pub fn monotonic_ns() -> u64 {
    now_ns()
}

pub fn ns_to_ntp(ns: u64) -> u64 {
    let mut b = [0u8; 8];
    put_ntp(&mut b, ns);
    u64::from_be_bytes(b)
}

pub fn ntp_to_ns(ntp: u64) -> u64 {
    get_ntp(&ntp.to_be_bytes())
}

/// NTP 32.32 fixed point (big-endian on the wire) from nanoseconds.
pub fn put_ntp(buf: &mut [u8], ns: u64) {
    let secs = (ns / SEC) as u32;
    let frac = (((ns % SEC) << 32) / SEC) as u32;
    buf[..4].copy_from_slice(&secs.to_be_bytes());
    buf[4..8].copy_from_slice(&frac.to_be_bytes());
}

/// Convert a 32.32 big-endian NTP timestamp to nanoseconds without underflow.
pub fn get_ntp(buf: &[u8]) -> u64 {
    let secs = u32::from_be_bytes(buf[..4].try_into().unwrap()) as u64;
    let frac = u32::from_be_bytes(buf[4..8].try_into().unwrap()) as u64;
    secs * SEC + ((frac * SEC) >> 32)
}

/// 32.32 fixed point (seconds in the high word) to ns.
pub fn fixed_to_ns(v: u64, minus_1900: bool) -> u64 {
    let mut secs = (v >> 32) & 0xffff_ffff;
    if minus_1900 && secs >= SECONDS_1900_TO_1970 {
        secs -= SECONDS_1900_TO_1970;
    }
    let frac = v & 0xffff_ffff;
    secs * SEC + ((frac * SEC) >> 32)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ntp_roundtrip() {
        let mut b = [0u8; 8];
        for ns in [0u64, 1, 999_999_999, 123_456_789_012, 4_000_000 * SEC + 5] {
            put_ntp(&mut b, ns);
            let back = get_ntp(&b);
            assert!(back.abs_diff(ns) <= 1, "{ns} -> {back}");
        }
    }
}
