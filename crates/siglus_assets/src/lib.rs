//! SiglusEngine asset helpers (format-first).
//!
//! This workspace snapshot focuses on:
//! - G00 graphics container parsing/decoding
//! - LZSS/LZSS32 decompressors used by SiglusEngine G00

pub mod g00;
pub mod lzss;

pub mod cgm;
pub mod dbs;
pub mod thumb_table;

pub mod angou;
pub mod gameexe;

pub mod scene_pck;

pub mod mpeg2;
pub mod nwa;
pub mod ogg_xor;
pub mod omv;
pub mod ovk;
pub mod vorbis;

mod util;
