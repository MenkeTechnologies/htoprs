//! Port of `linux/LibSensors.c`.
//!
//! The whole C file lives under `#ifdef HAVE_SENSORS_SENSORS_H` and talks to
//! the external `libsensors` C library (either linked with `BUILD_STATIC` or
//! `dlopen`ed at runtime). The rest of the crate has committed to the
//! *no-sensors* build variant — `CPUData` omits its `#ifdef` `temperature`
//! field and `LinuxMachine` drops the `LibSensors_reload()` reload clause (see
//! `linuxmachine.rs` module docs). Consequently every function that touches
//! the `sensors_*` FFI types (`sensors_chip_name`, `sensors_feature`,
//! `sensors_subfeature`, and the `sym_sensors_*` symbols) — or the omitted
//! `CPUData::temperature` field — cannot be faithfully ported yet and stays a
//! documented `todo!()` naming its missing dependency.
//!
//! `LibSensors_stringToID` is pure (no libsensors dependency) and is ported.
#![allow(non_snake_case)]
#![allow(dead_code)]

/// TODO: port of `int LibSensors_init(void)` from `LibSensors.c:55`.
pub fn LibSensors_init() {
    todo!("port of LibSensors.c:55: needs libsensors FFI (dlopen handle + sym_sensors_init/cleanup/get_* symbols), no-sensors build variant")
}

/// TODO: port of `void LibSensors_cleanup(void)` from `LibSensors.c:106`.
pub fn LibSensors_cleanup() {
    todo!("port of LibSensors.c:106: needs libsensors FFI (dlopen handle + sym_sensors_cleanup), no-sensors build variant")
}

/// TODO: port of `int LibSensors_reload(void)` from `LibSensors.c:123`.
pub fn LibSensors_reload() {
    todo!("port of LibSensors.c:123: needs libsensors FFI (dlopen handle + sym_sensors_cleanup/init), no-sensors build variant")
}

/// TODO: port of `static int tempDriverPriority(const sensors_chip_name* chip)`
/// from `LibSensors.c:135`.
pub fn tempDriverPriority() {
    todo!("port of LibSensors.c:135: needs libsensors FFI type sensors_chip_name (chip->prefix)")
}

/// TODO: port of `int LibSensors_countCCDs(void)` from `LibSensors.c:177`.
pub fn LibSensors_countCCDs() {
    todo!("port of LibSensors.c:177: needs libsensors FFI (sensors_chip_name/sensors_feature + sym_sensors_get_detected_chips/get_features/get_label)")
}

/// Port of `static int LibSensors_stringToID(const char* str)` from
/// `LibSensors.c:209`.
///
/// Mirrors `strtoul(str, &endptr, 10)` followed by the C guard
/// `if (parsedID >= INT_MAX || *endptr != '\0') return -1;`. `strtoul` skips
/// leading whitespace and an optional sign, consumes the decimal digit run,
/// and points `endptr` at the first unconsumed byte (or at `str` itself when
/// no digits were converted); the guard therefore rejects any input that is
/// not a pure decimal number filling the whole string, or that reaches
/// `INT_MAX`. Overflow saturates to `u64::MAX` (matching `strtoul`'s
/// `ULONG_MAX`), so it is likewise rejected by the `>= INT_MAX` test.
fn LibSensors_stringToID(str: &str) -> i32 {
    let bytes = str.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    // strtoul() skips leading whitespace.
    while i < len && bytes[i].is_ascii_whitespace() {
        i += 1;
    }

    // strtoul() accepts an optional leading sign.
    let mut negate = false;
    if i < len && (bytes[i] == b'+' || bytes[i] == b'-') {
        negate = bytes[i] == b'-';
        i += 1;
    }

    let digitStart = i;
    let mut parsedID: u64 = 0;
    while i < len && bytes[i].is_ascii_digit() {
        parsedID = parsedID
            .saturating_mul(10)
            .saturating_add((bytes[i] - b'0') as u64);
        i += 1;
    }

    // POSIX: when no digits are converted, strtoul returns 0 and stores the
    // original `nptr` in `endptr`; otherwise `endptr` is the first unconsumed
    // byte. `*endptr != '\0'` holds iff endptr is not at the terminating NUL.
    let endptr = if i == digitStart { 0 } else { i };
    let value = if negate {
        0u64.wrapping_sub(parsedID)
    } else {
        parsedID
    };

    if value >= i32::MAX as u64 || endptr < len {
        return -1;
    }
    value as i32
}

/// TODO: port of `void LibSensors_getCPUTemperatures(CPUData* cpus, unsigned int
/// existingCPUs, unsigned int activeCPUs)` from `LibSensors.c:217`.
pub fn LibSensors_getCPUTemperatures() {
    todo!("port of LibSensors.c:217: needs libsensors FFI (sensors_chip_name/feature/subfeature + sym_sensors_*) and the omitted CPUData::temperature field (no-sensors build variant)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn LibSensors_stringToID_parses_plain_decimal() {
        assert_eq!(LibSensors_stringToID("0"), 0);
        assert_eq!(LibSensors_stringToID("7"), 7);
        assert_eq!(LibSensors_stringToID("12"), 12);
    }

    #[test]
    fn LibSensors_stringToID_rejects_trailing_garbage() {
        assert_eq!(LibSensors_stringToID("12abc"), -1);
        assert_eq!(LibSensors_stringToID("1 "), -1);
        assert_eq!(LibSensors_stringToID("abc"), -1);
    }

    #[test]
    fn LibSensors_stringToID_rejects_at_or_above_int_max() {
        // INT_MAX itself is rejected (>=).
        assert_eq!(LibSensors_stringToID(&i32::MAX.to_string()), -1);
        assert_eq!(
            LibSensors_stringToID(&(i32::MAX as i64 - 1).to_string()),
            i32::MAX - 1
        );
        // Overflow saturates to u64::MAX and is rejected.
        assert_eq!(LibSensors_stringToID("999999999999999999999999"), -1);
    }

    #[test]
    fn LibSensors_stringToID_empty_string_is_zero() {
        // strtoul("") -> 0 with endptr at the NUL, so the guard passes.
        assert_eq!(LibSensors_stringToID(""), 0);
    }

    #[test]
    fn LibSensors_stringToID_negative_is_rejected() {
        // strtoul negates modulo 2^64 -> huge value -> rejected by >= INT_MAX.
        assert_eq!(LibSensors_stringToID("-5"), -1);
    }
}
