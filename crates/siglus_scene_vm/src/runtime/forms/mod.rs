//! Form (numeric command) dispatchers.
//!
//! In the original Siglus engine, a large set of operations are routed through a
//! single global switch ("forms"), and each form may further dispatch on a
//! sub-op ID.
//!
//! This module implements only the forms needed by the current runtime plan.

pub mod bgm;
pub mod bgm_table;
pub mod codes;
pub mod excall;
pub mod global;
pub mod input;
pub mod key;
pub mod keylist;
pub mod mov;
pub mod mouse;
pub mod pcm;
pub mod pcmch;
pub mod pcmevent;
pub mod koe_st;
pub mod screen;
pub mod msgbk;
pub mod se;
pub mod stage;
pub mod int_list;
pub mod str_list;
pub mod timewait;
pub mod counter;
pub mod prop_access;


// Runtime forms implemented from the original the original implementation source (IDs are configured externally).
pub mod math;
pub mod cgtable;
pub mod database;
pub mod g00buf;
pub mod file;
pub mod steam;
pub mod mask;
pub mod editbox;

pub mod syscom;
pub mod script;
pub mod system;
pub mod frame_action;
pub mod frame_action_ch;

use anyhow::Result;

use crate::runtime::{CommandContext, Value};

pub fn dispatch_form(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    global::dispatch_global_form(ctx, form_id, args)
}
