//! VM wait/blocking state.
//!
//! The original engine has many commands/forms that block execution until:
//! - a certain time passes, or
//! - the user presses a key / clicks.
//!
//! For bring-up, we implement a minimal, cross-platform blocking model.

use std::time::{Duration, Instant};

use crate::audio::{BgmEngine, PcmEngine, SeEngine};

use super::globals::GlobalState;
use super::Value;

#[derive(Debug, Clone, Copy)]
pub enum AudioWait {
    Bgm,
    SeAny,
    PcmAny,
    PcmSlot(u8),
}

#[derive(Debug, Clone)]
pub enum EventWait {
    ObjectAll {
        stage_form_id: u32,
        stage_idx: i64,
        obj_idx: usize,
    },
    ObjectOne {
        stage_form_id: u32,
        stage_idx: i64,
        obj_idx: usize,
        op: i32,
    },
    ObjectList {
        stage_form_id: u32,
        stage_idx: i64,
        obj_idx: usize,
        list_op: i32,
        list_idx: usize,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct MovieWait {
    pub stage_form_id: u32,
    pub stage_idx: i64,
    pub obj_idx: usize,
    pub return_value_flag: bool,
}

#[derive(Debug, Default, Clone)]
pub struct VmWait {
    until: Option<Instant>,
    waiting_for_key: bool,
    /// If set, a key press cancels the current time wait (TIMEWAIT_KEY behavior).
    skip_time_on_key: bool,

    audio: Option<AudioWait>,

    event: Option<EventWait>,
    event_key_skip: bool,

    movie: Option<MovieWait>,
    movie_key_skip: bool,

    movie_skip_info: Option<MovieWait>,
    pending_value: Option<Value>,

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
    ) -> bool {
        let blocked = self.is_blocked(bgm, se, pcm, globals);
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
    ) -> bool {
        // Auto-clear time waits when the deadline is reached.
        if let Some(t) = self.until {
            if Instant::now() >= t {
                self.until = None;
                self.skip_time_on_key = false;
            }
        }

        // Auto-clear audio waits when the predicate is satisfied.
        if let Some(w) = self.audio {
            let done = match w {
                AudioWait::Bgm => !bgm.is_playing(),
                AudioWait::SeAny => !se.is_playing_any(),
                AudioWait::PcmAny => !pcm.is_playing_any(),
                AudioWait::PcmSlot(s) => !pcm.is_playing_slot(s as usize),
            };
            if done {
                self.audio = None;
            }
        }

        // Auto-clear event waits when the predicate is satisfied.
        if let Some(w) = &self.event {
            let done = match w {
                EventWait::ObjectAll {
                    stage_form_id,
                    stage_idx,
                    obj_idx,
                } => match globals.stage_forms.get(stage_form_id) {
                    None => true,
                    Some(st) => match st.object_lists.get(stage_idx) {
                        None => true,
                        Some(list) => {
                            if *obj_idx >= list.len() {
                                true
                            } else {
                                !list[*obj_idx].any_event_active()
                            }
                        }
                    },
                },
                EventWait::ObjectOne {
                    stage_form_id,
                    stage_idx,
                    obj_idx,
                    op,
                } => match globals.stage_forms.get(stage_form_id) {
                    None => true,
                    Some(st) => match st.object_lists.get(stage_idx) {
                        None => true,
                        Some(list) => {
                            if *obj_idx >= list.len() {
                                true
                            } else {
                                let obj = &list[*obj_idx];
                                !obj
                                    .extra_events
                                    .get(op)
                                    .map(|e| e.check_event())
                                    .unwrap_or(false)
                            }
                        }
                    },
                },
                EventWait::ObjectList {
                    stage_form_id,
                    stage_idx,
                    obj_idx,
                    list_op,
                    list_idx,
                } => match globals.stage_forms.get(stage_form_id) {
                    None => true,
                    Some(st) => match st.object_lists.get(stage_idx) {
                        None => true,
                        Some(list) => {
                            if *obj_idx >= list.len() {
                                true
                            } else {
                                let obj = &list[*obj_idx];
                                let active = obj
                                    .rep_int_event_lists
                                    .get(list_op)
                                    .and_then(|v| v.get(*list_idx))
                                    .map(|e| e.check_event())
                                    .unwrap_or(false);
                                !active
                            }
                        }
                    },
                },
            };
            if done {
                self.event = None;
                self.event_key_skip = false;
            }
        }

        // Auto-clear movie waits when playback ends.
        if let Some(w) = self.movie {
            let done = match globals.stage_forms.get(&w.stage_form_id) {
                None => true,
                Some(st) => match st.object_lists.get(&w.stage_idx) {
                    None => true,
                    Some(list) => {
                        if w.obj_idx >= list.len() {
                            true
                        } else {
                            !list[w.obj_idx].movie.check_movie()
                        }
                    }
                },
            };

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
            || self.audio.is_some()
            || self.event.is_some()
            || self.movie.is_some()
            || self.wipe
    }

    pub fn wait_ms(&mut self, ms: u64) {
        if ms == 0 {
            return;
        }
        self.until = Some(Instant::now() + Duration::from_millis(ms));
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
        self.audio = Some(w);
        if key {
            self.waiting_for_key = true;
        }
    }

    pub fn wait_object_all_events(
        &mut self,
        stage_form_id: u32,
        stage_idx: i64,
        obj_idx: usize,
        key_skip: bool,
    ) {
        self.event = Some(EventWait::ObjectAll {
            stage_form_id,
            stage_idx,
            obj_idx,
        });
        self.event_key_skip = key_skip;
        if key_skip {
            self.waiting_for_key = true;
        }
    }

    pub fn wait_object_event(
        &mut self,
        stage_form_id: u32,
        stage_idx: i64,
        obj_idx: usize,
        op: i32,
        key_skip: bool,
    ) {
        self.event = Some(EventWait::ObjectOne {
            stage_form_id,
            stage_idx,
            obj_idx,
            op,
        });
        self.event_key_skip = key_skip;
        if key_skip {
            self.waiting_for_key = true;
        }
    }

    pub fn wait_object_event_list(
        &mut self,
        stage_form_id: u32,
        stage_idx: i64,
        obj_idx: usize,
        list_op: i32,
        list_idx: usize,
        key_skip: bool,
    ) {
        self.event = Some(EventWait::ObjectList {
            stage_form_id,
            stage_idx,
            obj_idx,
            list_op,
            list_idx,
        });
        self.event_key_skip = key_skip;
        if key_skip {
            self.waiting_for_key = true;
        }
    }

    pub fn wait_object_movie(
        &mut self,
        stage_form_id: u32,
        stage_idx: i64,
        obj_idx: usize,
        key_skip: bool,
        return_value_flag: bool,
    ) {
        self.movie = Some(MovieWait {
            stage_form_id,
            stage_idx,
            obj_idx,
            return_value_flag,
        });
        self.movie_key_skip = key_skip;
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
    pub fn notify_key(&mut self) -> bool {
        let wipe_skipped = self.wipe && self.wipe_key_skip;
        self.waiting_for_key = false;
        self.audio = None;
        if self.event_key_skip {
            self.event = None;
            self.event_key_skip = false;
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
        self.event = None;
        self.event_key_skip = false;
        self.movie = None;
        self.movie_key_skip = false;
        self.movie_skip_info = None;
        self.pending_value = None;
        self.wipe = false;
        self.wipe_key_skip = false;
    }
}
