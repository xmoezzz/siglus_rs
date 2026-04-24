//! Runtime recorder for unmapped numeric codes, names, and load-time notes.
//!
//! During reverse-engineering and incremental porting, it is common to encounter
//! numeric form codes that are not mapped to a known handler yet.
//! We also keep non-fatal load-time notes here so the runtime can continue.

use super::opcode::OpCode;
use std::collections::BTreeMap;

#[derive(Debug, Default)]
pub struct UnknownOpRecorder {
    /// Count unknown numeric codes.
    pub codes: BTreeMap<OpCode, u64>,
    /// Count unknown named commands.
    pub names: BTreeMap<String, u64>,
    /// Count non-fatal runtime or load-time notes.
    pub notes: BTreeMap<String, u64>,

    /// Observed element-chain signatures (e.g. "135:-1:0:2:-1:1:38").
    ///
    /// This is specifically useful when the project is missing a complete set
    /// of element / command id constants.
    pub element_chains: BTreeMap<String, u64>,
}

impl UnknownOpRecorder {
    pub fn record_code(&mut self, code: OpCode) {
        *self.codes.entry(code).or_insert(0) += 1;
    }

    pub fn record_name(&mut self, name: &str) {
        *self.names.entry(name.to_string()).or_insert(0) += 1;
    }

    pub fn record_note(&mut self, tag: &str) {
        *self.notes.entry(tag.to_string()).or_insert(0) += 1;
    }

    pub fn record_element_chain(&mut self, form_id: u32, chain: &[i32], note: &str) {
        let mut s = String::new();
        s.push_str(&form_id.to_string());
        for c in chain {
            s.push(':');
            s.push_str(&c.to_string());
        }
        if !note.is_empty() {
            s.push_str(" @");
            s.push_str(note);
        }
        *self.element_chains.entry(s).or_insert(0) += 1;
    }

    /// A compact human-readable summary.
    pub fn summary_string(&self, max_items: usize) -> String {
        let mut out = String::new();

        if !self.codes.is_empty() {
            out.push_str("unknown numeric codes:\n");
            for (i, (k, v)) in self.codes.iter().enumerate() {
                if i >= max_items {
                    out.push_str("  ...\n");
                    break;
                }
                out.push_str(&format!("  form {} x{}\n", k.id, v));
            }
        }

        if !self.notes.is_empty() {
            out.push_str("runtime notes:\n");
            for (i, (k, v)) in self.notes.iter().enumerate() {
                if i >= max_items {
                    out.push_str("  ...\n");
                    break;
                }
                out.push_str(&format!("  {k} x{v}\n"));
            }
        }

        if !self.names.is_empty() {
            out.push_str("unknown named commands:\n");
            for (i, (k, v)) in self.names.iter().enumerate() {
                if i >= max_items {
                    out.push_str("  ...\n");
                    break;
                }
                out.push_str(&format!("  {k} x{v}\n"));
            }
        }

        if !self.element_chains.is_empty() {
            out.push_str("unknown element chains:\n");
            for (i, (k, v)) in self.element_chains.iter().enumerate() {
                if i >= max_items {
                    out.push_str("  ...\n");
                    break;
                }
                out.push_str(&format!("  {k} x{v}\n"));
            }
        }

        if out.is_empty() {
            out.push_str("(no unknown ops recorded)\n");
        }

        out
    }
}
