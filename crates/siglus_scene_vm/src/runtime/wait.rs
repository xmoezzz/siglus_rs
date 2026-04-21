//! VM wait/blocking state.
//!
//! The original engine has many commands/forms that block execution until:
//! - a certain time passes, or
//! - the user presses a key / clicks.
//!
//! Cross-platform blocking and wait model.

use std::time::{Duration, Instant};

use crate::audio::{BgmEngine, PcmEngine, SeEngine};

use super::constants::RuntimeConstants;
use super::globals::{GlobalState, ObjectState};
use super::Value;

#[derive(Debug, Clone, Copy)]
pub enum AudioWait {
    Bgm,
    BgmFade,
    SeAny,
    PcmAny,
    PcmSlot(u8),
}

#[derive(Debug, Clone)]
pub enum EventWait {
    ObjectAll {
        stage_form_id: u32,
        stage_idx: i64,
        runtime_slot: usize,
    },
    ObjectOne {
        stage_form_id: u32,
        stage_idx: i64,
        runtime_slot: usize,
        op: i32,
    },
    ObjectList {
        stage_form_id: u32,
        stage_idx: i64,
        runtime_slot: usize,
        list_op: i32,
        list_idx: usize,
    },
    GenericIntEvent {
        form_id: u32,
        index: Option<usize>,
    },
    CounterThreshold {
        form_id: u32,
        index: usize,
        target: i64,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct MovieWait {
    pub stage_form_id: u32,
    pub stage_idx: i64,
    pub runtime_slot: usize,
    pub return_value_flag: bool,
}

fn object_runtime_slot(idx: usize, obj: &ObjectState) -> usize {
    obj.runtime_slot_or(idx)
}

fn find_object_by_runtime_slot<'a>(objects: &'a [ObjectState], runtime_slot: usize) -> Option<&'a ObjectState> {
    for (idx, obj) in objects.iter().enumerate() {
        if object_runtime_slot(idx, obj) == runtime_slot {
            return Some(obj);
        }
        if let Some(found) = find_object_by_runtime_slot(&obj.runtime.child_objects, runtime_slot) {
            return Some(found);
        }
    }
    None
}

fn find_object_by_runtime_slot_mut<'a>(
    mut objects: &'a mut [ObjectState],
    runtime_slot: usize,
) -> Option<&'a mut ObjectState> {
    let mut idx = 0usize;
    while let Some((obj, tail)) = objects.split_first_mut() {
        if object_runtime_slot(idx, obj) == runtime_slot {
            return Some(obj);
        }
        if let Some(found) = find_object_by_runtime_slot_mut(&mut obj.runtime.child_objects, runtime_slot) {
            return Some(found);
        }
        objects = tail;
        idx += 1;
    }
    None
}

fn object_active_by_runtime_slot(
    globals: &GlobalState,
    stage_form_id: u32,
    stage_idx: i64,
    runtime_slot: usize,
) -> Option<&ObjectState> {
    globals
        .stage_forms
        .get(&stage_form_id)
        .and_then(|st| st.object_lists.get(&stage_idx))
        .and_then(|list| find_object_by_runtime_slot(list, runtime_slot))
}

fn object_active_by_runtime_slot_mut(
    globals: &mut GlobalState,
    stage_form_id: u32,
    stage_idx: i64,
    runtime_slot: usize,
) -> Option<&mut ObjectState> {
    globals
        .stage_forms
        .get_mut(&stage_form_id)
        .and_then(|st| st.object_lists.get_mut(&stage_idx))
        .and_then(|list| find_object_by_runtime_slot_mut(list, runtime_slot))
}

