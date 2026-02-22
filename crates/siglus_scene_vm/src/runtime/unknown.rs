//! Unknown opcode/name recorder.
//!
//! During reverse-engineering and incremental porting, it is common to encounter
//! numeric form/syscall codes that are not mapped to a known handler yet.
//!
//! In addition, we may recognize a form/syscall but not have implemented every
//! sub-op. In that case we record an "unimplemented" tag so we can drive the
//! next RE/porting iteration.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use super::opcode::{OpCode, OpKind};

#[derive(Debug, Default)]
pub struct UnknownOpRecorder {
    /// Count unknown numeric codes.
    pub codes: BTreeMap<OpCode, u64>,
    /// Count unknown named commands.
    pub names: BTreeMap<String, u64>,
    /// Count recognized-but-unimplemented sub-operations.
    pub unimplemented: BTreeMap<String, u64>,

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

    pub fn record_unimplemented(&mut self, tag: &str) {
        *self.unimplemented.entry(tag.to_string()).or_insert(0) += 1;
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
                let kind = match k.kind {
                    OpKind::Syscall => "syscall",
                    OpKind::Form => "form",
                };
                out.push_str(&format!("  {kind} {} x{}\n", k.id, v));
            }
        }

        if !self.unimplemented.is_empty() {
            out.push_str("unimplemented ops:\n");
            for (i, (k, v)) in self.unimplemented.iter().enumerate() {
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

        if out.is_empty() {
            out.push_str("(no unknown ops recorded)\n");
        }

        out
    }

    pub fn write_report<P: AsRef<Path>>(&self, path: P) -> std::io::Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let mut f = std::fs::File::create(path)?;
        f.write_all(self.summary_string(2048).as_bytes())?;
        Ok(())
    }
}
