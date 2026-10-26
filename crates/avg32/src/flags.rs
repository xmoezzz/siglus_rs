//! AVG32 scenario variables.
//!
//! 2000 integer values, 2000 bits, 100 Shift-JIS strings of at most 63
//! bytes, and the unified gosub stack of `(scene, position)` pairs shared
//! by same-scene calls and cross-scene calls.  Out-of-range accesses read
//! as zero and writes are ignored, exactly like the original.

pub const VALUE_COUNT: usize = 2000;
pub const BIT_COUNT: usize = 2000;
pub const STRING_COUNT: usize = 100;
pub const STRING_BYTES: usize = 64;
pub const MAX_STACK: usize = 256;

#[cfg_attr(feature = "portable-vm", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flags {
    values: Vec<i32>,
    bits: Vec<bool>,
    strings: Vec<Vec<u8>>,
    stack: Vec<(i32, i32)>,
    saved_stack: Vec<(i32, i32)>,
}

impl Default for Flags {
    fn default() -> Self {
        Self {
            values: vec![0; VALUE_COUNT],
            bits: vec![false; BIT_COUNT],
            strings: vec![Vec::new(); STRING_COUNT],
            stack: Vec::new(),
            saved_stack: Vec::new(),
        }
    }
}

impl Flags {
    pub fn value(&self, index: i32) -> i32 {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.values.get(index))
            .copied()
            .unwrap_or(0)
    }

    pub fn set_value(&mut self, index: i32, value: i32) {
        if let Some(slot) = usize::try_from(index)
            .ok()
            .and_then(|index| self.values.get_mut(index))
        {
            *slot = value;
        }
    }

    pub fn bit(&self, index: i32) -> bool {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.bits.get(index))
            .copied()
            .unwrap_or(false)
    }

    pub fn set_bit(&mut self, index: i32, value: bool) {
        if let Some(slot) = usize::try_from(index)
            .ok()
            .and_then(|index| self.bits.get_mut(index))
        {
            *slot = value;
        }
    }

    pub fn string(&self, index: i32) -> &[u8] {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.strings.get(index))
            .map_or(&[], Vec::as_slice)
    }

    /// Stores at most 63 bytes, like `strncpy(str[i], s, 64); str[i][63] = 0`.
    pub fn set_string(&mut self, index: i32, value: &[u8]) {
        if let Some(slot) = usize::try_from(index)
            .ok()
            .and_then(|index| self.strings.get_mut(index))
        {
            let end = value
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(value.len());
            *slot = value[..end.min(STRING_BYTES - 1)].to_vec();
        }
    }

    pub fn push_stack(&mut self, seen: i32, position: i32) {
        if self.stack.len() < MAX_STACK {
            self.stack.push((seen, position));
        }
    }

    pub fn pop_stack(&mut self) -> Option<(i32, i32)> {
        self.stack.pop()
    }

    pub fn clear_stack(&mut self) {
        self.stack.clear();
    }

    pub fn stack(&self) -> &[(i32, i32)] {
        &self.stack
    }

    /// Snapshot taken at every save point.
    pub fn save_stack(&mut self) {
        self.saved_stack = self.stack.clone();
    }

    pub fn saved_stack(&self) -> &[(i32, i32)] {
        &self.saved_stack
    }

    pub fn restore_saved_stack(&mut self, stack: Vec<(i32, i32)>) {
        self.saved_stack = stack;
    }

    pub fn restore_stack(&mut self, stack: Vec<(i32, i32)>) {
        self.stack = stack.clone();
        self.saved_stack = stack;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_of_range_accesses_are_ignored() {
        let mut flags = Flags::default();
        flags.set_value(2000, 5);
        flags.set_value(-1, 5);
        assert_eq!(flags.value(2000), 0);
        flags.set_bit(1999, true);
        assert!(flags.bit(1999));
        flags.set_string(3, &[b'a'; 100]);
        assert_eq!(flags.string(3).len(), 63);
    }
}