fn finish_event_wait_by_key(w: &EventWait, globals: &mut GlobalState, ids: &RuntimeConstants) {
    match w {
        EventWait::ObjectAll {
            stage_form_id,
            stage_idx,
            runtime_slot,
        } => {
            if let Some(obj) = object_active_by_runtime_slot_mut(
                globals,
                *stage_form_id,
                *stage_idx,
                *runtime_slot,
            ) {
                obj.end_all_events();
            }
        }
        EventWait::ObjectOne {
            stage_form_id,
            stage_idx,
            runtime_slot,
            op,
        } => {
            if let Some(obj) = object_active_by_runtime_slot_mut(
                globals,
                *stage_form_id,
                *stage_idx,
                *runtime_slot,
            ) {
                if let Some(ev) = obj.int_event_by_op_mut(ids, *op) {
                    ev.end_event();
                }
            }
        }
        EventWait::ObjectList {
            stage_form_id,
            stage_idx,
            runtime_slot,
            list_op,
            list_idx,
        } => {
            if let Some(obj) = object_active_by_runtime_slot_mut(
                globals,
                *stage_form_id,
                *stage_idx,
                *runtime_slot,
            ) {
                if let Some(ev) = obj
                    .int_event_list_by_op_mut(ids, *list_op)
                    .and_then(|v| v.get_mut(*list_idx))
                {
                    ev.end_event();
                }
            }
        }
        EventWait::GenericIntEvent { form_id, index } => match index {
            Some(i) => {
                if let Some(ev) = globals
                    .int_event_lists
                    .get_mut(form_id)
                    .and_then(|v| v.get_mut(*i))
                {
                    ev.end_event();
                }
            }
            None => {
                if let Some(ev) = globals.int_event_roots.get_mut(form_id) {
                    ev.end_event();
                }
            }
        },
        EventWait::CounterThreshold { .. } => {}
    }
}

#[derive(Debug, Default, Clone)]
pub struct VmWait {
    until: Option<Instant>,
    until_frame: Option<u64>,
    waiting_for_key: bool,
    /// If set, a key press cancels the current time wait (TIMEWAIT_KEY behavior).
    skip_time_on_key: bool,

    audio: Option<AudioWait>,
    audio_return_value: bool,

    event: Option<EventWait>,
    event_key_skip: bool,
    event_return_value: bool,

    movie: Option<MovieWait>,
    movie_key_skip: bool,

    movie_skip_info: Option<MovieWait>,
    pending_value: Option<Value>,

    /// Blocks VM execution until a runtime modal UI supplies a return value.
    system_modal: bool,

    wipe: bool,
    wipe_key_skip: bool,
}

impl VmWait {
    pub fn poll(
        &mut self,
        stack: &mut Vec<Value>,
        bgm: &mut BgmEngine,
        se: &mut SeEngine,
        pcm: &mut PcmEngine,
        globals: &mut GlobalState,
        ids: &RuntimeConstants,
    ) -> bool {
        let blocked = self.is_blocked(bgm, se, pcm, globals, ids);
        if !blocked {
            if let Some(v) = self.pending_value.take() {
                stack.push(v);
            }
        }
        blocked
    }

