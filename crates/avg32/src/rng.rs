//! Native AVG32 random state shared by platform and portable hosts.
pub fn next_random(state: &mut u64) -> u32 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (z ^ (z >> 31)) as u32
}

/// Inclusive range.
pub fn random(state: &mut u64, low: i32, high: i32) -> i32 {
    let (low, high) = if low <= high {
        (low, high)
    } else {
        (high, low)
    };
    let span = (i64::from(high) - i64::from(low) + 1).max(1) as u64;
    low + (u64::from(next_random(state)) % span) as i32
}
