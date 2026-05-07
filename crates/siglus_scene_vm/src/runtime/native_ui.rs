//! Platform UI bridge for runtime modal operations.
//!
//! The VM must not synchronously block on platform UI. Mobile ports should show
//! the native dialog on the platform UI thread, then deliver the selected button
//! back through `CommandContext::submit_native_messagebox_result` from the main
//! engine loop or an event-loop callback.

use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeMessageBoxKind {
    Ok,
    OkCancel,
    YesNo,
    YesNoCancel,
}

impl NativeMessageBoxKind {
    pub fn from_system_op(op: i32) -> Self {
        match op {
            18 | 8 => Self::OkCancel,
            19 | 9 => Self::YesNo,
            20 | 10 => Self::YesNoCancel,
            _ => Self::Ok,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NativeMessageBoxButton {
    pub label: String,
    pub value: i64,
}

#[derive(Debug, Clone)]
pub struct NativeMessageBoxRequest {
    pub request_id: u64,
    pub kind: NativeMessageBoxKind,
    pub title: String,
    pub message: String,
    pub buttons: Vec<NativeMessageBoxButton>,
    pub debug_only: bool,
}

pub trait NativeUiBackend: Send + Sync {
    fn show_system_messagebox(&self, request: NativeMessageBoxRequest);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeMessageBoxResult {
    pub request_id: u64,
    pub value: i64,
}

#[derive(Debug, Clone)]
pub struct NativeUiRuntime {
    next_messagebox_request_id: u64,
    messagebox_results: VecDeque<NativeMessageBoxResult>,
}

impl Default for NativeUiRuntime {
    fn default() -> Self {
        Self {
            next_messagebox_request_id: 1,
            messagebox_results: VecDeque::new(),
        }
    }
}

impl NativeUiRuntime {
    pub fn next_messagebox_request_id(&mut self) -> u64 {
        let id = self.next_messagebox_request_id;
        self.next_messagebox_request_id = self.next_messagebox_request_id.wrapping_add(1).max(1);
        id
    }

    pub fn enqueue_messagebox_result(&mut self, request_id: u64, value: i64) {
        self.messagebox_results
            .push_back(NativeMessageBoxResult { request_id, value });
    }

    pub fn pop_messagebox_result(&mut self) -> Option<NativeMessageBoxResult> {
        self.messagebox_results.pop_front()
    }
}