    pub fn is_blocked(
        &mut self,
        bgm: &mut BgmEngine,
        se: &mut SeEngine,
        pcm: &mut PcmEngine,
        globals: &mut GlobalState,
        ids: &RuntimeConstants,
    ) -> bool {
        // Auto-clear time waits when the deadline is reached.
        if let Some(t) = self.until {
            if Instant::now() >= t {
                self.until = None;
                self.skip_time_on_key = false;
            }
        }

        if let Some(frame) = self.until_frame {
            if globals.render_frame >= frame {
                self.until_frame = None;
            }
        }

        // Auto-clear audio waits when the predicate is satisfied.
        if let Some(w) = self.audio {
            let done = match w {
                AudioWait::Bgm => !bgm.is_playing(),
                AudioWait::BgmFade => !bgm.is_fade_out_doing(),
                AudioWait::SeAny => !se.is_playing_any(),
                AudioWait::PcmAny => !pcm.is_playing_any(),
                AudioWait::PcmSlot(s) => !pcm.is_playing_slot(s as usize),
            };
            if done {
                self.audio = None;
                if self.audio_return_value {
                    self.pending_value = Some(Value::Int(0));
                }
                self.audio_return_value = false;
            }
        }

        // Auto-clear event waits when the predicate is satisfied.
        let event_done = if let Some(w) = self.event.as_ref() {
            match w {
                EventWait::ObjectAll {
                    stage_form_id,
                    stage_idx,
                    runtime_slot,
                } => object_active_by_runtime_slot(globals, *stage_form_id, *stage_idx, *runtime_slot)
                    .map(|obj| !obj.used || !obj.any_event_active())
                    .unwrap_or(true),
                EventWait::ObjectOne {
                    stage_form_id,
                    stage_idx,
                    runtime_slot,
                    op,
                } => object_active_by_runtime_slot(globals, *stage_form_id, *stage_idx, *runtime_slot)
                    .map(|obj| {
                        !obj.used
                            || !obj
                                .int_event_by_op(ids, *op)
                                .map(|e| e.check_event())
                                .unwrap_or(false)
                    })
                    .unwrap_or(true),
                EventWait::ObjectList {
                    stage_form_id,
                    stage_idx,
                    runtime_slot,
                    list_op,
                    list_idx,
                } => object_active_by_runtime_slot(globals, *stage_form_id, *stage_idx, *runtime_slot)
                    .map(|obj| {
                        let active = obj
                            .int_event_list_by_op(ids, *list_op)
                            .and_then(|v| v.get(*list_idx))
                            .map(|e| e.check_event())
                            .unwrap_or(false);
                        !obj.used || !active
                    })
                    .unwrap_or(true),
                EventWait::GenericIntEvent { form_id, index } => match index {
                    Some(i) => globals
                        .int_event_lists
                        .get(form_id)
                        .and_then(|v| v.get(*i))
                        .map(|e| !e.check_event())
                        .unwrap_or(true),
                    None => globals
                        .int_event_roots
                        .get(form_id)
                        .map(|e| !e.check_event())
                        .unwrap_or(true),
                },
                EventWait::CounterThreshold {
                    form_id,
                    index,
                    target,
                } => globals
                    .counter_lists
                    .get(form_id)
                    .and_then(|v| v.get(*index))
                    .map(|c| c.get_count() - *target >= 0)
                    .unwrap_or(true),
            }
        } else {
            false
        };
        if event_done {
            self.event = None;
            self.event_key_skip = false;
            if self.event_return_value {
                self.pending_value = Some(Value::Int(0));
            }
            self.event_return_value = false;
        }

        // Auto-clear movie waits when playback ends.
        if let Some(w) = self.movie {
            let done = object_active_by_runtime_slot(
                globals,
                w.stage_form_id,
                w.stage_idx,
                w.runtime_slot,
            )
            .map(|obj| !obj.used || !obj.movie.check_movie())
            .unwrap_or(true);

            if done {
                if w.return_value_flag {
                    self.pending_value = Some(Value::Int(0));
                }
                self.movie = None;
                self.movie_key_skip = false;
            }
        }

        // Auto-clear wipe waits when the wipe is finished.
        if self.wipe {
            if globals.wipe_done() {
                self.wipe = false;
                self.wipe_key_skip = false;
            }
        }

        self.waiting_for_key
            || self.until.is_some()
            || self.until_frame.is_some()
            || self.audio.is_some()
            || self.event.is_some()
            || self.movie.is_some()
            || self.system_modal
            || self.wipe
    }

    pub fn wait_system_modal(&mut self) {
        self.system_modal = true;
    }

    pub fn finish_system_modal(&mut self, value: Value) {
        if self.system_modal {
            self.system_modal = false;
            self.pending_value = Some(value);
        }
    }

    pub fn system_modal_active(&self) -> bool {
        self.system_modal
    }

    pub fn wait_ms(&mut self, ms: u64) {
        if ms == 0 {
            return;
        }
        self.until = Some(Instant::now() + Duration::from_millis(ms));
        self.skip_time_on_key = false;
    }

    pub fn wait_next_frame(&mut self, current_frame: u64) {
        self.until_frame = Some(current_frame.saturating_add(1));
        self.skip_time_on_key = false;
    }

    /// Wait for a duration, but allow any key/mouse press to cancel the wait.
    pub fn wait_ms_key(&mut self, ms: u64) {
        if ms == 0 {
            return;
        }
        self.until = Some(Instant::now() + Duration::from_millis(ms));
        self.skip_time_on_key = true;
    }

    pub fn wait_key(&mut self) {
        self.waiting_for_key = true;
    }

    pub fn wait_audio(&mut self, w: AudioWait, key: bool) {
        self.wait_audio_with_return(w, key, false);
    }

    pub fn wait_audio_with_return(&mut self, w: AudioWait, key: bool, return_value_flag: bool) {
        self.audio = Some(w);
        self.audio_return_value = return_value_flag;
        if key {
            self.waiting_for_key = true;
        }
    }

    pub fn wait_object_all_events(
        &mut self,
        stage_form_id: u32,
        stage_idx: i64,
        runtime_slot: usize,
        key_skip: bool,
    ) {
        self.event = Some(EventWait::ObjectAll {
            stage_form_id,
            stage_idx,
            runtime_slot,
        });
        self.event_key_skip = key_skip;
        self.event_return_value = false;
        if key_skip {
            self.waiting_for_key = true;
        }
    }

