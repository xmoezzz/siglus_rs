//! The AVG32 scenario interpreter.
//!
//! Like the original, the interpreter is a cooperative per-frame loop: a
//! command that has to wait (a click, a timer, a transition, a choice …)
//! keeps itself as the current command and is re-entered on the next frame,
//! while every other command runs to completion immediately.  All graphics,
//! text, sound and input go through [`System`].

#[cfg(feature = "native-engine")]
use crate::ard::AreaMap;
#[cfg(feature = "native-engine")]
use crate::effect::Effect;
#[cfg(feature = "native-engine")]
use crate::movie::{AviMovie, MoviePlayer};
use crate::nls;
#[cfg(feature = "native-engine")]
use crate::pdtmgr::{HIDEPDT, Rect};
#[cfg(feature = "portable-vm")]
use crate::portable::System;
use crate::scene::{Avg32SceneHeader, parse_scene_value};
#[cfg(feature = "native-engine")]
use crate::system::System;

/// The multi-PDT scroller/slideshow of `0x6a`.
#[cfg(feature = "native-engine")]
#[derive(Debug, Clone, Default)]
struct Ending {
    active: bool,
    cmd: u8,
    align: u8,
    x: i32,
    wait: i32,
    pixel: i32,
    line: i32,
    total: i32,
    time: u64,
    cancel: i32,
    files: Vec<Vec<u8>>,
    sizes: Vec<(i32, i32)>,
    offsets: Vec<i32>,
    images: Vec<Option<crate::buffer::PdtBuffer>>,
}

#[cfg_attr(feature = "portable-vm", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Default)]
struct MenuState {
    cur: i32,
    cur_sub: i32,
    bit: u8,
    bit_count: u8,
    sub_bit: u8,
    sub_bit_count: u8,
    sub_count: [[u8; 8]; 8],
    start: i32,
    loopback: i32,
}

#[cfg_attr(feature = "portable-vm", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "portable-vm", derive(Clone))]
#[derive(Debug)]
pub struct Scenario {
    pub seen: i32,
    data: Vec<u8>,
    code_offset: usize,
    pub pos: i32,
    cmd: u8,
    header: Option<Avg32SceneHeader>,
    menu: MenuState,
    jump_base: i32,
    pub last_save_seen: i32,
    pub last_save_pos: i32,
    wait_cmd: u8,
    wait_base: u64,
    wait_time: u64,
    wait_cancel: i32,
    fade_cmd: u8,
    fade_count: i32,
    fade_step: u64,
    fade_prev: u64,
    fade_colour: [i32; 3],
    flash_base: u64,
    flash_time: u64,
    flash_colour: [i32; 3],
    flash_count: i32,
    mes_flag: i32,
    mes_wait_base: u64,
    #[cfg(feature = "native-engine")]
    area: Option<AreaMap>,
    #[cfg(feature = "native-engine")]
    area_enabled: [bool; 256],
    select_flag: u8,
    select_index: i32,
    shake_flag: bool,
    anm_flag: bool,
    multi_anm: i32,
    pub loading: bool,
    sound_wait: u8,
    #[cfg(feature = "native-engine")]
    ending: Ending,
    #[cfg(feature = "native-engine")]
    pub effect: Effect,
    auto_mode_count: i32,
    #[cfg(feature = "native-engine")]
    movie: Option<(MoviePlayer, bool)>,
    name_wait: bool,
    menu_wait: bool,
}

/// ASCII to full-width characters (in the NLS encoding).
pub fn han_to_zen(text: &[u8]) -> Vec<u8> {
    const TABLE: [u8; 0x60] = [
        0x40, 0x49, 0x68, 0x94, 0x90, 0x93, 0x95, 0x66, 0x69, 0x6a, 0x96, 0x7b, 0x43, 0x7c, 0x44,
        0x5e, 0x4f, 0x50, 0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x46, 0x47, 0x83, 0x81,
        0x84, 0x48, 0x97, 0x60, 0x61, 0x62, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x6b,
        0x6c, 0x6d, 0x6e, 0x6f, 0x70, 0x71, 0x72, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x6d,
        0x8f, 0x6e, 0x4f, 0x51, 0x65, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a,
        0x8b, 0x8c, 0x8d, 0x8e, 0x8f, 0x90, 0x91, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99,
        0x9a, 0x6f, 0x62, 0x70, 0x60, 0x45,
    ];
    let mut out = Vec::with_capacity(text.len() * 2);
    for &c in text {
        if c == 0 {
            break;
        }
        if (b' '..0x80).contains(&c) {
            out.push(if c.is_ascii_alphanumeric() {
                0x82
            } else {
                0x81
            });
            out.push(TABLE[usize::from(c - 0x20)]);
            if out.len() > 61 {
                break;
            }
        }
    }
    nls::from_sjis(&out)
}

/// C `strcmp` over Shift-JIS bytes.
fn strcmp(left: &[u8], right: &[u8]) -> i32 {
    for (a, b) in left
        .iter()
        .chain(std::iter::once(&0))
        .zip(right.iter().chain(std::iter::once(&0)))
    {
        if a != b || *a == 0 {
            return i32::from(*a) - i32::from(*b);
        }
    }
    0
}

impl Scenario {
    pub fn new(sys: &mut System, seen: i32) -> Self {
        let mut scenario = Self {
            seen: -1,
            data: Vec::new(),
            code_offset: 0,
            pos: 0,
            cmd: 0,
            header: None,
            menu: MenuState::default(),
            jump_base: 0,
            last_save_seen: seen,
            last_save_pos: 0,
            wait_cmd: 0,
            wait_base: 0,
            wait_time: 0,
            wait_cancel: 0,
            fade_cmd: 0,
            fade_count: 0,
            fade_step: 0,
            fade_prev: 0,
            fade_colour: [0; 3],
            flash_base: 0,
            flash_time: 0,
            flash_colour: [0; 3],
            flash_count: 0,
            mes_flag: 0,
            mes_wait_base: 0,
            #[cfg(feature = "native-engine")]
            area: None,
            #[cfg(feature = "native-engine")]
            area_enabled: [true; 256],
            select_flag: 0,
            select_index: 0,
            shake_flag: false,
            anm_flag: false,
            multi_anm: 0,
            loading: false,
            sound_wait: 0,
            #[cfg(feature = "native-engine")]
            ending: Ending::default(),
            #[cfg(feature = "native-engine")]
            effect: Effect::default(),
            auto_mode_count: -1,
            #[cfg(feature = "native-engine")]
            movie: None,
            name_wait: false,
            menu_wait: false,
        };
        if !scenario.change_seen(sys, seen) {
            sys.warn(format!("scene SEEN{seen:03}.TXT could not be opened"));
            sys.running = false;
        }
        scenario.pos = 0;
        scenario
    }

    /// Resume at `(seen, pos)` (load / return to menu).
    pub fn reset(&mut self, sys: &mut System, seen: i32, pos: i32) {
        if !self.change_seen(sys, seen) {
            sys.running = false;
        }
        self.pos = pos;
        self.cmd = 0;
        self.last_save_seen = self.seen;
        self.last_save_pos = self.pos;
        self.wait_cmd = 0;
        #[cfg(feature = "native-engine")]
        {
            self.effect = Effect::default();
        }
        self.fade_cmd = 0;
        self.mes_flag = 0;
        self.select_flag = 0;
        self.shake_flag = false;
        self.anm_flag = false;
        self.multi_anm = 0;
        self.loading = false;
        #[cfg(feature = "native-engine")]
        {
            self.ending = Ending::default();
        }
        #[cfg(feature = "native-engine")]
        {
            self.area = None;
        }
        #[cfg(feature = "native-engine")]
        {
            self.area_enabled = [true; 256];
        }
        self.auto_mode_count = -1;
        self.sound_wait = 0;
        self.flash_count = 0;
        #[cfg(feature = "native-engine")]
        {
            self.movie = None;
        }
        self.name_wait = false;
        self.menu_wait = false;
    }

    /// Acknowledge the deferred native mes_print return at its syscall boundary.
    #[cfg(feature = "portable-vm")]
    pub fn complete_message_print(&mut self, status: i32) -> anyhow::Result<()> {
        anyhow::ensure!((-1..=1).contains(&status), "native message print status");
        self.mes_flag = status;
        Ok(())
    }

    pub fn current_command(&self) -> u8 {
        self.cmd
    }

    fn code(&self) -> &[u8] {
        &self.data[self.code_offset.min(self.data.len())..]
    }

    fn peek(&self) -> u8 {
        usize::try_from(self.pos)
            .ok()
            .and_then(|pos| self.code().get(pos))
            .copied()
            .unwrap_or(0)
    }

    fn byte(&mut self) -> u8 {
        let byte = self.peek();
        self.pos += 1;
        byte
    }

    /// Reads a little-endian 32-bit integer.
    fn int(&mut self) -> i32 {
        let value = self.int_at(self.pos);
        self.pos += 4;
        value
    }

    fn int_at(&self, at: i32) -> i32 {
        usize::try_from(at)
            .ok()
            .and_then(|at| self.code().get(at..at + 4))
            .map_or(0, |bytes| {
                i32::from_le_bytes(bytes.try_into().expect("four bytes"))
            })
    }

    /// A compact number, dereferenced when it names
    /// a variable.
    fn value(&mut self, sys: &System) -> i32 {
        let at = self.pos.max(0) as usize;
        match parse_scene_value(self.code().get(at..).unwrap_or(&[])) {
            Ok((value, length)) => {
                self.pos += length as i32;
                match value.kind {
                    crate::scene::ValueKind::Constant => value.value as i32,
                    crate::scene::ValueKind::Variable => sys.flags.value(value.value as i32),
                }
            }
            Err(_) => {
                self.pos += 1;
                0
            }
        }
    }

    #[cfg(feature = "native-engine")]
    fn rect(&mut self, sys: &System) -> Rect {
        Rect::new(
            self.value(sys),
            self.value(sys),
            self.value(sys),
            self.value(sys),
        )
    }

    #[cfg(feature = "native-engine")]
    fn colour(&mut self, sys: &System) -> [i32; 3] {
        [self.value(sys), self.value(sys), self.value(sys)]
    }

    /// A C string, or `@n` for string variable `n`.
    fn text(&mut self, sys: &System) -> Vec<u8> {
        if self.peek() == b'@' {
            self.pos += 1;
            let index = self.value(sys);
            return sys.flags.string(index).to_vec();
        }
        let start = self.pos.max(0) as usize;
        let code = self.code();
        let end = code
            .get(start..)
            .and_then(|tail| tail.iter().position(|byte| *byte == 0))
            .map_or(code.len(), |length| start + length);
        let text = code.get(start..end).unwrap_or(&[]).to_vec();
        self.pos = end as i32 + 1;
        text
    }

    fn name(&mut self, sys: &System) -> String {
        nls::decode(&self.text(sys))
    }

