//! Integer event (linear interpolation / loop / turn) used by a few engine subsystems.
//!
//! This follows the original Siglus behavior:
//! - `value` is the "current" stored value (for oneshot, it is set to the target immediately).
//! - `cur_value` is the animated/interpolated value computed each frame.

#[derive(Debug, Clone)]
pub struct IntEvent {
    pub def_value: i32,
    pub value: i32,
    pub cur_time: i32,
    pub end_time: i32,
    pub delay_time: i32,
    pub start_value: i32,
    pub cur_value: i32,
    pub end_value: i32,
    /// -1: none, 0: oneshot, 1: loop, 2: turn
    pub loop_type: i32,
    /// -1: none, 0: linear, 1: speed_up, 2: speed_down
    pub speed_type: i32,
    /// 0: game time, 1: real time
    pub real_flag: i32,
}

impl IntEvent {
    pub fn new(def_value: i32) -> Self {
        let mut s = Self {
            def_value,
            value: def_value,
            cur_time: 0,
            end_time: 0,
            delay_time: 0,
            start_value: def_value,
            cur_value: def_value,
            end_value: def_value,
            loop_type: -1,
            speed_type: -1,
            real_flag: 0,
        };
        s.reinit();
        s
    }

    pub fn reinit(&mut self) {
        self.value = self.def_value;
        self.cur_time = 0;
        self.end_time = 0;
        self.delay_time = 0;
        self.start_value = self.def_value;
        self.cur_value = self.def_value;
        self.end_value = self.def_value;
        self.loop_type = -1;
        self.speed_type = -1;
        self.real_flag = 0;
    }

    pub fn set_value(&mut self, v: i32) {
        self.value = v;
        // Note: the original does not touch the event fields here.
    }

    pub fn get_value(&self) -> i32 {
        self.value
    }

    pub fn get_total_value(&self) -> i32 {
        self.cur_value
    }

    pub fn set_event(
        &mut self,
        value: i32,
        total_time: i32,
        delay_time: i32,
        speed_type: i32,
        real_flag: i32,
    ) {
        self.cur_time = 0;
        self.end_time = total_time;
        self.delay_time = delay_time;
        self.start_value = self.value;
        self.cur_value = self.value;
        self.end_value = value;
        self.loop_type = 0; // oneshot
        self.speed_type = speed_type;
        self.real_flag = real_flag;

        // The original updates the stored value immediately.
        self.value = value;
    }

    pub fn loop_event(
        &mut self,
        start_value: i32,
        end_value: i32,
        loop_time: i32,
        delay_time: i32,
        speed_type: i32,
        real_flag: i32,
    ) {
        self.cur_time = 0;
        self.end_time = loop_time;
        self.delay_time = delay_time;
        self.start_value = start_value;
        self.cur_value = start_value;
        self.end_value = end_value;
        self.loop_type = 1; // loop
        self.speed_type = speed_type;
        self.real_flag = real_flag;
    }

    pub fn turn_event(
        &mut self,
        start_value: i32,
        end_value: i32,
        loop_time: i32,
        delay_time: i32,
        speed_type: i32,
        real_flag: i32,
    ) {
        self.cur_time = 0;
        self.end_time = loop_time;
        self.delay_time = delay_time;
        self.start_value = start_value;
        self.cur_value = start_value;
        self.end_value = end_value;
        self.loop_type = 2; // turn
        self.speed_type = speed_type;
        self.real_flag = real_flag;
    }

    pub fn end_event(&mut self) {
        self.loop_type = -1;
        self.cur_value = self.value;
    }

    pub fn check_event(&self) -> bool {
        self.loop_type != -1
    }

    pub fn update_time(&mut self, past_game_time: i32, past_real_time: i32) {
        if self.loop_type == -1 {
            return;
        }
        if self.real_flag == 0 {
            self.cur_time = self.cur_time.saturating_add(past_game_time);
        } else {
            self.cur_time = self.cur_time.saturating_add(past_real_time);
        }
    }

    pub fn frame(&mut self) {
        self.cur_value = self.value;
        if self.loop_type == -1 {
            return;
        }
        self.frame_sub();
    }

    fn frame_sub(&mut self) {
        let end_time = self.end_time;
        let start_value = self.start_value;
        let end_value = self.end_value;
        let mut cur_time = self.cur_time - self.delay_time;

        // oneshot: if time is over, stop the event.
        if self.loop_type == 0 {
            if cur_time - end_time >= 0 {
                self.loop_type = -1;
                return;
            }
        }

        // Not started yet.
        if cur_time <= 0 {
            self.cur_value = start_value;
            return;
        }

        if end_time <= 0 {
            // Avoid division by zero / modulo by zero.
            self.cur_value = end_value;
            return;
        }

        if self.loop_type == 1 {
            cur_time %= end_time;
        }

        if self.loop_type == 2 {
            cur_time %= end_time * 2;
            if cur_time - end_time > 0 {
                cur_time = end_time - (cur_time - end_time);
            }
        }

        match self.speed_type {
            0 => {
                self.cur_value = (((end_value - start_value) as f64) * (cur_time as f64)
                    / (end_time as f64)
                    + (start_value as f64)) as i32;
            }
            1 => {
                let ct = cur_time as f64;
                let et = end_time as f64;
                self.cur_value = (((end_value - start_value) as f64) * ct * ct / et / et
                    + (start_value as f64)) as i32;
            }
            2 => {
                let ct = (cur_time - end_time) as f64;
                let et = end_time as f64;
                self.cur_value = (-(end_value - start_value) as f64 * ct * ct / et / et
                    + (end_value as f64)) as i32;
            }
            _ => {
                // Unknown/none: keep the last computed value.
            }
        }
    }

    /// Convenience: advance by one "tick".
    pub fn tick(&mut self, delta: i32) {
        self.update_time(delta, delta);
        self.frame();
    }
}