    pub fn wait_object_event(
        &mut self,
        stage_form_id: u32,
        stage_idx: i64,
        runtime_slot: usize,
        op: i32,
        key_skip: bool,
        return_value_flag: bool,
    ) {
        self.event = Some(EventWait::ObjectOne {
            stage_form_id,
            stage_idx,
            runtime_slot,
            op,
        });
        self.event_key_skip = key_skip;
        self.event_return_value = return_value_flag;
        if key_skip {
            self.waiting_for_key = true;
        }
    }

    pub fn wait_object_event_list(
        &mut self,
        stage_form_id: u32,
        stage_idx: i64,
        runtime_slot: usize,
        list_op: i32,
        list_idx: usize,
        key_skip: bool,
        return_value_flag: bool,
    ) {
        self.event = Some(EventWait::ObjectList {
            stage_form_id,
            stage_idx,
            runtime_slot,
            list_op,
            list_idx,
        });
        self.event_key_skip = key_skip;
        self.event_return_value = return_value_flag;
        if key_skip {
            self.waiting_for_key = true;
        }
    }

    pub fn wait_object_movie(
        &mut self,
        stage_form_id: u32,
        stage_idx: i64,
        runtime_slot: usize,
        key_skip: bool,
        return_value_flag: bool,
    ) {
        self.movie = Some(MovieWait {
            stage_form_id,
            stage_idx,
            runtime_slot,
            return_value_flag,
        });
        self.movie_key_skip = key_skip;
        if key_skip {
            self.waiting_for_key = true;
        }
    }

    pub fn wait_generic_int_event(
        &mut self,
        form_id: u32,
        index: Option<usize>,
        key_skip: bool,
        return_value_flag: bool,
    ) {
        self.event = Some(EventWait::GenericIntEvent { form_id, index });
        self.event_key_skip = key_skip;
        self.event_return_value = return_value_flag;
        if key_skip {
            self.waiting_for_key = true;
        }
    }

    pub fn wait_counter(
        &mut self,
        form_id: u32,
        index: usize,
        target: i64,
        key_skip: bool,
        return_value_flag: bool,
    ) {
        self.event = Some(EventWait::CounterThreshold {
            form_id,
            index,
            target,
        });
        self.event_key_skip = key_skip;
        self.event_return_value = return_value_flag;
        if key_skip {
            self.waiting_for_key = true;
        }
    }

    pub fn wait_wipe(&mut self, key_skip: bool) {
        self.wipe = true;
        self.wipe_key_skip = key_skip;
        if key_skip {
            self.waiting_for_key = true;
        }
    }

    /// Notify the wait system that a key/mouse input happened.
    ///
    /// Returns true if the input is interpreted as a wipe-skip (used by WIPE/WAIT_WIPE).
    pub fn notify_key(&mut self, globals: &mut GlobalState, ids: &RuntimeConstants) -> bool {
        let wipe_skipped = self.wipe && self.wipe_key_skip;
        self.waiting_for_key = false;
        if self.audio.is_some() && self.audio_return_value {
            self.pending_value = Some(Value::Int(1));
        }
        self.audio = None;
        self.audio_return_value = false;
        if self.event_key_skip {
            if let Some(w) = self.event.take() {
                finish_event_wait_by_key(&w, globals, ids);
                if self.event_return_value {
                    self.pending_value = Some(Value::Int(1));
                }
            }
            self.event_key_skip = false;
            self.event_return_value = false;
        }
        if self.movie_key_skip {
            if let Some(w) = self.movie.take() {
                if w.return_value_flag {
                    self.pending_value = Some(Value::Int(1));
                }
                self.movie_skip_info = Some(w);
            }
            self.movie_key_skip = false;
        }
        if self.skip_time_on_key {
            self.until = None;
            self.skip_time_on_key = false;
        }

        if wipe_skipped {
            self.wipe = false;
            self.wipe_key_skip = false;
        }

        wipe_skipped
    }

    /// If the current wait was skipped via key input, returns the skipped movie wait info.
    pub fn take_movie_skip(&mut self) -> Option<MovieWait> {
        self.movie_skip_info.take()
    }

    pub fn clear(&mut self) {
        self.until = None;
        self.waiting_for_key = false;
        self.skip_time_on_key = false;
        self.audio = None;
        self.audio_return_value = false;
        self.event = None;
        self.event_key_skip = false;
        self.event_return_value = false;
        self.movie = None;
        self.movie_key_skip = false;
        self.movie_skip_info = None;
        self.pending_value = None;
        self.system_modal = false;
        self.wipe = false;
        self.wipe_key_skip = false;
    }
}