    /// `(text, visible, attribute)`.
    fn formatted_text(&mut self, sys: &System) -> (Vec<u8>, bool, i32) {
        let mut out = Vec::new();
        let mut visible = true;
        let mut attribute = 0;
        loop {
            let c = self.byte();
            match c {
                0 => break,
                0xfe | 0xff => {
                    let start = self.pos.max(0) as usize;
                    let code = self.code();
                    let end = code
                        .get(start..)
                        .and_then(|tail| tail.iter().position(|byte| *byte == 0))
                        .map_or(code.len(), |length| start + length);
                    out.extend_from_slice(code.get(start..end).unwrap_or(&[]));
                    self.pos = end as i32 + 1;
                }
                0xfd => {
                    let index = self.value(sys);
                    out.extend_from_slice(sys.flags.string(index));
                }
                0x28 => {
                    self.pos -= 1;
                    let result = self.conditions(sys);
                    if result != 0 && result != 1 {
                        attribute = result;
                        visible = true;
                    } else {
                        visible = result != 0;
                    }
                }
                0x10 => match self.byte() {
                    0x01 => {
                        let value = self.value(sys);
                        out.extend(han_to_zen(value.to_string().as_bytes()));
                    }
                    0x02 => {
                        let value = self.value(sys);
                        let width = self.value(sys).clamp(0, 32) as usize;
                        out.extend(han_to_zen(format!("{value:0width$}").as_bytes()));
                    }
                    0x03 => {
                        let index = self.value(sys);
                        out.extend_from_slice(sys.flags.string(index));
                    }
                    0x11 => {
                        self.value(sys);
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        (out, visible, attribute)
    }

    /// 0/1, or a choice attribute when a
    /// true branch carries one (`$58 $20 colour` etc.).
    fn conditions(&mut self, sys: &System) -> i32 {
        const OPEN: u8 = b'(';
        const CLOSE: u8 = b')';
        const AND: u8 = 0x26;
        const OR: u8 = 0x27;
        const ATTR: u8 = 0x58;
        let mut tokens: Vec<u8> = Vec::new();
        let mut attributes: Vec<i32> = Vec::new();
        let mut depth = 0;
        loop {
            let c = self.byte();
            match c {
                OPEN => {
                    tokens.push(c);
                    depth += 1;
                }
                CLOSE => {
                    tokens.push(c);
                    depth -= 1;
                    if depth <= 0 {
                        break;
                    }
                }
                AND | OR => tokens.push(c),
                ATTR => {
                    let kind = i32::from(self.byte());
                    let attribute = match kind {
                        0x20 | 0x22 => kind + (self.value(sys) << 8),
                        _ => kind,
                    };
                    attributes.push(attribute);
                    tokens.push(ATTR);
                }
                0x36..=0x55 => {
                    let left = self.value(sys);
                    let right = self.value(sys);
                    let f = &sys.flags;
                    let result = match c {
                        0x36 => f.bit(left) != (right != 0),
                        0x37 => f.bit(left) == (right != 0),
                        0x38 => f.bit(left) != f.bit(right),
                        0x39 => f.bit(left) == f.bit(right),
                        0x3a => f.value(left) != right,
                        0x3b => f.value(left) == right,
                        0x41 | 0x42 => f.value(left) & right != 0,
                        0x43 => f.value(left) ^ right != 0,
                        0x44 => f.value(left) > right,
                        0x45 => f.value(left) < right,
                        0x46 => f.value(left) >= right,
                        0x47 => f.value(left) <= right,
                        0x48 => f.value(left) != f.value(right),
                        0x49 => f.value(left) == f.value(right),
                        0x4f | 0x50 => f.value(left) & f.value(right) != 0,
                        0x51 => f.value(left) ^ f.value(right) != 0,
                        0x52 => f.value(left) > f.value(right),
                        0x53 => f.value(left) < f.value(right),
                        0x54 => f.value(left) >= f.value(right),
                        0x55 => f.value(left) <= f.value(right),
                        _ => false,
                    };
                    tokens.push(u8::from(result));
                }
                _ => {
                    if self.pos as usize >= self.code().len() {
                        break;
                    }
                }
            }
        }
        // Reduce `(x)`, `x & y`, `x | y` left to right; an attribute marker
        // after a true group selects that attribute.
        let mut attribute = 0;
        let mut attribute_index = 0;
        let mut guard = 0;
        while tokens.len() > 1 && guard < 10_000 {
            guard += 1;
            let mut changed = false;
            let mut at = 0;
            while at < tokens.len() {
                if tokens[at] == OPEN
                    && tokens.get(at + 2) == Some(&CLOSE)
                    && tokens.get(at + 1).is_some_and(|value| *value < 2)
                {
                    let value = tokens[at + 1];
                    tokens.splice(at..at + 3, [value]);
                    changed = true;
                    if tokens.get(at + 1) == Some(&ATTR) {
                        if value == 1 {
                            if let Some(found) = attributes.get(attribute_index) {
                                attribute = *found;
                            }
                        }
                        attribute_index += 1;
                        let next = tokens.get(at + 2).copied();
                        if matches!(next, Some(0 | 1 | OPEN)) {
                            tokens[at + 1] = OR;
                        } else {
                            tokens.remove(at + 1);
                        }
                    }
                    continue;
                }
                if (tokens[at] == AND || tokens[at] == OR)
                    && at > 0
                    && tokens[at - 1] < 2
                    && tokens.get(at + 1).is_some_and(|value| *value < 2)
                {
                    let (left, right) = (tokens[at - 1], tokens[at + 1]);
                    let result = if tokens[at] == AND {
                        left & right
                    } else {
                        left | right
                    };
                    tokens.splice(at - 1..at + 2, [result]);
                    changed = true;
                    continue;
                }
                at += 1;
            }
            if !changed {
                break;
            }
        }
        let truth = tokens.first().copied().unwrap_or(0) == 1;
        if truth {
            if attribute != 0 { attribute } else { 1 }
        } else {
            0
        }
    }

    /// Records a save point.
    fn save_point(&mut self, sys: &mut System) {
        if sys.check_novel_save() {
            self.last_save_seen = self.seen;
            self.last_save_pos = self.pos;
            sys.flags.save_stack();
        }
    }

    fn jump(&mut self, target: i32) {
        self.pos = target + self.jump_base;
    }

    /// Switches to another scene.
    pub fn change_seen(&mut self, sys: &mut System, seen: i32) -> bool {
        if self.seen == seen && !self.data.is_empty() {
            if self
                .header
                .as_ref()
                .is_some_and(|header| !header.menus.is_empty())
            {
                self.pos = self.menu.loopback;
                self.menu.loopback = self.menu.loopback + self.int_at(self.menu.loopback - 4) + 4;
                self.menu.start = self.pos + self.int_at(self.pos - 4) - 1;
                self.jump_base = self.pos;
            } else {
                self.pos = 0;
            }
            sys.mouse.disable_popup();
            return true;
        }
        let bytes = match sys.read_seen(seen) {
            Ok(bytes) => bytes,
            Err(error) => {
                sys.warn(format!("SEEN{seen:03}.TXT: {error:#}"));
                return false;
            }
        };
        let header = match Avg32SceneHeader::parse(&bytes) {
            Ok(header) => header,
            Err(error) => {
                sys.warn(format!("SEEN{seen:03}.TXT: {error:#}"));
                return false;
            }
        };
        self.code_offset = header.code_offset;
        self.menu = MenuState {
            cur: -1,
            bit: 1,
            sub_bit: 1,
            start: header.menu_start,
            loopback: header.loopback,
            ..MenuState::default()
        };
        self.header = Some(header);
        self.data = bytes;
        self.seen = seen;
        self.jump_base = 0;
        self.pos = 0;
        sys.mouse.disable_popup();
        true
    }

    /// One frame's worth of decoding.
    pub fn run(&mut self, sys: &mut System) {
        self.run_budget(sys, 100_000);
    }

    /// Execute up to a caller-owned instruction budget, never waiting on a device.
    pub fn run_budget(&mut self, sys: &mut System, budget: u32) {
        #[cfg(feature = "portable-vm")]
        if sys.has_events() || sys.error.is_some() {
            return;
        }
        #[cfg(feature = "portable-vm")]
        let _nls = crate::nls::ScopedNls::enter(sys.nls);
        #[cfg(feature = "portable-vm")]
        {
            sys.frame_yielded = false;
        }
        for _ in 0..budget.min(100_000) {
            #[cfg(feature = "portable-vm")]
            {
                sys.instructions = sys.instructions.saturating_add(1);
            }
            if !sys.running {
                return;
            }
            let end = if sys.window_in_effect() {
                true
            } else if {
                #[cfg(feature = "native-engine")]
                {
                    self.effect.active()
                }
                #[cfg(feature = "portable-vm")]
                {
                    false
                }
            } {
                #[cfg(feature = "native-engine")]
                {
                    let mut effect = self.effect;
                    sys.run_effect(&mut effect);
                    self.effect = effect;
                    !sys.check_skip()
                }
                #[cfg(feature = "portable-vm")]
                {
                    false
                }
            } else if self.mes_flag != 0 {
                self.print_message(sys);
                !sys.check_skip()
            } else {
                #[cfg(feature = "native-engine")]
                if self.multi_anm != 0
                    && self.cmd != 0x13
                    && (self.multi_anm == 1 || sys.sound.koe_playing())
                {
                    sys.multi_animation_exec();
                }
                if self.cmd == 0 {
                    if self.pos < 0 || self.pos as usize >= self.code().len() {
                        sys.warn(format!(
                            "SEEN{:03}: ran past the end of the scene",
                            self.seen
                        ));
                        sys.running = false;
                        return;
                    }
                    self.cmd = self.byte();
                }
                self.decode(sys)
            };
            #[cfg(feature = "portable-vm")]
            if sys.has_events() || sys.error.is_some() {
                sys.frame_yielded = end;
                return;
            }
            if end {
                #[cfg(feature = "portable-vm")]
                {
                    sys.frame_yielded = true;
                }
                return;
            }
        }
        #[cfg(feature = "native-engine")]
        sys.warn(format!(
            "SEEN{:03}: {:#x}: command loop did not yield",
            self.seen, self.pos
        ));
    }

    /// Prints the pending message.
    fn print_message(&mut self, sys: &mut System) {
        let now = sys.now();
        if self.mes_flag == -1 {
            if !sys.check_skip() {
                sys.mes_draw_icon(1);
                if !sys.mouse.button(now) {
                    return;
                }
                self.mes_wait_base = now;
            }
            self.mes_flag = 1;
            sys.mes_clear_window();
        } else if sys.check_skip() || sys.mouse.button(now) {
            self.mes_flag = sys.mes_print_all();
        } else {
            if now.saturating_sub(self.mes_wait_base) < sys.ini().mes_wait.max(0) as u64 {
                return;
            }
            self.mes_wait_base = now;
            self.mes_flag = sys.mes_print();
        }
    }

    fn decode(&mut self, sys: &mut System) -> bool {
        match self.cmd {
            0x01 => self.click_wait(sys, true),
            0x02 => {
                if matches!(self.byte(), 1..=3) {
                    sys.mes_set(&[0x0d]);
                }
                self.cmd = 0;
                false
            }
            0x03 => self.click_wait(sys, false),
            0x04 => self.text_window(sys),
            0x05 => {
                match self.value(sys) {
                    1 => sys.mes.double_text = false,
                    2 => sys.mes.double_text = true,
                    other => sys.warn(format!("font size {other}")),
                }
                self.cmd = 0;
                false
            }
            0x06 | 0x1a | 0x22..=0x29 | 0x65 => {
                self.cmd = 0;
                true
            }
            0x08 => {
                if matches!(self.byte(), 0x10 | 0x11) {
                    self.value(sys);
                }
                self.cmd = 0;
                true
            }
            0x0b => self.graphics_load(sys),
            #[cfg(feature = "native-engine")]
            0x0c => self.animation(sys),
            0x0e => self.sound(sys),
            0x10 => self.value_text(sys),
            #[cfg(feature = "native-engine")]
            0x13 => self.fade(sys),
            0x15 => {
                let condition = self.conditions(sys);
                let target = self.int();
                if condition == 0 {
                    self.jump(target);
                }
                self.cmd = 0;
                false
            }
            0x16 => {
                let subcommand = self.byte();
                let target = self.value(sys);
                if subcommand != 1 {
                    sys.flags.push_stack(self.seen, self.pos);
                }
                if !self.change_seen(sys, target) {
                    sys.running = false;
                }
                self.save_point(sys);
                self.cmd = 0;
                false
            }
            #[cfg(feature = "native-engine")]
            0x17 => {
                if self.shake_flag {
                    if sys.shake_step() {
                        self.cmd = 0;
                        self.shake_flag = false;
                    }
                } else if self.byte() == 1 {
                    let pattern = self.value(sys);
                    sys.shake_setup(pattern);
                    self.shake_flag = true;
                } else {
                    self.cmd = 0;
                }
                true
            }
            #[cfg(feature = "native-engine")]
            0x18 => {
                if self.byte() == 1 {
                    let colour = self.value(sys);
                    sys.change_font_color(colour);
                }
                self.cmd = 0;
                false
            }
            0x19 => self.wait(sys),
            0x1b => {
                let target = self.int();
                sys.flags.push_stack(self.seen, self.pos);
                self.jump(target);
                self.cmd = 0;
                false
            }
            0x1c => {
                let target = self.int();
                self.jump(target);
                self.cmd = 0;
                false
            }
            0x1d | 0x1e => {
                let count = i32::from(self.byte());
                let index = self.value(sys);
                let selector = sys.flags.value(index);
                let mut target = None;
                for item in 1..=count {
                    let offset = self.int();
                    if item == selector {
                        target = Some(offset);
                    }
                }
                if let Some(target) = target {
                    if self.cmd == 0x1d {
                        sys.flags.push_stack(self.seen, self.pos);
                    }
                    self.jump(target);
                }
                self.cmd = 0;
                false
            }
            0x20 => {
                match self.byte() {
                    0x01 => {
                        if let Some((seen, pos)) = sys.flags.pop_stack() {
                            if seen != self.seen && !self.change_seen(sys, seen) {
                                sys.running = false;
                            }
                            self.pos = pos;
                        }
                    }
                    0x02 => {
                        if let Some((seen, pos)) = sys.flags.pop_stack() {
                            if !self.change_seen(sys, seen) {
                                sys.running = false;
                            }
                            self.pos = pos;
                            self.save_point(sys);
                        }
                    }
                    0x03 => {
                        sys.flags.pop_stack();
                    }
                    0x06 => sys.flags.clear_stack(),
                    other => sys.warn(format!("return subcommand {other:#04x}")),
                }
                self.cmd = 0;
                false
            }
            0x2c..=0x31 => {
                self.legacy(sys);
                self.cmd = 0;
                true
            }
            0x37 | 0x39 | 0x3b..=0x43 | 0x49..=0x51 | 0x56 | 0x57 => {
                self.variable(sys);
                self.cmd = 0;
                false
            }
            0x58 => self.choice(sys),
            0x59 => {
                self.strings(sys);
                self.cmd = 0;
                false
            }
            0x5b => {
                let subcommand = self.byte();
                let mut index = self.value(sys);
                while self.peek() != 0 {
                    let value = self.value(sys);
                    if subcommand == 1 {
                        sys.flags.set_value(index, value);
                    } else if subcommand == 2 {
                        sys.flags.set_bit(index, value != 0);
                    }
                    index += 1;
                }
                self.pos += 1;
                self.cmd = 0;
                false
            }
            0x5c => {
                let subcommand = self.byte();
                let from = self.value(sys);
                let to = self.value(sys).min(1999);
                let value = self.value(sys);
                for index in from..=to {
                    if subcommand == 1 {
                        sys.flags.set_value(index, value);
                    } else if subcommand == 2 {
                        sys.flags.set_bit(index, value != 0);
                    }
                }
                self.cmd = 0;
                false
            }
            0x5d => {
                let subcommand = self.byte();
                let source = self.value(sys);
                let destination = self.value(sys);
                let count = self.value(sys);
                for offset in 0..count.max(0) {
                    if subcommand == 1 {
                        let value = sys.flags.value(source + offset);
                        sys.flags.set_value(destination + offset, value);
                    } else if subcommand == 2 {
                        let bit = sys.flags.bit(source + offset);
                        sys.flags.set_bit(destination + offset, bit);
                    }
                }
                self.cmd = 0;
                false
            }
            0x5e => {
                let subcommand = self.byte();
                let index = self.value(sys);
                let value = match subcommand {
                    1..=4 => sys.date_time(i32::from(subcommand)),
                    0x10 => self.seen,
                    _ => 0,
                };
                sys.flags.set_value(index, value);
                self.cmd = 0;
                false
            }
            0x5f => {
                self.multi_add(sys);
                self.cmd = 0;
                false
            }
            0x60 => self.system_command(sys),
            #[cfg(feature = "native-engine")]
            0x61 => self.name_control(sys),
            #[cfg(feature = "native-engine")]
            0x63 => {
                match self.byte() {
                    0x01 => {
                        let rect = self.rect(sys);
                        let buffer = self.value(sys);
                        sys.gfx.get_region(rect, buffer);
                    }
                    0x02 => {
                        let (x, y) = (self.value(sys), self.value(sys));
                        let buffer = self.value(sys);
                        sys.gfx.put_region(x, y, buffer);
                    }
                    0x20 => {
                        self.byte();
                        self.value(sys);
                    }
                    other => sys.warn(format!("0x63 subcommand {other:#04x}")),
                }
                self.cmd = 0;
                false
            }
            #[cfg(feature = "native-engine")]
            0x64 | 0x67 | 0x68 | 0x69 | 0x6a => self.graphics(sys),
            #[cfg(feature = "native-engine")]
            0x66 => {
                self.byte();
                let (x, y) = (self.value(sys), self.value(sys));
                let buffer = self.value(sys);
                let colour = self.colour(sys);
                let (text, _, _) = self.formatted_text(sys);
                sys.draw_buffer_string(x, y, buffer, colour, &text);
                self.cmd = 0;
                true
            }
            #[cfg(feature = "native-engine")]
            0x6c => self.area_control(sys),
            #[cfg(feature = "native-engine")]
            0x6d => self.mouse_control(sys),
            #[cfg(feature = "native-engine")]
            0x6e => self.cg_mode(sys),
            0x6f => {
                for _ in 0..4 {
                    self.value(sys);
                }
                self.cmd = 0;
                true
            }
            #[cfg(feature = "native-engine")]
            0x70 => self.window_config(sys),
            #[cfg(feature = "native-engine")]
            0x72 => self.window_position(sys),
            #[cfg(feature = "native-engine")]
            0x73 => self.system_values(sys),
            #[cfg(feature = "native-engine")]
            0x74 => {
                match self.byte() {
                    0x01 => {
                        let index = self.value(sys);
                        sys.flags.set_value(index, sys.mouse.popup_flag());
                    }
                    0x02 => {
                        let disabled = self.value(sys);
                        if disabled != 0 {
                            sys.mouse.disable_popup();
                        }
                    }
                    0x03 => {
                        let item = self.value(sys);
                        let index = self.value(sys);
                        sys.flags
                            .set_value(index, i32::from(sys.menu_enabled(item)));
                    }
                    0x04 => {
                        let item = self.value(sys);
                        let enabled = self.value(sys);
                        sys.menu_enable(item, enabled != 0);
                    }
                    other => sys.warn(format!("0x74 subcommand {other:#04x}")),
                }
                self.cmd = 0;
                true
            }
            #[cfg(feature = "native-engine")]
            0x75 => {
                let subcommand = self.byte();
                let data = self.value(sys);
                let channel = usize::from((subcommand & 0x0f).max(1) - 1).min(3);
                match subcommand >> 4 {
                    0 => sys.flags.set_value(data, sys.sound.volumes[channel]),
                    1 => sys.sound.set_volume(channel, data),
                    2 => sys.sound.set_mute(channel, data != 0),
                    _ => sys.warn(format!("0x75 subcommand {subcommand:#04x}")),
                }
                self.cmd = 0;
                true
            }
            #[cfg(feature = "native-engine")]
            0x76 => {
                match self.byte() {
                    0x01 => {
                        let mode = self.value(sys);
                        sys.set_novel_mode(mode);
                    }
                    0x02 => {
                        self.value(sys);
                    }
                    0x04 | 0x05 => {}
                    other => sys.warn(format!("0x76 subcommand {other:#04x}")),
                }
                self.cmd = 0;
                true
            }
            0x7f => {
                self.value(sys);
                self.cmd = 0;
                true
            }
            0xfe | 0xff => self.text_packet(sys),
            #[cfg(feature = "native-engine")]
            _ => self.unknown(sys),
            #[cfg(feature = "portable-vm")]
            0 => self.unknown(sys),
            #[cfg(feature = "portable-vm")]
            other => {
                sys.unsupported(other, self.pos - 1);
                true
            }
        }
    }

    /// Wait for a click (`0x01` also ends the message line).
    fn click_wait(&mut self, sys: &mut System, line_feed: bool) -> bool {
        let now = sys.now();
        let proceed = if sys.check_skip() {
            true
        } else {
            sys.mes_draw_icon(0);
            sys.mouse.text_icon_start();
            sys.mouse.button(now)
        };
        if proceed {
            self.cmd = 0;
            sys.mes_hide_icon();
            sys.mouse.text_icon_end();
            if line_feed {
                self.mes_flag = sys.mes_line_feed();
            }
            self.save_point(sys);
            if line_feed {
                sys.play_se(3);
            }
        }
        true
    }

    /// `0x04` text window commands.
    fn text_window(&mut self, sys: &mut System) -> bool {
        let subcommand = self.peek();
        match subcommand {
            0x01 | 0x02 => {
                sys.mes_clear();
                sys.mes_hide();
                self.pos += 1;
                self.cmd = 0;
            }
            0x03 | 0x05 => {
                sys.mes_clear();
                self.pos += 1;
                self.cmd = 0;
            }
            0x04 => {
                let now = sys.now();
                let proceed = if sys.check_skip() {
                    true
                } else {
                    sys.mouse.text_icon_start();
                    sys.mes_draw_icon(1);
                    sys.mouse.button(now)
                };
                if proceed {
                    self.cmd = 0;
                    self.pos += 1;
                    sys.mouse.text_icon_end();
                    sys.mes_clear();
                    self.save_point(sys);
                    sys.play_se(3);
                }
            }
            other => {
                sys.warn(format!("text window subcommand {other:#04x}"));
                self.pos += 1;
                self.cmd = 0;
            }
        }
        true
    }

    #[cfg(feature = "native-engine")]
    fn start_effect(&mut self, sys: &mut System, effect: Effect) {
        self.effect = effect;
        self.effect.curcount = 0;
        self.effect.prevtime = sys.now();
        if !self.effect.active() {
            // `#SEL` presets with command 0 only load; nothing to show.
            self.effect.cmd = 0;
        }
    }

    /// `0x0b` picture loading.
    fn graphics_load(&mut self, sys: &mut System) -> bool {
        sys.mes_hide();
        let subcommand = self.byte();
        match subcommand {
            #[cfg(feature = "native-engine")]
            0x01 | 0x03 | 0x05 => {
                let name = self.text(sys);
                let index = self.value(sys);
                let effect = sys.copy_sel(index);
                sys.snr_load_effect(&name, &effect);
                self.start_effect(sys, effect);
            }
            #[cfg(feature = "native-engine")]
            0x02 | 0x04 | 0x06 => {
                let name = self.text(sys);
                let mut values = [0i32; 15];
                for value in &mut values {
                    *value = self.value(sys);
                }
                let effect = Effect {
                    sx1: values[0],
                    sy1: values[1],
                    sx2: values[2],
                    sy2: values[3],
                    dx: values[4],
                    dy: values[5],
                    steptime: values[6].max(0) as u32,
                    cmd: values[7],
                    mask: values[8],
                    arg1: values[9],
                    arg2: values[10],
                    arg3: values[11],
                    step: values[12],
                    arg5: values[13],
                    arg6: values[14],
                    srcpdt: 1,
                    dstpdt: 0,
                    ..Effect::default()
                };
                sys.snr_load_effect(&name, &effect);
                self.start_effect(sys, effect);
            }
            0x09 | 0x10 | 0x54 => {
                let name = self.text(sys);
                let buffer = self.value(sys);
                sys.snr_load_file(&name, buffer);
            }
            0x11 => {
                self.text(sys);
            }
            0x13 => {}
            #[cfg(feature = "native-engine")]
            0x22 | 0x24 => {
                let count = self.byte();
                let mut name = if subcommand == 0x22 {
                    let base = self.text(sys);
                    sys.snr_multi_load_file(&base);
                    base
                } else {
                    let base = self.value(sys);
                    sys.snr_multi_load_pdt(base);
                    Vec::new()
                };
                let index = self.value(sys);
                let effect = sys.copy_sel(index);
                for _ in 0..count {
                    let method = self.byte();
                    let layer = self.text(sys);
                    if !layer.is_empty() {
                        name = layer;
                    }
                    match method {
                        0x01 => sys.snr_load_copy(&name, Rect::full(), 0, 0, 1, method),
                        0x02 => {
                            let preset = self.value(sys);
                            let e = sys.copy_sel(preset);
                            sys.snr_load_copy(
                                &name,
                                Rect::new(e.sx1, e.sy1, e.sx2, e.sy2),
                                e.dx,
                                e.dy,
                                1,
                                method,
                            );
                        }
                        0x03 | 0x04 => {
                            let rect = self.rect(sys);
                            let (dx, dy) = (self.value(sys), self.value(sys));
                            if method == 0x04 {
                                // The fourth value is unused by every known
                                // engine revision.
                                self.value(sys);
                            }
                            sys.snr_load_copy(&name, rect, dx, dy, 1, method);
                        }
                        other => {
                            sys.warn(format!("0x0b:{subcommand:#04x} layer method {other:#04x}"))
                        }
                    }
                }
                sys.snr_commit_multi();
                self.start_effect(sys, effect);
            }
            0x30 => {}
            0x31 | 0x32 => {
                self.value(sys);
            }
            #[cfg(feature = "native-engine")]
            0x33 => {
                let index = self.value(sys);
                let count = sys.macro_count();
                sys.flags.set_value(index, count);
            }
            #[cfg(feature = "native-engine")]
            0x50 => sys.snr_copy(Rect::full(), 0, 0, 0, HIDEPDT as i32, 0),
            #[cfg(feature = "native-engine")]
            0x52 => {
                let index = self.value(sys);
                let mut effect = sys.copy_sel(index);
                effect.srcpdt = HIDEPDT as i32;
                sys.snr_commit_multi();
                self.start_effect(sys, effect);
            }
            other => sys.warn(format!("0x0b subcommand {other:#04x}")),
        }
        self.cmd = 0;
        false
    }

    /// `0x0c` animations.
    #[cfg(feature = "native-engine")]
    fn animation(&mut self, sys: &mut System) -> bool {
        if self.anm_flag {
            if sys.animation_exec() {
                self.anm_flag = false;
                self.cmd = 0;
            }
            return true;
        }
        let subcommand = self.byte();
        match subcommand {
            0x10 | 0x16 | 0x18 => {
                let name = self.name(sys);
                let seen = if subcommand == 0x18 {
                    0
                } else {
                    self.value(sys)
                };
                if subcommand != 0x10 {
                    self.value(sys);
                    self.value(sys);
                }
                if sys.animation_setup(&name, seen) {
                    self.anm_flag = true;
                } else {
                    self.cmd = 0;
                }
            }
            0x12 | 0x30 => {
                let name = self.name(sys);
                while self.peek() != 0 {
                    let seen = self.value(sys);
                    sys.multi_animation_setup(&name, seen);
                }
                self.pos += 1;
                self.multi_anm = if subcommand == 0x30 { -1 } else { 1 };
                self.cmd = 0;
            }
            0x13 => self.cmd = 0,
            0x20 | 0x24 => {
                self.text(sys);
                while self.peek() != 0 {
                    let seen = self.value(sys);
                    sys.multi_animation_stop(seen);
                }
                self.pos += 1;
                self.cmd = 0;
            }
            0x21 | 0x25 => {
                sys.multi_animation_clear();
                self.multi_anm = 0;
                self.cmd = 0;
            }
            other => {
                sys.warn(format!("0x0c subcommand {other:#04x}"));
                self.cmd = 0;
            }
        }
        true
    }

    /// `0x0e` BGM, effects, voice and movies.
    fn sound(&mut self, sys: &mut System) -> bool {
        if self.sound_wait != 0 {
            let playing = match self.sound_wait {
                0x02 | 0x06 => sys.sound.bgm_playing(),
                0x20 => sys.sound.koe_playing(),
                0x34 | 0x35 => sys.sound.wav_playing(),
                #[cfg(feature = "native-engine")]
                0x52..=0x55 => {
                    let cancellable = matches!(self.sound_wait, 0x53 | 0x55);
                    let now = sys.now();
                    if let Some((movie, _)) = self.movie.as_mut() {
                        if let Err(error) = movie.tick(&mut sys.gfx) {
                            sys.warn(format!("movie: {error:#}"));
                            movie.stop();
                        }
                        if cancellable && (sys.mouse.button(now) || sys.check_skip()) {
                            movie.stop();
                        }
                        movie.active()
                    } else {
                        false
                    }
                }
                _ => false,
            };
            let skipping = sys.check_skip() && !matches!(self.sound_wait, 0x52 | 0x54);
            if !playing || skipping {
                #[cfg(feature = "native-engine")]
                if matches!(self.sound_wait, 0x52..=0x55) {
                    #[cfg(feature = "native-engine")]
                    {
                        self.movie = None;
                    }
                    sys.sound.movie_stop();
                }
                self.cmd = 0;
                self.sound_wait = 0;
            }
            return true;
        }
        let subcommand = self.byte();
        // Fade lengths: DirectSound BGM fades
        // over `n * 40` ms, CD audio over five times that.
        let unit = if sys.ini().music_type == 2 { 40 } else { 200 };
        let fade = |value: i32| (value.max(0) as u32).saturating_mul(unit);
        match subcommand {
            0x01..=0x03 => {
                let name = self.name(sys);
                sys.bgm_play(&name, subcommand == 0x01, 0);
                if subcommand == 0x02 {
                    self.sound_wait = subcommand;
                    return true;
                }
            }
            0x05..=0x07 => {
                let name = self.name(sys);
                let time = self.value(sys);
                sys.bgm_play(&name, subcommand == 0x05, fade(time));
                if subcommand == 0x06 {
                    self.sound_wait = subcommand;
                    return true;
                }
            }
            0x10 => {
                let time = self.value(sys);
                sys.sound.bgm_stop(fade(time));
            }
            0x11 | 0x12 | 0x16 => sys.sound.bgm_stop(0),
            0x20..=0x22 => {
                let id = self.value(sys);
                if subcommand == 0x22 {
                    self.value(sys);
                }
                sys.koe_play(id);
                // `#KOE_TEXT_TYPE`: voiced lines are heard, not shown.
                if sys.ini().value("KOE_TEXT_TYPE") != 0
                    && sys.sound.koe_playing()
                    && matches!(self.peek(), 0xfe | 0xff)
                {
                    self.pos += 1;
                    if sys.version >= 1714 {
                        self.pos += 4;
                    }
                    self.text(sys);
                    if self.peek() == 0x01 {
                        self.pos += 1;
                    }
                }
                if subcommand == 0x20 {
                    self.sound_wait = subcommand;
                    return true;
                }
            }
            0x30..=0x35 => {
                let name = self.name(sys);
                let channel = matches!(subcommand, 0x31 | 0x33 | 0x35).then(|| self.value(sys));
                sys.wav_play(&name, matches!(subcommand, 0x32 | 0x33), channel);
                if matches!(subcommand, 0x34 | 0x35) {
                    self.sound_wait = subcommand;
                    return true;
                }
            }
            0x36 | 0x38 => sys.sound.wav_stop(None),
            0x37 | 0x39 => {
                let channel = self.value(sys);
                sys.sound.wav_stop(Some(channel));
            }
            0x40 | 0x44 => {
                let index = self.value(sys);
                sys.play_se(index);
            }
            #[cfg(feature = "native-engine")]
            0x50..=0x55 => {
                let name = self.name(sys);
                if subcommand >= 0x54 {
                    self.text(sys);
                }
                let rect = self.rect(sys);
                self.start_movie(sys, &name, subcommand == 0x51, rect);
                if subcommand >= 0x52 && self.movie.is_some() {
                    self.sound_wait = subcommand;
                    return true;
                }
            }
            0x60 => {}
            other => sys.warn(format!("0x0e subcommand {other:#04x}")),
        }
        self.cmd = 0;
        true
    }

    #[cfg(feature = "native-engine")]
    fn start_movie(&mut self, sys: &mut System, name: &str, looped: bool, rect: Rect) {
        let bytes = sys.game.resources.read("AVI", name).or_else(|_| {
            let directory = sys.ini().mov_dir.clone();
            crate::sound::find_file(
                sys.game.resources.root(),
                &directory,
                &format!("{name}.AVI"),
            )
            .ok_or_else(|| anyhow::anyhow!("movie {name} not found"))
            .and_then(|path| Ok(game_fs::read(path)?))
        });
        let movie = match bytes.and_then(|bytes| AviMovie::parse(&bytes)) {
            Ok(movie) => movie,
            Err(error) => {
                sys.warn(format!("movie {name}: {error:#}"));
                return;
            }
        };
        if let Some(audio) = &movie.audio {
            sys.sound.movie_play(
                audio.sample_rate,
                audio.channels,
                audio.bits_per_sample,
                &audio.pcm,
            );
        }
        match MoviePlayer::start(&movie, looped, rect, &mut sys.gfx) {
            Ok(Some(player)) => self.movie = Some((player, looped)),
            Ok(None) => {}
            Err(error) => sys.warn(format!("movie {name}: {error:#}")),
        }
    }

    /// Non-blocking movies keep playing underneath the scenario.
    #[cfg(feature = "native-engine")]
    pub fn tick_background(&mut self, sys: &mut System) {
        if self.sound_wait == 0 {
            if let Some((movie, _)) = self.movie.as_mut() {
                if movie.tick(&mut sys.gfx).is_err() || !movie.active() {
                    #[cfg(feature = "native-engine")]
                    {
                        self.movie = None;
                    }
                    sys.sound.movie_stop();
                }
            }
        }
    }

    /// `0x10` numbers and strings inserted into the message.
    fn value_text(&mut self, sys: &mut System) -> bool {
        let text = match self.byte() {
            0x01 => {
                let index = self.value(sys);
                han_to_zen(sys.flags.value(index).to_string().as_bytes())
            }
            0x02 => {
                let index = self.value(sys);
                let width = self.value(sys).clamp(0, 32) as usize;
                han_to_zen(format!("{:0width$}", sys.flags.value(index)).as_bytes())
            }
            0x03 => {
                let index = self.value(sys);
                sys.flags.string(index).to_vec()
            }
            other => {
                sys.warn(format!("0x10 subcommand {other:#04x}"));
                Vec::new()
            }
        };
        let mut packet = vec![0xff];
        packet.extend(text);
        sys.mes_set(&packet);
        self.mes_wait_base = sys.now();
        self.mes_flag = 1;
        self.cmd = 0;
        true
    }

    /// `0x13` screen fades through a colour.
    #[cfg(feature = "native-engine")]
    fn fade(&mut self, sys: &mut System) -> bool {
        if self.fade_cmd != 0 {
            let now = sys.now();
            if now.saturating_sub(self.fade_prev) >= self.fade_step || sys.check_skip() {
                self.fade_prev = now;
                sys.snr_screen_fade(self.fade_cmd, self.fade_count, self.fade_colour);
                self.fade_count += 1;
                if self.fade_count >= 16 {
                    sys.snr_fill_rect(Rect::full(), 3, self.fade_colour);
                    sys.mouse.start_pdt_draw();
                    self.fade_cmd = 0;
                    self.cmd = 0;
                    return false;
                }
            }
            return !sys.check_skip();
        }
        sys.mes_hide();
        self.fade_count = 0;
        let subcommand = self.byte();
        self.fade_cmd = subcommand;
        self.fade_prev = sys.now();
        sys.mouse.start_pdt_draw();
        let default_time = sys.ini().default_fade_time.max(0) as u64;
        let table = |sys: &System, index: i32| sys.ini().fade_table[index.clamp(0, 15) as usize];
        match subcommand {
            0x01 | 0x02 => {
                let index = self.value(sys);
                self.fade_step = if subcommand == 0x02 {
                    self.value(sys).max(0) as u64
                } else {
                    default_time
                };
                self.fade_colour = table(sys, index);
                sys.snr_fill_rect(Rect::full(), 1, self.fade_colour);
            }
            0x03 | 0x04 => {
                self.fade_colour = self.colour(sys);
                self.fade_step = if subcommand == 0x04 {
                    self.value(sys).max(0) as u64
                } else {
                    default_time
                };
                sys.snr_fill_rect(Rect::full(), 1, self.fade_colour);
            }
            0x10 => {
                let index = self.value(sys);
                self.fade_step = 0;
                self.fade_colour = table(sys, index);
                self.fade_count = 15;
            }
            0x11 => {
                self.fade_colour = self.colour(sys);
                self.fade_step = 0;
                self.fade_count = 15;
            }
            other => {
                sys.warn(format!("0x13 subcommand {other:#04x}"));
                self.fade_cmd = 0;
                self.cmd = 0;
            }
        }
        self.multi_anm = 0;
        false
    }

    /// `0x19` timed waits.
    fn wait(&mut self, sys: &mut System) -> bool {
        let now = sys.now();
        if self.wait_cmd != 0 {
            let skip = sys.check_skip();
            let elapsed = now.saturating_sub(self.wait_base);
            let since_base = sys.timer().max(0) as u64;
            let mut done = false;
            match self.wait_cmd {
                0x01 => done = skip || elapsed >= self.wait_time,
                0x04 => done = skip || since_base >= self.wait_time,
                0x02 | 0x05 => {
                    let reached = if self.wait_cmd == 0x02 {
                        elapsed
                    } else {
                        since_base
                    } >= self.wait_time;
                    if reached {
                        sys.flags.set_value(self.wait_cancel, 0);
                        done = true;
                    } else if skip || sys.mouse.button(now) {
                        sys.flags.set_value(self.wait_cancel, 1);
                        done = true;
                    }
                }
                _ => done = true,
            }
            if done {
                self.cmd = 0;
                self.wait_cmd = 0;
            }
            return true;
        }
        self.wait_cmd = self.byte();
        match self.wait_cmd {
            0x01 | 0x02 => {
                sys.mouse.start_pdt_draw();
                self.wait_time = self.value(sys).max(0) as u64;
                if self.wait_cmd == 0x02 {
                    self.wait_cancel = self.value(sys);
                }
                self.wait_base = now;
            }
            0x04 | 0x05 => {
                sys.mouse.start_pdt_draw();
                self.wait_time = self.value(sys).max(0) as u64;
                if self.wait_cmd == 0x05 {
                    self.wait_cancel = self.value(sys);
                }
            }
            0x03 => {
                sys.reset_timer();
                self.wait_cmd = 0;
                self.cmd = 0;
            }
            0x06 => {
                let index = self.value(sys);
                let timer = sys.timer();
                sys.flags.set_value(index, timer);
                self.wait_cmd = 0;
                self.cmd = 0;
            }
            0x10..=0x13 => {
                self.wait_cmd = 0;
                self.cmd = 0;
            }
            other => {
                sys.warn(format!("0x19 subcommand {other:#04x}"));
                self.wait_cmd = 0;
                self.cmd = 0;
            }
        }
        true
    }

    /// The `0x2c`-`0x31` family (only `0x2e` has a known
    /// meaning: it re-targets the scenario-menu availability bits).
    fn legacy(&mut self, sys: &mut System) {
        let subcommand = self.byte();
        match (self.cmd, subcommand) {
            (0x2c, 1) | (0x2d, 1) | (0x2d, 2) | (0x31, 1) => {
                self.value(sys);
            }
            (0x2d, 3) | (0x2d, 4) => {
                self.value(sys);
                self.value(sys);
            }
            (0x2e, 1) => {
                let value = self.value(sys);
                self.menu.bit_count = (value - 1).clamp(0, 7) as u8;
                self.menu.bit = 1 << self.menu.bit_count;
            }
            (0x2e, 2) => {
                self.value(sys);
                let value = self.value(sys);
                self.menu.sub_bit_count = (value - 1).clamp(0, 7) as u8;
                self.menu.sub_bit = 1 << self.menu.sub_bit_count;
            }
            (0x2f, 1) => {
                for _ in 0..3 {
                    self.value(sys);
                }
            }
            (0x30, 1) | (0x31, 2) => {}
            (command, other) => sys.warn(format!("{command:#04x} subcommand {other:#04x}")),
        }
    }

    /// Variable and bit arithmetic.
    fn variable(&mut self, sys: &mut System) {
        let index = self.value(sys);
        if self.cmd == 0x56 {
            let bit = sys.random(0, 1) != 0;
            sys.flags.set_bit(index, bit);
            return;
        }
        let data = self.value(sys);
        let f = &mut sys.flags;
        let current = f.value(index);
        let other = f.value(data);
        let result = match self.cmd {
            0x37 => {
                f.set_bit(index, data != 0);
                return;
            }
            0x39 => {
                let bit = f.bit(data);
                f.set_bit(index, bit);
                return;
            }
            0x3b => data,
            0x3c => current.wrapping_add(data),
            0x3d => current.wrapping_sub(data),
            0x3e => current.wrapping_mul(data),
            0x3f => current.checked_div(data).unwrap_or(current),
            0x40 => current.checked_rem(data).unwrap_or(current),
            0x41 => current & data,
            0x42 => current | data,
            0x43 => current ^ data,
            0x49 => other,
            0x4a => current.wrapping_add(other),
            0x4b => current.wrapping_sub(other),
            0x4c => current.wrapping_mul(other),
            0x4d => current.checked_div(other).unwrap_or(current),
            0x4e => current.checked_rem(other).unwrap_or(current),
            0x4f => current & other,
            0x50 => current | other,
            0x51 => current ^ other,
            0x57 => {
                let high = self.value(sys);
                let roll = sys.random(data, high);
                sys.flags.set_value(index, roll);
                return;
            }
            _ => return,
        };
        f.set_value(index, result);
    }

    /// `0x58` choices (and the load menu).
    fn choice(&mut self, sys: &mut System) -> bool {
        if self.select_flag != 0 {
            match self.select_flag {
                0x01 | 0x02 => {
                    let chosen = sys.select();
                    if chosen != -1 {
                        sys.flags.set_value(self.select_index, chosen);
                        self.cmd = 0;
                        sys.mes_clear();
                        if self.select_flag == 0x01 {
                            sys.select_sub_window_close();
                        }
                        self.select_flag = 0;
                        sys.mouse.text_icon_end();
                    }
                }
                0x04 => {
                    if sys.menu.is_none() {
                        let slot = sys.menu_result.take().unwrap_or(0);
                        sys.flags.set_value(self.select_index, slot);
                        self.select_flag = 0;
                        self.cmd = 0;
                    }
                }
                _ => {
                    self.select_flag = 0;
                    self.cmd = 0;
                }
            }
            return true;
        }
        self.select_flag = self.byte();
        match self.select_flag {
            0x01 | 0x02 => {
                self.pos -= 2;
                self.save_point(sys);
                self.pos += 2;
                self.select_index = self.value(sys);
                sys.select_clear();
                if self.byte() == 0x22 {
                    if self.peek() == 0 {
                        self.pos += 1;
                    }
                    loop {
                        let (text, visible, attribute) = self.formatted_text(sys);
                        if visible {
                            match attribute & 0xff {
                                0x20 => sys.select_add_item(&text, 1, attribute >> 8),
                                0x21 => sys.select_add_item(&text, -1, 0),
                                0x22 => sys.select_add_item(&text, 0, attribute >> 8),
                                _ => sys.select_add_item(&text, 1, 0),
                            }
                        } else {
                            sys.select_add_item(&text, -1, 0);
                        }
                        if self.peek() == 0x23 || self.pos as usize >= self.code().len() {
                            break;
                        }
                    }
                    self.pos += 1;
                }
                sys.mes_clear();
                if self.select_flag == 0x01 {
                    sys.select_sub_window_setup();
                }
                sys.mouse.text_icon_start();
            }
            0x04 => {
                self.select_index = self.value(sys);
                sys.open_load_picker();
            }
            other => {
                sys.warn(format!("0x58 subcommand {other:#04x}"));
                self.select_flag = 0;
                self.cmd = 0;
            }
        }
        true
    }

    /// `0x59` string variables.
    fn strings(&mut self, sys: &mut System) {
        let subcommand = self.byte();
        let index = self.value(sys);
        match subcommand {
            0x01 => {
                let text = self.text(sys);
                sys.flags.set_string(index, &text);
            }
            0x02 => {
                let source = self.value(sys);
                let length = sys.flags.string(source).len() as i32;
                sys.flags.set_value(index, length);
            }
            0x03 => {
                let left = self.value(sys);
                let right = self.value(sys);
                let order = strcmp(sys.flags.string(left), sys.flags.string(right));
                sys.flags.set_value(index, order);
            }
            0x04 => {
                let source = self.value(sys);
                let mut joined = sys.flags.string(index).to_vec();
                joined.extend_from_slice(sys.flags.string(source));
                sys.flags.set_string(index, &joined);
            }
            0x05 => {
                let source = self.value(sys);
                let text = sys.flags.string(source).to_vec();
                sys.flags.set_string(index, &text);
            }
            0x06 => {
                let destination = self.value(sys);
                let radix = self.value(sys);
                let value = sys.flags.value(index);
                let text = match radix {
                    10 => Some(value.to_string()),
                    16 => Some(format!("{value:X}")),
                    _ => None,
                };
                if let Some(text) = text {
                    sys.flags.set_string(destination, text.as_bytes());
                }
            }
            0x07 => {
                let text = han_to_zen(sys.flags.string(index));
                sys.flags.set_string(index, &text);
            }
            0x08 => {
                let destination = self.value(sys);
                let text = nls::decode(sys.flags.string(index));
                let digits: String = text
                    .trim_start()
                    .chars()
                    .enumerate()
                    .take_while(|(position, c)| {
                        c.is_ascii_digit() || (*position == 0 && (*c == '-' || *c == '+'))
                    })
                    .map(|(_, c)| c)
                    .collect();
                sys.flags
                    .set_value(destination, digits.parse().unwrap_or(0));
            }
            other => sys.warn(format!("0x59 subcommand {other:#04x}")),
        }
    }

    /// `0x5f` sums, percentages and indexed copies.
    fn multi_add(&mut self, sys: &mut System) {
        match self.byte() {
            0x01 => {
                let destination = self.value(sys);
                let mut total = 0i32;
                loop {
                    let code = self.byte();
                    match code {
                        0x01 => {
                            let index = self.value(sys);
                            total = total.wrapping_add(sys.flags.value(index));
                        }
                        0x02 => {
                            let from = self.value(sys);
                            let to = self.value(sys);
                            for index in from..=to {
                                total = total.wrapping_add(sys.flags.value(index));
                            }
                        }
                        0x11 => {
                            let index = self.value(sys);
                            total += i32::from(sys.flags.bit(index));
                        }
                        0x12 => {
                            let from = self.value(sys);
                            let to = self.value(sys);
                            for index in from..=to {
                                total += i32::from(sys.flags.bit(index));
                            }
                        }
                        _ => {}
                    }
                    if code == 0 || self.peek() == 0 {
                        if code != 0 {
                            self.pos += 1;
                        }
                        break;
                    }
                }
                sys.flags.set_value(destination, total);
            }
            0x10 => {
                let destination = self.value(sys);
                let numerator = self.value(sys);
                let denominator = self.value(sys);
                let percentage = if denominator == 0 {
                    0
                } else {
                    numerator.saturating_mul(100) / denominator
                };
                sys.flags.set_value(destination, percentage.clamp(0, 100));
            }
            0x20 => {
                let count = self.byte();
                let mut destination = self.value(sys);
                let source = self.value(sys);
                for _ in 0..count {
                    let offset = self.value(sys);
                    let value = sys.flags.value(source + offset);
                    sys.flags.set_value(destination, value);
                    destination += 1;
                }
            }
            other => sys.warn(format!("0x5f subcommand {other:#04x}")),
        }
    }

    /// `0x60` save, load, title and quit.
    fn system_command(&mut self, sys: &mut System) -> bool {
        #[cfg(feature = "native-engine")]
        if self.loading {
            if sys.loading_proc() {
                self.cmd = 0;
                self.loading = false;
            }
            return true;
        }
        match self.byte() {
            #[cfg(feature = "native-engine")]
            0x02 => {
                let slot = self.value(sys);
                if let Some((seen, pos)) = sys.load(slot - 1) {
                    sys.reset();
                    self.reset(sys, seen, pos);
                    self.cmd = 0x60;
                    self.loading = true;
                    return true;
                }
            }
            #[cfg(feature = "native-engine")]
            0x03 => {
                let slot = self.value(sys);
                // A scripted save resumes right after this command, so it
                // records the current call stack rather than the one at
                // the last save point.
                let saved = sys.flags.saved_stack().to_vec();
                sys.flags.save_stack();
                sys.save(slot - 1, self.seen, self.pos);
                sys.flags.restore_saved_stack(saved);
            }
            0x04 => {
                let (title, _, _) = self.formatted_text(sys);
                sys.set_window_title(title);
            }
            #[cfg(feature = "native-engine")]
            0x05 => sys.open_context_menu(),
            0x20 => sys.running = false,
            #[cfg(feature = "native-engine")]
            sub @ (0x30 | 0x35) => {
                let slot = self.value(sys);
                let index = self.value(sys);
                let summary = usize::try_from(slot - 1)
                    .ok()
                    .and_then(|slot| sys.slots.get(slot))
                    .cloned();
                let title = summary.map(|summary| {
                    if summary.valid {
                        if sub == 0x30 {
                            let mut text = format!(
                                "{:02}/{:02}({:02}:{:02}) ",
                                summary.month, summary.day, summary.hour, summary.minute
                            )
                            .into_bytes();
                            text.extend_from_slice(&summary.title);
                            text
                        } else {
                            summary.title
                        }
                    } else {
                        nls::encode(&sys.ini().save_no_title)
                    }
                });
                sys.flags.set_string(index, &title.unwrap_or_default());
            }
            #[cfg(feature = "native-engine")]
            sub @ (0x31 | 0x36 | 0x37) => {
                let slot = self.value(sys);
                let index = self.value(sys);
                let summary = usize::try_from(slot - 1)
                    .ok()
                    .and_then(|slot| sys.slots.get(slot))
                    .cloned()
                    .unwrap_or_default();
                let value = match sub {
                    0x31 => i32::from(summary.valid),
                    0x36 => summary.month * 100 + summary.day,
                    _ => summary.hour * 100 + summary.minute,
                };
                sys.flags.set_value(index, value);
            }
            other => sys.warn(format!("0x60 subcommand {other:#04x}")),
        }
        self.cmd = 0;
        true
    }

    /// `0x61` character names and name entry.
    #[cfg(feature = "native-engine")]
    fn name_control(&mut self, sys: &mut System) -> bool {
        if self.name_wait {
            if sys
                .name_input
                .as_ref()
                .is_none_or(|input| input.inline_box.is_some())
            {
                self.name_wait = false;
                self.cmd = 0;
            }
            return true;
        }
        match self.byte() {
            0x01 => {
                let rect = self.rect(sys);
                let foreground = self.colour(sys);
                let background = self.colour(sys);
                sys.name_input = Some(crate::system::NameInput {
                    inline_box: Some(crate::system::InlineBox {
                        rect: [rect.x1, rect.y1, rect.x2, rect.y2],
                        foreground,
                        background,
                        target: None,
                    }),
                    ..Default::default()
                });
            }
            0x02 => {
                let index = self.value(sys);
                let text = sys
                    .name_input
                    .as_ref()
                    .map(|input| nls::encode(&input.values[0]))
                    .unwrap_or_default();
                sys.flags.set_string(index, &text);
            }
            0x03 => {
                let index = self.value(sys);
                let initial = nls::decode(sys.flags.string(index));
                if let Some(input) = sys.name_input.as_mut() {
                    input.values[0] = initial;
                    if let Some(inline) = input.inline_box.as_mut() {
                        inline.target = Some(index);
                    }
                }
            }
            0x04 => sys.name_input = None,
            0x10 | 0x12 => {
                let name = self.value(sys);
                let index = self.value(sys);
                let value = sys.name(name).to_vec();
                sys.flags.set_string(index, &value);
            }
            0x11 => {
                let name = self.value(sys);
                let index = self.value(sys);
                let value = sys.flags.string(index).to_vec();
                sys.set_name(name, &value);
            }
            0x20 => {
                let index = self.value(sys) + 1;
                sys.open_name_input([nls::encode("名前"), Vec::new()], [index, 0]);
                self.name_wait = true;
                return true;
            }
            0x21 => {
                self.value(sys);
                self.text(sys);
                for _ in 0..9 {
                    self.value(sys);
                }
            }
            0x24 => {
                let count = self.byte();
                let mut titles = [Vec::new(), Vec::new()];
                let mut indices = [0, 0];
                for item in 0..usize::from(count) {
                    let index = self.value(sys);
                    let (title, _, _) = self.formatted_text(sys);
                    if item < 2 {
                        titles[item] = title;
                        indices[item] = index;
                    }
                }
                sys.open_name_input(titles, indices);
                self.name_wait = true;
                return true;
            }
            0x30 | 0x31 => {}
            other => sys.warn(format!("0x61 subcommand {other:#04x}")),
        }
        self.cmd = 0;
        true
    }

    /// `0x64`/`0x67`-`0x6a` buffer drawing.
    #[cfg(feature = "native-engine")]
    fn graphics(&mut self, sys: &mut System) -> bool {
        sys.mes_hide();
        let version = sys.version;
        match self.cmd {
            0x64 => {
                let subcommand = self.byte();
                match subcommand {
                    0x02 | 0x04 | 0x10 | 0x15 => {
                        let rect = self.rect(sys);
                        let buffer = self.value(sys);
                        let colour = self.colour(sys);
                        match subcommand {
                            0x02 => sys.snr_clear_rect(rect, buffer, colour),
                            0x04 => sys.gfx.draw_rect_line(rect, buffer, colour),
                            0x10 => sys.gfx.color_mask(rect, buffer, colour),
                            _ => {
                                let count = self.value(sys);
                                sys.snr_fade_color(rect, buffer, colour, count);
                            }
                        }
                    }
                    0x07 | 0x11 | 0x12 | 0x20 => {
                        let rect = self.rect(sys);
                        let buffer = self.value(sys);
                        match subcommand {
                            0x07 => sys.gfx.invert(rect, buffer),
                            0x11 => sys.snr_fade_color(rect, buffer, [0, 0, 0], 0x80),
                            0x12 => sys.snr_fade_color(rect, buffer, [0, 0, 0], 0xc0),
                            _ => sys.gfx.monochrome(rect, buffer),
                        }
                    }
                    0x30 => {
                        let source_rect = self.rect(sys);
                        let source = self.value(sys);
                        let destination_rect = self.rect(sys);
                        let destination = self.value(sys);
                        sys.gfx
                            .stretch_copy(source_rect, source, destination_rect, destination);
                    }
                    0x32 => {
                        let clamp = |value: i32, max: i32| value.clamp(0, max);
                        let mut effect = Effect {
                            sx1: self.value(sys),
                            sy1: self.value(sys),
                            sx2: self.value(sys),
                            sy2: self.value(sys),
                            arg3: self.value(sys),
                            arg4: self.value(sys),
                            arg5: self.value(sys),
                            arg6: self.value(sys),
                            srcpdt: self.value(sys),
                            dx: self.value(sys),
                            dy: self.value(sys),
                            arg1: self.value(sys),
                            arg2: self.value(sys),
                            dstpdt: self.value(sys),
                            step: self.value(sys),
                            steptime: self.value(sys).max(0) as u32,
                            cmd: 9999,
                            ..Effect::default()
                        };
                        effect.sx1 = clamp(effect.sx1, 639);
                        effect.sx2 = clamp(effect.sx2, 639);
                        effect.sy1 = clamp(effect.sy1, 479);
                        effect.sy2 = clamp(effect.sy2, 479);
                        self.start_effect(sys, effect);
                    }
                    other => sys.warn(format!("0x64 subcommand {other:#04x}")),
                }
                self.cmd = 0;
                false
            }
            0x67 => {
                let subcommand = self.byte();
                match subcommand {
                    0x00 => {
                        let rect = self.rect(sys);
                        let source = self.value(sys);
                        self.value(sys);
                        sys.gfx.copy_back_buffer(rect, source, true);
                    }
                    0x01 | 0x02 | 0x08 => {
                        let rect = self.rect(sys);
                        let source = self.value(sys);
                        let (dx, dy) = (self.value(sys), self.value(sys));
                        let destination = self.value(sys);
                        let has_flag = match subcommand {
                            0x01 => version >= 1704,
                            0x02 => version >= 1613,
                            _ => true,
                        };
                        let flag = if has_flag { self.value(sys) } else { 0 };
                        match subcommand {
                            0x01 => sys.snr_copy(rect, source, dx, dy, destination, flag),
                            0x02 => sys.snr_mask_copy(rect, source, dx, dy, destination, flag),
                            _ => sys
                                .gfx
                                .copy_with_mask(rect, source, dx, dy, destination, flag),
                        }
                    }
                    0x03 => {
                        let rect = self.rect(sys);
                        let source = self.value(sys);
                        let (dx, dy) = (self.value(sys), self.value(sys));
                        let destination = self.value(sys);
                        let key = self.colour(sys);
                        sys.snr_color_key_copy(rect, source, dx, dy, destination, key);
                    }
                    0x05 => {
                        let rect = self.rect(sys);
                        let source = self.value(sys);
                        let (dx, dy) = (self.value(sys), self.value(sys));
                        let destination = self.value(sys);
                        sys.snr_swap(rect, source, dx, dy, destination);
                    }
                    0x11 | 0x12 => {
                        let source = self.value(sys);
                        let destination = self.value(sys);
                        let has_flag = if subcommand == 0x11 {
                            version >= 1704
                        } else {
                            version >= 1613
                        };
                        let flag = if has_flag { self.value(sys) } else { 0 };
                        if subcommand == 0x11 {
                            sys.snr_all_copy(source, destination, flag);
                        } else {
                            sys.snr_mask_copy(Rect::full(), source, 0, 0, destination, flag);
                        }
                    }
                    0x20..=0x22 => {
                        let variable = self.value(sys);
                        let mut number = sys.flags.value(variable);
                        let origin = (self.value(sys), self.value(sys));
                        let size = (self.value(sys), self.value(sys));
                        let stride = (self.value(sys), self.value(sys));
                        let source = self.value(sys);
                        let target = (self.value(sys), self.value(sys));
                        let step = (self.value(sys), self.value(sys));
                        let count = self.value(sys);
                        let zero_padded = self.value(sys) != 0;
                        let destination = self.value(sys);
                        let extra = match subcommand {
                            0x21 => Some([self.value(sys), 0, 0]),
                            0x22 => Some(self.colour(sys)),
                            _ => None,
                        };
                        for cell in (0..count.max(0)).rev() {
                            let digit = number.rem_euclid(10);
                            let rect = Rect::new(
                                origin.0 + stride.0 * digit,
                                origin.1 + stride.1 * digit,
                                origin.0 + stride.0 * digit + size.0 - 1,
                                origin.1 + stride.1 * digit + size.1 - 1,
                            );
                            let (dx, dy) = (target.0 + step.0 * cell, target.1 + step.1 * cell);
                            match (subcommand, extra) {
                                (0x21, Some([flag, _, _])) => {
                                    sys.snr_mask_copy(rect, source, dx, dy, destination, flag)
                                }
                                (0x22, Some(key)) => {
                                    sys.snr_color_key_copy(rect, source, dx, dy, destination, key)
                                }
                                _ => sys.snr_copy(rect, source, dx, dy, destination, 0),
                            }
                            number /= 10;
                            if number == 0 && !zero_padded {
                                break;
                            }
                        }
                    }
                    other => sys.warn(format!("0x67 subcommand {other:#04x}")),
                }
                self.cmd = 0;
                false
            }
            0x68 => {
                if self.flash_count != 0 {
                    let now = sys.now();
                    if self.flash_count & 1 != 0 {
                        sys.snr_copy(Rect::full(), HIDEPDT as i32, 0, 0, 0, 0);
                        self.flash_base = now;
                        self.flash_count -= 1;
                        if self.flash_count == 0 {
                            self.cmd = 0;
                            return false;
                        }
                    } else if now.saturating_sub(self.flash_base) >= self.flash_time
                        || sys.check_skip()
                    {
                        sys.snr_fill_rect(Rect::full(), 0, self.flash_colour);
                        self.flash_count -= 1;
                    }
                    return true;
                }
                match self.byte() {
                    0x01 => {
                        let buffer = self.value(sys);
                        let colour = self.colour(sys);
                        sys.snr_fill_rect(Rect::full(), buffer, colour);
                        self.cmd = 0;
                    }
                    0x10 => {
                        self.flash_colour = self.colour(sys);
                        self.flash_time = self.value(sys).max(0) as u64;
                        self.flash_count = self.value(sys).max(0) * 2;
                        if self.flash_count != 0 {
                            sys.snr_copy(Rect::full(), 0, 0, 0, HIDEPDT as i32, 0);
                            sys.snr_fill_rect(Rect::full(), 0, self.flash_colour);
                            self.flash_count -= 1;
                            self.flash_base = sys.now();
                        } else {
                            self.cmd = 0;
                        }
                        return true;
                    }
                    other => {
                        sys.warn(format!("0x68 subcommand {other:#04x}"));
                        self.cmd = 0;
                    }
                }
                false
            }
            0x69 => {
                self.byte();
                let direction = i32::from(self.byte());
                let rect = self.rect(sys);
                let lines = self.value(sys);
                let speed = self.value(sys);
                let effect = Effect {
                    sx1: rect.x1,
                    sy1: rect.y1,
                    sx2: rect.x2,
                    sy2: rect.y2,
                    arg4: direction,
                    arg5: speed,
                    arg6: lines,
                    step: 8,
                    srcpdt: 1,
                    dstpdt: 0,
                    steptime: 1,
                    cmd: 1000,
                    ..Effect::default()
                };
                self.start_effect(sys, effect);
                self.cmd = 0;
                false
            }
            _ => self.ending(sys),
        }
    }

    /// `0x6a`: the ending scroller/slideshow.
    #[cfg(feature = "native-engine")]
    fn ending(&mut self, sys: &mut System) -> bool {
        if !self.ending.active {
            let mode = self.byte();
            self.ending = Ending {
                cmd: mode,
                ..Ending::default()
            };
            match mode {
                0x10 | 0x20 | 0x30 => {
                    self.ending.align = self.byte();
                    let count = self.byte();
                    self.ending.x = self.value(sys);
                    self.ending.wait = self.value(sys);
                    self.ending.pixel = self.value(sys).max(1);
                    if mode == 0x30 {
                        self.ending.cancel = self.value(sys);
                    }
                    for _ in 0..count {
                        let file = self.text(sys);
                        let spacing = self.value(sys);
                        let image = if matches!(file.first(), Some(b'?' | b'*') | None) {
                            None
                        } else {
                            sys.load_image(&nls::decode(&file))
                        };
                        let size = image
                            .as_ref()
                            .map_or((0, 0), |image| (image.width as i32, image.height as i32));
                        self.ending.offsets.push(self.ending.total);
                        self.ending.total += size.1 + spacing;
                        self.ending.sizes.push(size);
                        self.ending.files.push(file);
                        self.ending.images.push(None);
                    }
                    self.pos += 1;
                    sys.snr_copy(Rect::full(), 0, 0, 0, 1, 0);
                    self.ending.active = true;
                    self.ending.time = sys.now();
                }
                0x03 | 0x04 => {
                    let count = self.byte();
                    self.value(sys);
                    self.value(sys);
                    for _ in 0..count {
                        let file = self.text(sys);
                        let wait = self.value(sys);
                        self.ending.files.push(file);
                        self.ending.offsets.push(wait);
                    }
                    self.pos += 1;
                    if let Some(first) = self.ending.files.first().cloned() {
                        self.show_immediately(sys, &first);
                    }
                    self.ending.time = sys.now();
                    self.ending.wait = self.ending.offsets.first().copied().unwrap_or(0);
                    self.ending.active = true;
                }
                0x05 => {
                    self.cmd = 0;
                    return false;
                }
                other => {
                    sys.warn(format!("0x6a mode {other:#04x}"));
                    self.cmd = 0;
                    return false;
                }
            }
            return true;
        }
        let now = sys.now();
        match self.ending.cmd {
            0x10 | 0x20 | 0x30 => {
                if self.ending.cmd == 0x30 && (sys.mouse.button(now) || sys.check_skip()) {
                    sys.flags.set_value(self.ending.cancel, 1);
                    #[cfg(feature = "native-engine")]
                    {
                        self.ending = Ending::default();
                    }
                    self.cmd = 0;
                    return true;
                }
                if now.saturating_sub(self.ending.time) < self.ending.wait.max(0) as u64 {
                    return true;
                }
                self.ending.time = now;
                for index in 0..self.ending.files.len() {
                    let (width, height) = self.ending.sizes[index];
                    let line = self.ending.line - self.ending.offsets[index];
                    if line >= 0 && line - height < 480 {
                        let x = match self.ending.align {
                            1 => self.ending.x - width / 2,
                            3 => self.ending.x - width,
                            _ => self.ending.x,
                        };
                        let y = 479 - line;
                        if self.ending.images[index].is_none() && width > 0 {
                            let name = nls::decode(&self.ending.files[index]);
                            self.ending.images[index] = sys
                                .load_image(&name)
                                .map(|image| crate::buffer::PdtBuffer::from_image(&image));
                        }
                        if let Some(image) = &self.ending.images[index] {
                            let pixel = self.ending.pixel;
                            let area = Rect::new(x, y, x + width - 1, y + height + pixel - 1);
                            sys.gfx.copy(area, 1, x, y, 2, 0);
                            sys.gfx.mask_copy_from(
                                image,
                                Rect::new(0, 0, width - 1, height - 1),
                                x,
                                y,
                                2,
                                0,
                            );
                            sys.gfx.copy(area, 2, x, y, 0, 0);
                        }
                    } else {
                        self.ending.images[index] = None;
                    }
                }
                self.ending.line += self.ending.pixel;
                if self.ending.line >= self.ending.total {
                    sys.flags.set_value(self.ending.cancel, 0);
                    #[cfg(feature = "native-engine")]
                    {
                        self.ending = Ending::default();
                    }
                    self.cmd = 0;
                }
            }
            _ => {
                if now.saturating_sub(self.ending.time) < self.ending.wait.max(0) as u64
                    && !sys.check_skip()
                {
                    return true;
                }
                self.ending.time = now;
                self.ending.line += 1;
                if self.ending.line as usize >= self.ending.files.len() {
                    #[cfg(feature = "native-engine")]
                    {
                        self.ending = Ending::default();
                    }
                    self.cmd = 0;
                } else {
                    let index = self.ending.line as usize;
                    let file = self.ending.files[index].clone();
                    self.show_immediately(sys, &file);
                    self.ending.wait = self.ending.offsets[index];
                }
            }
        }
        true
    }

    #[cfg(feature = "native-engine")]
    fn show_immediately(&mut self, sys: &mut System, name: &[u8]) {
        let effect = Effect {
            sx2: 639,
            sy2: 479,
            cmd: 2,
            srcpdt: 1,
            dstpdt: 0,
            ..Effect::default()
        };
        sys.snr_load_effect(name, &effect);
        self.start_effect(sys, effect);
    }

    /// `0x6c` click areas (`.ARD` maps).
    #[cfg(feature = "native-engine")]
    fn area_control(&mut self, sys: &mut System) -> bool {
        match self.byte() {
            0x02 => {
                let cursor = self.name(sys);
                let name = self.name(sys);
                sys.cursor = sys
                    .game
                    .resources
                    .read("CUR", &cursor)
                    .ok()
                    .and_then(|bytes| crate::cursor::Cursor::parse(&bytes).ok());
                #[cfg(feature = "native-engine")]
                {
                    self.area_enabled = [true; 256];
                }
                self.area = match sys
                    .game
                    .resources
                    .read("ARD", &name)
                    .and_then(|bytes| AreaMap::parse(&bytes))
                {
                    Ok(map) => Some(map),
                    Err(error) => {
                        sys.warn(format!("ARD {name}: {error:#}"));
                        None
                    }
                };
            }
            0x03 => {
                #[cfg(feature = "native-engine")]
                {
                    self.area = None;
                }
                #[cfg(feature = "native-engine")]
                {
                    self.area_enabled = [true; 256];
                }
                sys.cursor = None;
            }
            sub @ (0x04 | 0x05) => {
                let area_index = self.value(sys);
                let button_index = self.value(sys);
                let now = sys.now();
                let (x, y, flag) = sys.mouse.state(now);
                if sub == 0x04 {
                    sys.flags.set_value(button_index, i32::from(flag > 0));
                    let area = if flag == 0 { self.area_find(x, y) } else { 0 };
                    sys.flags.set_value(area_index, area);
                } else {
                    sys.flags.set_value(button_index, flag);
                    let area = self.area_find(x, y);
                    sys.flags.set_value(area_index, area);
                }
            }
            0x10 => {
                let area = self.value(sys);
                if let Some(slot) = usize::try_from(area)
                    .ok()
                    .and_then(|area| self.area_enabled.get_mut(area))
                {
                    *slot = false;
                }
            }
            0x11 => {
                let area = self.value(sys);
                if let Some(slot) = usize::try_from(area)
                    .ok()
                    .and_then(|area| self.area_enabled.get_mut(area))
                {
                    *slot = true;
                }
            }
            0x15 => {
                let (x, y) = (self.value(sys), self.value(sys));
                let index = self.value(sys);
                let area = self.area_find(x, y);
                sys.flags.set_value(index, area);
            }
            0x20 => {
                self.value(sys);
                self.value(sys);
            }
            other => sys.warn(format!("0x6c subcommand {other:#04x}")),
        }
        self.cmd = 0;
        true
    }

    #[cfg(feature = "native-engine")]
    fn area_find(&self, x: i32, y: i32) -> i32 {
        let Some(map) = &self.area else {
            return 0;
        };
        let area = i32::from(map.area_at(x, y));
        if self.area_enabled[area as usize] {
            area
        } else {
            0
        }
    }

    /// `0x6d` mouse state.
    #[cfg(feature = "native-engine")]
    fn mouse_control(&mut self, sys: &mut System) -> bool {
        let now = sys.now();
        match self.byte() {
            0x01 => {
                let (x, y, flag) = sys.mouse.state(now);
                if flag != -1 {
                    let (xi, yi, fi) = (self.value(sys), self.value(sys), self.value(sys));
                    sys.flags.set_value(xi, x);
                    sys.flags.set_value(yi, y);
                    sys.flags.set_value(fi, flag);
                    self.cmd = 0;
                } else {
                    self.pos -= 1;
                }
                return true;
            }
            0x02 => {
                let (xi, yi, fi) = (self.value(sys), self.value(sys), self.value(sys));
                let (x, y, flag) = sys.mouse.state(now);
                sys.flags.set_value(xi, x);
                sys.flags.set_value(yi, y);
                sys.flags.set_value(fi, flag);
            }
            0x03 => sys.mouse.flush(),
            0x20 => sys.mouse.visible = false,
            0x21 => sys.mouse.visible = true,
            other => sys.warn(format!("0x6d subcommand {other:#04x}")),
        }
        self.cmd = 0;
        true
    }

    /// `0x6e` CG mode (`MODE.CGM`).
    #[cfg(feature = "native-engine")]
    fn cg_mode(&mut self, sys: &mut System) -> bool {
        match self.byte() {
            0x01 => {
                let index = self.value(sys);
                sys.flags.set_value(index, sys.cgm.total());
            }
            0x02 => {
                let index = self.value(sys);
                let count = sys.cgm.seen_count(&sys.flags);
                sys.flags.set_value(index, count);
            }
            0x03 => {
                let index = self.value(sys);
                let percentage = sys.cgm.percentage(&sys.flags);
                sys.flags.set_value(index, percentage);
            }
            0x04 => {
                if self.auto_cg_mode(sys) {
                    self.value(sys);
                } else {
                    self.pos -= 1;
                    return true;
                }
            }
            0x05 => {
                let number = self.value(sys);
                let name_index = self.value(sys);
                let flag_index = self.value(sys);
                let entry = sys.cgm.entry(sys.flags.value(number)).cloned();
                sys.flags.set_string(
                    name_index,
                    entry
                        .as_ref()
                        .map_or(&[][..], |entry| entry.name.as_bytes()),
                );
                sys.flags
                    .set_value(flag_index, entry.map_or(0, |entry| entry.flag));
            }
            other => sys.warn(format!("0x6e subcommand {other:#04x}")),
        }
        self.cmd = 0;
        true
    }

    /// Shows every seen CG, one per click.
    #[cfg(feature = "native-engine")]
    fn auto_cg_mode(&mut self, sys: &mut System) -> bool {
        let now = sys.now();
        if self.auto_mode_count == -1 || sys.mouse.button(now) || sys.check_skip() {
            loop {
                self.auto_mode_count += 1;
                if self.auto_mode_count >= sys.cgm.total() {
                    self.auto_mode_count = -1;
                    return true;
                }
                if sys.cgm.seen(self.auto_mode_count, &sys.flags) {
                    let name = sys
                        .cgm
                        .entry(self.auto_mode_count)
                        .map(|entry| entry.name.clone())
                        .unwrap_or_default();
                    let effect = sys.copy_sel(0);
                    sys.snr_load_effect(name.as_bytes(), &effect);
                    self.start_effect(sys, effect);
                    break;
                }
            }
        }
        false
    }

    /// `0x70` message-window colour and style.
    #[cfg(feature = "native-engine")]
    fn window_config(&mut self, sys: &mut System) -> bool {
        match self.byte() {
            0x01 => {
                let indices = [
                    self.value(sys),
                    self.value(sys),
                    self.value(sys),
                    self.value(sys),
                ];
                let ini = sys.ini();
                let values = [
                    ini.win_color_flag,
                    ini.win_color[0],
                    ini.win_color[1],
                    ini.win_color[2],
                ];
                for (index, value) in indices.into_iter().zip(values) {
                    sys.flags.set_value(index, value);
                }
            }
            0x02 => {
                let flag = self.value(sys);
                let colour = self.colour(sys);
                sys.set_window_colour(flag, colour);
            }
            sub @ (0x03 | 0x05) => {
                let index = self.value(sys);
                let key = if sub == 0x03 {
                    "WINDOW_MOVE_BOX"
                } else {
                    "WINDOW_CLEAR_BOX"
                };
                let value = sys.ini().value(key);
                sys.flags.set_value(index, value);
            }
            sub @ (0x04 | 0x06) => {
                let value = self.value(sys);
                let key = if sub == 0x04 {
                    "WINDOW_MOVE_BOX"
                } else {
                    "WINDOW_CLEAR_BOX"
                };
                sys.ini_mut().set_value(key, value);
            }
            0x10 => {
                let index = self.value(sys);
                let style = if sys.mes.style_force != 0 {
                    sys.mes.style_force
                } else {
                    sys.mes.style - 1
                };
                sys.flags.set_value(index, style);
            }
            0x11 => {
                let style = self.value(sys);
                sys.change_window_style(style);
            }
            other => sys.warn(format!("0x70 subcommand {other:#04x}")),
        }
        self.cmd = 0;
        true
    }

    /// `0x72` window positions.
    #[cfg(feature = "native-engine")]
    fn window_position(&mut self, sys: &mut System) -> bool {
        let subcommand = self.byte();
        match subcommand {
            0x01..=0x05 => {
                let (xi, yi) = (self.value(sys), self.value(sys));
                let ini = sys.ini();
                let (x, y) = match subcommand {
                    0x01 => (ini.win_x, ini.win_y),
                    0x02 => (ini.sub_win_x, ini.sub_win_y),
                    0x03 => (ini.sys_win[0], ini.sys_win[1]),
                    0x04 => (ini.sub_pos[0], ini.sub_pos[1]),
                    _ => (ini.value("WINDOW_GRP_POS_X"), ini.value("WINDOW_GRP_POS_Y")),
                };
                sys.flags.set_value(xi, x);
                sys.flags.set_value(yi, y);
            }
            0x11..=0x15 => {
                let (x, y) = (self.value(sys), self.value(sys));
                match subcommand {
                    0x11 => sys.mes_set_pos(x, y),
                    0x12 => {
                        sys.ini_mut().sub_win_x = x;
                        sys.ini_mut().sub_win_y = y;
                    }
                    0x13 => sys.ini_mut().sys_win = [x, y],
                    0x14 => sys.ini_mut().sub_pos = [x, y],
                    _ => {
                        sys.ini_mut().set_value("WINDOW_GRP_POS_X", x);
                        sys.ini_mut().set_value("WINDOW_GRP_POS_Y", y);
                    }
                }
            }
            other => sys.warn(format!("0x72 subcommand {other:#04x}")),
        }
        self.cmd = 0;
        true
    }

    /// `0x73` system values (`#MESSAGE_SIZE`, `#MSG_SPEED`, …).
    #[cfg(feature = "native-engine")]
    fn system_values(&mut self, sys: &mut System) -> bool {
        let subcommand = self.byte();
        match subcommand {
            0x01 | 0x05 => {
                let (xi, yi) = (self.value(sys), self.value(sys));
                let ini = sys.ini();
                let (x, y) = if subcommand == 0x01 {
                    (ini.mes_x, ini.mes_y)
                } else {
                    (ini.font_x, ini.font_y)
                };
                sys.flags.set_value(xi, x);
                sys.flags.set_value(yi, y);
            }
            0x02 => {
                let (x, y) = (self.value(sys), self.value(sys));
                sys.mes_set_size(x, y);
            }
            0x06 => {
                let (x, y) = (self.value(sys), self.value(sys));
                sys.set_font_size(x, y);
            }
            0x28 | 0x2a => {
                let index = self.value(sys);
                let speed = sys.ini().mes_wait;
                sys.flags
                    .set_value(index, if speed != 0 { speed - 8 } else { 0 });
            }
            0x29 | 0x2b => {
                let speed = self.value(sys).max(0);
                sys.ini_mut().mes_wait = if speed != 0 { speed + 8 } else { 0 };
            }
            0x31 => {
                let (x, y) = (self.value(sys), self.value(sys));
                sys.mouse_move(x, y);
            }
            0x32 | 0x34 => {
                self.value(sys);
            }
            0x10..=0x27 | 0x2c..=0x30 | 0x33 => {
                let get = subcommand & 1 == 0 || subcommand == 0x33;
                let key = match subcommand {
                    0x10 | 0x11 => "MOJI_COLOR",
                    0x12 | 0x13 => "MSG_CANCEL",
                    0x16 | 0x17 => "MOJI_KAGE",
                    0x18 | 0x19 => "KAGE_COLOR",
                    0x1a | 0x1b => "SEL_CANCEL",
                    0x1c | 0x1d => "CTRL_KEY",
                    0x1e | 0x1f => "SAVE_START",
                    0x20 | 0x21 => "NVL_TEXT_OFF",
                    0x22 | 0x23 => "FADE_TIME",
                    0x24 | 0x25 => "CURSOR_MONO",
                    0x26 | 0x27 => "COPY_WIND_SW",
                    0x2c | 0x2d => "RETURN_KEY_WAIT",
                    0x2e | 0x2f => "KOE_TEXT_TYPE",
                    _ => "GAME_SPECK_INIT",
                };
                if get {
                    let index = self.value(sys);
                    let value = match key {
                        "MOJI_COLOR" => sys.ini().font_color,
                        "FADE_TIME" => sys.ini().default_fade_time,
                        "CTRL_KEY" => i32::from(!sys.skip_enabled),
                        _ => sys.ini().value(key),
                    };
                    sys.flags.set_value(index, value);
                } else {
                    let value = self.value(sys);
                    match key {
                        "MOJI_COLOR" => sys.ini_mut().font_color = value,
                        "FADE_TIME" => sys.ini_mut().default_fade_time = value,
                        "CTRL_KEY" => sys.skip_enabled = value == 0,
                        _ => {}
                    }
                    sys.ini_mut().set_value(key, value);
                }
            }
            other => sys.warn(format!("0x73 subcommand {other:#04x}")),
        }
        self.cmd = 0;
        true
    }

    /// `0xfe`/`0xff` message text (consecutive packets are
    /// gathered into one message).
    fn text_packet(&mut self, sys: &mut System) -> bool {
        self.pos -= 1;
        self.save_point(sys);
        self.pos += 1;
        let extended = sys.version >= 1714;
        loop {
            let file_offset = (self.code_offset + self.pos.max(0) as usize) as u32;
            #[cfg(feature = "native-engine")]
            if let Some(voice) = sys
                .voice_patch
                .as_ref()
                .and_then(|patch| patch.voice_at(self.seen, file_offset))
            {
                sys.koe_play(voice);
            }
            if extended {
                self.int();
            }
            let mut packet = vec![self.cmd];
            packet.extend(self.text(sys));
            sys.mes_set(&packet);
            let next = self.peek();
            if next != 0xfe && next != 0xff {
                break;
            }
            self.cmd = next;
            self.pos += 1;
        }
        self.mes_flag = 1;
        self.mes_wait_base = sys.now();
        self.cmd = 0;
        true
    }

    /// `0x00` (scenario-menu display or end of game) and unknown
    /// opcodes.
    fn unknown(&mut self, sys: &mut System) -> bool {
        if self.cmd == 0 {
            let has_menu = self
                .header
                .as_ref()
                .is_some_and(|header| !header.menus.is_empty());
            if has_menu && self.pos - 1 == self.menu.start {
                self.pos -= 1;
                self.scenario_menu(sys);
                return true;
            }
            if self.pos as usize >= self.code().len().saturating_sub(1) {
                sys.running = false;
            } else {
                sys.warn(format!(
                    "SEEN{:03}: {:#x}: stray 0x00",
                    self.seen,
                    self.pos - 1
                ));
            }
        } else {
            sys.warn(format!(
                "SEEN{:03}: {:#x}: unknown opcode {:#04x}",
                self.seen,
                self.pos - 1,
                self.cmd
            ));
        }
        self.cmd = 0;
        true
    }

    /// The two-level scenario menu of scenes whose
    /// header carries one.
    fn scenario_menu(&mut self, sys: &mut System) {
        let Some(header) = self.header.clone() else {
            return;
        };
        if self.select_flag != 0 {
            let chosen = if self.menu.cur != -1
                && header
                    .menus
                    .get(self.menu.cur as usize)
                    .is_some_and(|menu| menu.submenus.len() == 1)
            {
                1
            } else {
                sys.select()
            };
            if chosen == -1 {
                return;
            }
            sys.mes_clear();
            self.select_flag = 0;
            if self.menu.cur == -1 {
                self.menu.cur = chosen - 1;
                return;
            }
            let (menu, sub) = (self.menu.cur as usize, (chosen - 1).max(0) as usize);
            self.menu.cur_sub = sub as i32;
            let Some(submenu) = header
                .menus
                .get(menu)
                .and_then(|menu| menu.submenus.get(sub))
            else {
                self.menu.cur = -1;
                return;
            };
            let repeat = usize::from(self.menu.sub_count[menu.min(7)][sub.min(7)]);
            if repeat + 1 < submenu.starts.len() {
                self.menu.sub_count[menu.min(7)][sub.min(7)] += 1;
            }
            let repeat = repeat.min(submenu.starts.len().saturating_sub(1));
            self.pos = submenu.starts.get(repeat).copied().unwrap_or(0);
            self.menu.start = self.pos + self.int_at(self.pos - 4) - 1;
            let flag = submenu.flags.get(repeat).copied().unwrap_or(0);
            if flag != 0 {
                if flag & 0x80 != 0 {
                    self.menu.bit_count = (self.menu.bit_count + 1).min(7);
                    self.menu.bit = 1 << self.menu.bit_count;
                } else {
                    self.menu.sub_bit_count = flag.min(7);
                    self.menu.sub_bit = 1 << self.menu.sub_bit_count;
                }
            }
            self.menu.cur = -1;
            self.jump_base = self.pos;
            return;
        }
        sys.mes_clear();
        sys.select_clear();
        self.select_flag = 2;
        if self.menu.cur == -1 {
            if header.menus.len() == 1 {
                self.menu.cur = 0;
                self.select_flag = 0;
            } else {
                for menu in &header.menus {
                    let text = header
                        .menu_strings
                        .get(menu.string)
                        .cloned()
                        .unwrap_or_default();
                    let enable = if menu.id & self.menu.bit != 0 { 1 } else { -1 };
                    sys.select_add_item(&text, enable, 0);
                }
            }
        } else if let Some(menu) = header.menus.get(self.menu.cur as usize) {
            if menu.submenus.len() >= 2 {
                for submenu in &menu.submenus {
                    let text = header
                        .menu_strings
                        .get(submenu.string)
                        .cloned()
                        .unwrap_or_default();
                    let enable = if submenu.id & self.menu.sub_bit != 0 {
                        1
                    } else {
                        -1
                    };
                    sys.select_add_item(&text, enable, 0);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(feature = "native-engine")]
    fn han_to_zen_matches_the_reference_table() {
        assert_eq!(han_to_zen(b"12"), vec![0x82, 0x50, 0x82, 0x51]);
        assert_eq!(han_to_zen(b"A "), vec![0x82, 0x60, 0x81, 0x40]);
    }

    #[test]
    #[cfg(feature = "native-engine")]
    fn strcmp_orders_like_c() {
        assert_eq!(strcmp(b"abc", b"abc"), 0);
        assert!(strcmp(b"abc", b"abd") < 0);
        assert!(strcmp(b"abcd", b"abc") > 0);
    }
}
