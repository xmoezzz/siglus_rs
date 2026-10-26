//! The AVG32 "system" layer: input, screen macros, animation, shake,
//! save/load and the other services the interpreter calls.
//!
//! The scenario interpreter ([`crate::scenario`]) drives this layer one
//! frame at a time; the platform frontend only feeds it input events and
//! uploads [`System::frame_rgba`].

use std::path::PathBuf;
use std::rc::Rc;
use web_time::Instant;

use crate::animation::Avg32Animation;
use crate::buffer::{AVG32_HEIGHT, AVG32_WIDTH, SCREEN_H, SCREEN_W};
use crate::cgmode::CgMode;
use crate::effect::{Effect, EffectRunner};
use crate::flags::Flags;
use crate::font::{GlyphSource, NovelFont, TrueTypeFont};
use crate::game::Avg32Game;
use crate::ini::Ini;
use crate::menu::MenuState;
use crate::meswin::{MesWin, Selection};
use crate::nls;
use crate::pdt::{PdtImage, decode_pdt};
use crate::pdtmgr::{ANMPDT, BACKUPPDT, PdtManager, Rect};
use crate::savedata::{self, MacroItem, SlotData, SlotSummary};
use crate::sound::Sound;

/// Milliseconds since some fixed origin; real or simulated.
#[derive(Debug, Clone)]
pub enum Clock {
    Real(Instant),
    Virtual(u64),
}

impl Clock {
    pub fn now(&self) -> u64 {
        match self {
            Clock::Real(start) => start.elapsed().as_millis() as u64,
            Clock::Virtual(now) => *now,
        }
    }

    pub fn advance(&mut self, milliseconds: u64) {
        if let Clock::Virtual(now) = self {
            *now += milliseconds;
        }
    }
}

/// Keys the engine reacts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Enter,
    Up,
    Down,
    Left,
    Right,
    Escape,
    Space,
    PageUp,
    PageDown,
    Backspace,
}

impl Key {
    /// The key codes recorded for `GetKeyInput`-style queries.
    fn code(self) -> u8 {
        match self {
            Key::Enter => 0x0d,
            Key::Up => 0x1e,
            Key::Down => 0x1f,
            Key::Left => 0x1c,
            Key::Right => 0x1d,
            Key::Escape => 0x1b,
            Key::Space => 0x20,
            Key::PageUp => 0x0b,
            Key::PageDown => 0x0c,
            Key::Backspace => 0x08,
        }
    }
}

pub use crate::input::Mouse;

#[derive(Debug, Clone, Default)]
struct Shake {
    pattern: usize,
    count: usize,
    base: u64,
    time: u64,
    offset: Option<(i32, i32)>,
}

#[derive(Debug, Clone, Default)]
struct AnimItem {
    seen: usize,
    stream: usize,
    frame: usize,
    end_stream: usize,
    frames: Vec<crate::animation::AnimationCell>,
    prev_time: u64,
    wait: u64,
}

#[derive(Debug, Clone, Default)]
struct Animation {
    data: Option<Rc<Avg32Animation>>,
    name: String,
    single: Option<AnimItem>,
    multi: Vec<AnimItem>,
}

/// State of the post-load fade.
#[derive(Debug, Clone)]
pub struct Loading {
    count: i32,
    time: u64,
    effect: Effect,
    title: Vec<u8>,
    bgm: Vec<u8>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NameInput {
    /// Titles and `#NAME` indices (1-based; 0 = unused) of up to two fields.
    pub titles: [String; 2],
    pub indices: [i32; 2],
    pub values: [String; 2],
    pub focus: usize,
    /// `Some((x1, y1, x2, y2, fg, bg, string))` for the in-scene text box
    /// (`0x61:01`), which edits a string variable instead of a name.
    pub inline_box: Option<InlineBox>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InlineBox {
    pub rect: [i32; 4],
    pub foreground: [i32; 3],
    pub background: [i32; 3],
    pub target: Option<i32>,
}

pub struct System {
    pub game: Avg32Game,
    pub version: i32,
    pub gfx: PdtManager,
    pub flags: Flags,
    pub clock: Clock,
    timer_base: u64,
    rng: u64,
    pub mouse: Mouse,
    key: Option<(u8, u64)>,
    pub skip_key: bool,
    pub skip_enabled: bool,
    pub skip_mode: bool,
    pub mes: MesWin,
    pub sel: Selection,
    pub window_effect: Effect,
    window_runner: EffectRunner,
    pub effect_runner: EffectRunner,
    shake: Shake,
    anim: Animation,
    pub sound: Sound,
    pub cgm: CgMode,
    macros: Vec<MacroItem>,
    pending_macro: Option<MacroItem>,
    pub names: [Vec<u8>; 26],
    pub menu_enabled: [bool; 32],
    pub title: Vec<u8>,
    pub window_title: String,
    pub slots: Vec<SlotSummary>,
    pub loading: Option<Loading>,
    pub(crate) font: Option<Box<dyn GlyphSource>>,
    pub(crate) novel_font: Option<NovelFont>,
    pub running: bool,
    pub hide_window: bool,
    pub name_input: Option<NameInput>,
    pub menu: Option<MenuState>,
    /// Result of a load picker opened by the scenario (`0x58:04`).
    pub menu_result: Option<i32>,
    /// Game-supplied mouse cursor (`CUR16M`), drawn into the frame.
    pub cursor: Option<crate::cursor::Cursor>,
    /// A `voicepat.txt`-style voice table (see [`crate::voicepatch`]).
    pub voice_patch: Option<crate::voicepatch::VoicePatch>,
    /// Requests for the host (full-screen toggle, ...).
    pub requests: Vec<HostRequest>,
    pub warnings: Vec<String>,
    /// Menu work that needs the scenario (save, load, return to menu).
    pub pending_action: Option<crate::menu::SystemAction>,
    /// Message history (the `#MSGBK` backlog) and the page being viewed.
    pub backlog: Vec<BacklogPage>,
    pub backlog_view: Option<usize>,
    last_voice: Option<i32>,
    save_path: PathBuf,
    persist: bool,
}

/// One page of the message backlog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BacklogPage {
    /// Message-buffer bytes (NLS-encoded, with `0xfe`/`0xff`/`0x0d` markers).
    pub text: Vec<u8>,
    pub voice: Option<i32>,
}

const BACKLOG_PAGES: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostRequest {
    ToggleFullscreen(bool),
    Quit,
}

impl std::fmt::Debug for System {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("System")
            .field("version", &self.version)
            .finish_non_exhaustive()
    }
}

/// Detects the engine revision.
pub fn detect_version(ini: &Ini, root: &std::path::Path) -> i32 {
    if let Some(version) = std::env::var("AVG32_VERSION")
        .ok()
        .and_then(|value| value.parse().ok())
    {
        return version;
    }
    let registry: String = ini
        .reg_name
        .chars()
        .map(|c| if c == '¥' { '\\' } else { c })
        .collect();
    let known = [
        ("BONBEE\\RIBBON2", 1704),
        ("Mebius\\絶望", 1704),
        ("KEY\\KANON_DEMO", 1613),
        ("KEY\\KANON", 1613),
        ("KEY\\KANON_ALL", 1713),
        ("KEY\\AIR", 1714),
        ("KEY\\DEMO\\AIR_01", 1713),
        ("RAM\\NEGAI", 1613),
        ("RAM\\KOIGOKORO", 1714),
        ("OTHERWISE\\SENSEOFF", 1713),
        ("OZ_PROJECT\\BABYFACE", 1713),
        ("SAGAPLANETS\\P_HEART", 1713),
        ("SAGAPLANETS\\DEMO\\REN", 1714),
        ("13CM\\SUKI", 1604),
        ("13CM\\フロレアール", 1613),
        ("13CM\\DEVOTE", 1704),
        ("13CM\\LEMON", 1713),
        ("13CM\\SHIMAI", 1714),
        ("13CM\\NYUIN", 1613),
        ("ZERO\\IINARI\\", 1713),
        ("ZERO\\BALLET\\", 1613),
        ("ZERO\\RYO_DOU", 1714),
        ("MANBOU\\MIND", 1613),
        ("FLADY\\MAYAKU", 1604),
        ("G-PANDA\\MAMAHAHA", 1714),
        ("G-PANDAMAMAHAHA", 1714),
        ("CRAFT\\FLOWERS", 1604),
        ("SIRIUS\\HIME", 1714),
        ("REX\\HAKO", 1714),
    ];
    if let Some((_, version)) = known.iter().find(|(name, _)| *name == registry) {
        return *version;
    }
    let size = |name: &str| {
        crate::resource::find_case_insensitive(root, std::path::Path::new(name))
            .and_then(|path| game_fs::file_len(path).ok())
    };
    if let Some(size) = size("AVG3217D.EXE") {
        return if size >= 330_000 { 1714 } else { 1704 };
    }
    if let Some(size) = size("AVG3217M.EXE") {
        return if size >= 310_000 { 1714 } else { 1713 };
    }
    if let Some(size) = size("AVG3216D.EXE") {
        return if size >= 280_000 { 1613 } else { 1604 };
    }
    1613
}

impl System {
    pub fn read_seen(&self, seen: i32) -> anyhow::Result<Vec<u8>> {
        self.game.read_seen(seen)
    }

    pub fn new(game: Avg32Game, clock: Clock, audio: bool, persist: bool) -> Self {
        let version = detect_version(&game.ini, &game.layout.root);
        let root = game.layout.root.clone();
        let names = std::array::from_fn(|index| nls::encode(&game.ini.names[index]));
        let cgm = game
            .resources
            .read("***", &game.ini.cgm_file)
            .ok()
            .and_then(|bytes| CgMode::parse(&bytes).ok())
            .unwrap_or_default();
        let novel_font = game
            .resources
            .read("***", "FN.DAT")
            .ok()
            .map(NovelFont::new);
        let save_name = if game.ini.save_file.is_empty() {
            "SAVE.INI".to_owned()
        } else {
            game.ini.save_file.clone()
        };
        let save_path = std::env::var_os("AVG32_SAVE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.clone())
            .join(save_name);
        let sound = if audio {
            Sound::new(&root)
        } else {
            Sound::silent(&root)
        };
        let mut system = Self {
            version,
            gfx: PdtManager::new(),
            flags: Flags::default(),
            clock,
            timer_base: 0,
            rng: 0x853c_49e6_748f_ea9b,
            mouse: Mouse::default(),
            key: None,
            skip_key: false,
            skip_enabled: true,
            skip_mode: false,
            mes: MesWin::default(),
            sel: Selection::default(),
            window_effect: Effect::default(),
            window_runner: EffectRunner::default(),
            effect_runner: EffectRunner::default(),
            shake: Shake::default(),
            anim: Animation::default(),
            sound,
            cgm,
            macros: Vec::new(),
            pending_macro: None,
            names,
            menu_enabled: [true; 32],
            title: Vec::new(),
            window_title: String::new(),
            slots: Vec::new(),
            loading: None,
            font: TrueTypeFont::load_system().map(|font| Box::new(font) as Box<dyn GlyphSource>),
            novel_font,
            running: true,
            hide_window: false,
            name_input: None,
            menu: None,
            menu_result: None,
            cursor: None,
            voice_patch: None,
            requests: Vec::new(),
            warnings: Vec::new(),
            pending_action: None,
            backlog: Vec::new(),
            backlog_view: None,
            last_voice: None,
            save_path,
            persist,
            game,
        };
        system.timer_base = system.now();
        if audio {
            system.voice_patch = crate::voicepatch::VoicePatch::discover(&system.game);
        }
        system.set_window_title(Vec::new());
        system.mes.style = 1;
        system.load_global_flags();
        system.refresh_slots();
        let style = system.mes.style;
        system.mes_setup(style);
        if !system.game.ini.exfont.is_empty() {
            let exfont = system.game.ini.exfont.clone();
            if let Some(image) = system.load_image(&exfont) {
                system
                    .gfx
                    .load_file(Some(&image), crate::pdtmgr::EXFONTPDT as i32);
            }
        }
        system
    }

    pub fn ini(&self) -> &Ini {
        &self.game.ini
    }

    pub fn ini_mut(&mut self) -> &mut Ini {
        &mut self.game.ini
    }

    /// Replaces the glyph rasteriser (e.g. with a font bundled by the host).
    pub fn set_font(&mut self, font: Box<dyn GlyphSource>) {
        self.font = Some(font);
    }

    pub fn warn(&mut self, message: impl Into<String>) {
        if self.warnings.len() < 256 {
            self.warnings.push(message.into());
        }
    }

    // ---- time / random -----------------------------------------------------

    pub fn now(&self) -> u64 {
        self.clock.now()
    }

    pub fn reset_timer(&mut self) {
        self.timer_base = self.now();
    }

    pub fn timer(&self) -> i32 {
        self.now().saturating_sub(self.timer_base) as i32
    }

    pub fn seed_random(&mut self, seed: u64) {
        self.rng = seed;
    }

    /// Inclusive range, using the original native generator.
    pub fn random(&mut self, low: i32, high: i32) -> i32 {
        crate::rng::random(&mut self.rng, low, high)
    }

    pub fn rng_state(&self) -> u64 {
        self.rng
    }

    /// Current date/time field selected by `kind`.
    pub fn date_time(&self, kind: i32) -> i32 {
        use chrono::{Datelike, Timelike};
        let now = chrono::Local::now();
        match kind {
            1 => now.month() as i32 * 100 + now.day() as i32,
            2 => now.hour() as i32 * 100 + now.minute() as i32,
            3 => now.year() - 1900,
            4 => now.weekday().num_days_from_sunday() as i32,
            _ => 0,
        }
    }

    // ---- input ---------------------------------------------------------------

    pub fn mouse_move(&mut self, x: i32, y: i32) {
        self.mouse.x = if (0..SCREEN_W).contains(&x) { x } else { -999 };
        self.mouse.y = if (0..SCREEN_H).contains(&y) { y } else { -999 };
    }

    /// A mouse button release.
    pub fn mouse_up(&mut self, right: bool) {
        if self.backlog_view.is_some() {
            crate::menu::backlog_click(self, right);
            return;
        }
        if self.menu.is_some() {
            crate::menu::menu_click(self, right);
            return;
        }
        let modal_name_input = self
            .name_input
            .as_ref()
            .is_some_and(|input| input.inline_box.is_none());
        if modal_name_input && !right {
            return;
        }
        if self.hide_window {
            self.hide_window = false;
            let style = self.mes.style;
            self.mes_setup(style);
            return;
        }
        self.mouse.click_time = self.now();
        if right {
            if self.mouse.menu_flag() {
                self.open_context_menu();
            } else {
                self.mouse.click = 1;
            }
        } else {
            self.mouse.click = 0;
        }
    }

    /// A key press.
    pub fn key_down(&mut self, key: Key) {
        if self.backlog_view.is_some() {
            crate::menu::backlog_key(self, key);
            return;
        }
        if self.menu.is_some() {
            crate::menu::menu_key(self, key);
            return;
        }
        if self.name_input.is_some() {
            crate::menu::name_input_key(self, key);
            return;
        }
        match key {
            Key::Enter => {
                if self.hide_window {
                    self.hide_window = false;
                    let style = self.mes.style;
                    self.mes_setup(style);
                } else {
                    self.mouse.click_time = self.now();
                    self.mouse.click = 0;
                }
                self.key = Some((key.code(), self.now()));
            }
            Key::Space => {
                if self.hide_window {
                    self.hide_window = false;
                    let style = self.mes.style;
                    self.mes_setup(style);
                } else if self.mes.shown {
                    self.hide_window = true;
                    self.mes_hide_temp();
                }
            }
            Key::PageUp => self.open_backlog(),
            Key::Escape => {
                if self.mouse.menu_flag() {
                    self.open_context_menu();
                } else {
                    self.mouse.click_time = self.now();
                    self.mouse.click = 1;
                }
            }
            _ => self.key = Some((key.code(), self.now())),
        }
    }

    /// Adds a finished message page to the backlog.
    pub fn record_backlog(&mut self, text: &[u8]) {
        let has_text = text
            .iter()
            .any(|byte| !matches!(byte, 0xfe | 0xff | 0x0d | 0x00));
        if !has_text {
            return;
        }
        let page = BacklogPage {
            text: text.to_vec(),
            voice: self.last_voice.take(),
        };
        if let Some(last) = self.backlog.last_mut() {
            if last.text == page.text {
                if last.voice.is_none() {
                    last.voice = page.voice;
                }
                return;
            }
        }
        self.backlog.push(page);
        if self.backlog.len() > BACKLOG_PAGES {
            self.backlog.remove(0);
        }
    }

    /// Opens the backlog at the newest page (only while the game waits for
    /// the player, as the original offers it).
    pub fn open_backlog(&mut self) {
        // The page on screen belongs to the history as well.
        let current = self.mes.buf[..self.mes.ptr.min(self.mes.buf.len())].to_vec();
        self.record_backlog(&current);
        if !self.backlog.is_empty() && self.menu.is_none() && self.name_input.is_none() {
            self.backlog_view = Some(self.backlog.len() - 1);
        }
    }

    pub fn replay_voice(&mut self, id: i32) {
        let version = self.version;
        self.sound.koe_play(&self.game.ini, version, id);
    }

    /// Mouse-wheel input: up opens/scrolls back the backlog, down scrolls
    /// forward and finally closes it.
    pub fn wheel(&mut self, up: bool) {
        match (self.backlog_view, up) {
            (None, true) => self.open_backlog(),
            (Some(page), true) => self.backlog_view = Some(page.saturating_sub(1)),
            (Some(page), false) => {
                self.backlog_view = (page + 1 < self.backlog.len()).then_some(page + 1);
            }
            (None, false) => {}
        }
    }

    /// A left click the scenario sees even while a text box has focus.
    pub fn mouse_click_for_script(&mut self) {
        self.mouse.click_time = self.now();
        self.mouse.click = 0;
    }

    pub fn text_input(&mut self, text: &str) {
        if self.name_input.is_some() {
            crate::menu::name_input_text(self, text);
        }
    }

    /// The key pressed within the last 200 ms.
    pub fn key_input(&mut self) -> u8 {
        let now = self.now();
        let key = self
            .key
            .filter(|(_, time)| now.saturating_sub(*time) < 200)
            .map_or(0, |(code, _)| code);
        self.key = None;
        key
    }

    /// Whether fast-forward is active.
    pub fn check_skip(&self) -> bool {
        (self.skip_key || self.skip_mode) && self.skip_enabled
    }

    // ---- files ---------------------------------------------------------------------

    pub fn load_image(&mut self, name: &str) -> Option<PdtImage> {
        let name = name.trim_end_matches([' ', '\0']);
        match self
            .game
            .resources
            .read("PDT", name)
            .and_then(|bytes| decode_pdt(&bytes))
        {
            Ok(image) => Some(image),
            Err(error) => {
                self.warn(format!("PDT {name}: {error:#}"));
                None
            }
        }
    }

    fn is_working_name(name: &[u8]) -> bool {
        matches!(name.first(), Some(b'*' | b'?'))
    }

    fn mark_cg(&mut self, name: &str) {
        let cgm = std::mem::take(&mut self.cgm);
        cgm.mark_seen(name.trim(), &mut self.flags);
        self.cgm = cgm;
    }

    // ---- screen macros ------------------------------------------------------------

    fn stack_macro(&mut self, item: MacroItem) {
        self.macros.push(item);
        if self.macros.len() > savedata::MAX_MACROS {
            self.macros.remove(0);
        }
    }

    pub fn macro_count(&self) -> i32 {
        self.macros.len() as i32
    }

    /// Rebuilds the screen from the recorded draws.
    fn redraw_macros(&mut self) {
        for item in self.macros.clone() {
            let a = &item.args;
            let file = |index: usize| {
                item.files
                    .get(index)
                    .map(|name| nls::decode(name))
                    .unwrap_or_default()
            };
            match item.cmd {
                0x00 => {
                    let image = self.load_named(&file(0));
                    self.gfx.load_file(image.as_ref(), 1);
                    self.gfx
                        .mask_copy(Rect::new(a[0], a[1], a[2], a[3]), 1, a[4], a[5], 0, 0);
                }
                0x02 => {
                    let image = self.load_named(&file(0));
                    self.gfx.load_file(image.as_ref(), a[26]);
                }
                0x32 => {
                    let colour = [(a[24] >> 16) & 255, (a[24] >> 8) & 255, a[24] & 255];
                    self.gfx
                        .fill_rect(Rect::new(a[26], a[27], a[28], a[29]), a[30], colour);
                }
                0x4c => self.gfx.fade_color(
                    Rect::new(a[26], a[27], a[28], a[29]),
                    a[30],
                    [a[31], a[32], a[33]],
                    a[34],
                ),
                0x64 => self.gfx.copy(
                    Rect::new(a[26], a[27], a[28], a[29]),
                    a[30],
                    a[31],
                    a[32],
                    a[33],
                    a[34],
                ),
                0x66 => self.gfx.mask_copy(
                    Rect::new(a[26], a[27], a[28], a[29]),
                    a[30],
                    a[31],
                    a[32],
                    a[33],
                    a[34],
                ),
                0x68 => {
                    let colour = [(a[34] >> 16) & 255, (a[34] >> 8) & 255, a[34] & 255];
                    self.gfx.color_key_copy(
                        Rect::new(a[26], a[27], a[28], a[29]),
                        a[30],
                        a[31],
                        a[32],
                        a[33],
                        colour,
                    );
                }
                0x6a => self.gfx.swap(
                    Rect::new(a[26], a[27], a[28], a[29]),
                    a[30],
                    a[31],
                    a[32],
                    a[33],
                ),
                0x96 | 0x97 => {
                    if item.cmd == 0x96 {
                        let image = self.load_named(&file(0));
                        self.gfx.load_base_file(image.as_ref(), 1);
                    } else {
                        let source = file(0).trim().parse().unwrap_or(0);
                        self.gfx.all_copy(source, 1, 0);
                    }
                    for layer in 1..item.files.len() {
                        let name = file(layer);
                        let Some(image) = self.load_image(&name) else {
                            continue;
                        };
                        let base = 26 + (layer - 1) * 8;
                        if a.get(base) == Some(&2) {
                            self.gfx.load_copy(
                                &image,
                                Rect::new(a[base + 1], a[base + 2], a[base + 3], a[base + 4]),
                                a[base + 5],
                                a[base + 6],
                                1,
                            );
                        } else {
                            self.gfx.load_copy(&image, Rect::full(), 0, 0, 1);
                        }
                    }
                    self.gfx.mask_copy(Rect::full(), 1, 0, 0, 0, 0);
                }
                _ => {}
            }
        }
    }

    fn load_named(&mut self, name: &str) -> Option<PdtImage> {
        if Self::is_working_name(name.as_bytes()) {
            None
        } else {
            self.load_image(name)
        }
    }

    // ---- scenario PDT operations -------------------------------------------------

    pub fn snr_screen_fade(&mut self, cmd: u8, count: i32, colour: [i32; 3]) {
        if count == 0 {
            let mut item = MacroItem::new(0x32);
            item.args[24] = (colour[0] << 16) | (colour[1] << 8) | colour[2];
            item.args[26..31].copy_from_slice(&[0, 0, 639, 479, 0]);
            self.stack_macro(item);
        }
        self.screen_fade(cmd, count, colour);
    }

    /// One of the 16 ordered-dither passes (or a
    /// solid fill for the instant `0x10`/`0x11` variants).
    pub fn screen_fade(&mut self, cmd: u8, count: i32, colour: [i32; 3]) {
        const X: [i32; 16] = [0, 2, 0, 2, 1, 3, 1, 3, 0, 2, 0, 2, 1, 3, 1, 3];
        const Y: [i32; 16] = [0, 2, 2, 0, 1, 3, 3, 1, 1, 3, 3, 1, 0, 2, 2, 0];
        let colour = colour.map(|c| c.clamp(0, 255) as u8);
        let display = self.gfx.display_mut();
        if matches!(cmd, 0x10 | 0x11) {
            for pixel in display.rgb.chunks_exact_mut(3) {
                pixel.copy_from_slice(&colour);
            }
        } else {
            let phase = count.clamp(0, 15) as usize;
            let mut y = Y[phase];
            while y < SCREEN_H {
                let mut x = X[phase];
                while x < SCREEN_W {
                    let at = display.at(x, y);
                    display.set_pixel(at, colour);
                    x += 4;
                }
                y += 4;
            }
        }
        self.gfx.present_all();
    }

    pub fn snr_load_file(&mut self, name: &[u8], destination: i32) {
        let text = nls::decode(name);
        let mut item = MacroItem::new(0x02);
        item.files.push(name.to_vec());
        item.args[26] = destination;
        self.stack_macro(item);
        let image = self.load_named(&text);
        if image.is_some() || Self::is_working_name(name) {
            self.gfx.load_file(image.as_ref(), destination);
        }
        self.mark_cg(&text);
    }

    /// Loads the picture into buffer 1; the caller then
    /// runs `effect` to reveal it.
    pub fn snr_load_effect(&mut self, name: &[u8], effect: &Effect) {
        let text = nls::decode(name);
        let mut item = MacroItem::new(0x00);
        item.files.push(name.to_vec());
        item.args[..15].copy_from_slice(&[
            effect.sx1,
            effect.sy1,
            effect.sx2,
            effect.sy2,
            effect.dx,
            effect.dy,
            effect.steptime as i32,
            effect.cmd,
            effect.mask,
            effect.arg1,
            effect.arg2,
            effect.arg3,
            effect.step,
            effect.arg5,
            effect.arg6,
        ]);
        self.stack_macro(item);
        let image = self.load_named(&text);
        if image.is_some() || Self::is_working_name(name) {
            self.gfx.load_file(image.as_ref(), 1);
        }
        self.mark_cg(&text);
    }

    pub fn snr_multi_load_file(&mut self, name: &[u8]) {
        let text = nls::decode(name);
        let mut item = MacroItem::new(0x96);
        item.files.push(name.to_vec());
        self.pending_macro = Some(item);
        let image = self.load_named(&text);
        self.gfx.load_base_file(image.as_ref(), 1);
        self.mark_cg(&text);
    }

    pub fn snr_multi_load_pdt(&mut self, source: i32) {
        let mut item = MacroItem::new(0x97);
        item.files.push(source.to_string().into_bytes());
        self.pending_macro = Some(item);
        self.gfx.all_copy(source, 1, 0);
    }

    pub fn snr_load_copy(
        &mut self,
        name: &[u8],
        rect: Rect,
        dx: i32,
        dy: i32,
        destination: i32,
        method: u8,
    ) {
        let text = nls::decode(name);
        if let Some(item) = self.pending_macro.as_mut() {
            let layer = item.files.len() - 1;
            item.files.push(name.to_vec());
            let base = 26 + layer * 8;
            if base + 7 < item.args.len() {
                if method == 1 {
                    item.args[base] = 0;
                } else {
                    item.args[base] = 2;
                    item.args[base + 1..base + 7]
                        .copy_from_slice(&[rect.x1, rect.y1, rect.x2, rect.y2, dx, dy]);
                }
            }
        }
        if let Some(image) = self.load_named(&text) {
            self.gfx.load_copy(&image, rect, dx, dy, destination);
        }
        self.mark_cg(&text);
    }

    /// Commits the pending multi-layer macro.
    pub fn snr_commit_multi(&mut self) {
        if let Some(item) = self.pending_macro.take() {
            self.stack_macro(item);
        }
    }

    fn copy_macro(
        &mut self,
        cmd: i32,
        rect: Rect,
        source: i32,
        dx: i32,
        dy: i32,
        destination: i32,
        extra: i32,
    ) {
        let mut item = MacroItem::new(cmd);
        item.args[26..35].copy_from_slice(&[
            rect.x1,
            rect.y1,
            rect.x2,
            rect.y2,
            source,
            dx,
            dy,
            destination,
            extra,
        ]);
        self.stack_macro(item);
    }

    pub fn snr_copy(
        &mut self,
        rect: Rect,
        source: i32,
        dx: i32,
        dy: i32,
        destination: i32,
        fade: i32,
    ) {
        self.copy_macro(0x64, rect, source, dx, dy, destination, fade);
        self.gfx.copy(rect, source, dx, dy, destination, fade);
    }

    pub fn snr_mask_copy(
        &mut self,
        rect: Rect,
        source: i32,
        dx: i32,
        dy: i32,
        destination: i32,
        fade: i32,
    ) {
        self.copy_macro(0x66, rect, source, dx, dy, destination, fade);
        self.gfx.mask_copy(rect, source, dx, dy, destination, fade);
    }

    pub fn snr_color_key_copy(
        &mut self,
        rect: Rect,
        source: i32,
        dx: i32,
        dy: i32,
        destination: i32,
        key: [i32; 3],
    ) {
        self.copy_macro(
            0x68,
            rect,
            source,
            dx,
            dy,
            destination,
            (key[0] << 16) | (key[1] << 8) | key[2],
        );
        self.gfx
            .color_key_copy(rect, source, dx, dy, destination, key);
    }

    pub fn snr_swap(&mut self, rect: Rect, source: i32, dx: i32, dy: i32, destination: i32) {
        self.copy_macro(0x6a, rect, source, dx, dy, destination, 0);
        self.gfx.swap(rect, source, dx, dy, destination);
    }

    pub fn snr_all_copy(&mut self, source: i32, destination: i32, fade: i32) {
        self.copy_macro(0x64, Rect::full(), source, 0, 0, destination, fade);
        self.gfx.all_copy(source, destination, fade);
    }

    pub fn snr_fill_rect(&mut self, rect: Rect, buffer: i32, colour: [i32; 3]) {
        let mut item = MacroItem::new(0x32);
        item.args[24] = (colour[0] << 16) | (colour[1] << 8) | colour[2];
        item.args[26..31].copy_from_slice(&[rect.x1, rect.y1, rect.x2, rect.y2, buffer]);
        self.stack_macro(item);
        self.gfx.fill_rect(rect, buffer, colour);
    }

    pub fn snr_clear_rect(&mut self, rect: Rect, buffer: i32, colour: [i32; 3]) {
        let mut item = MacroItem::new(0x32);
        item.args[24] = (colour[0] << 16) | (colour[1] << 8) | colour[2];
        item.args[26..31].copy_from_slice(&[rect.x1, rect.y1, rect.x2, rect.y2, buffer]);
        self.stack_macro(item);
        self.gfx.clear_rect(rect, buffer, colour);
    }

    pub fn snr_fade_color(&mut self, rect: Rect, buffer: i32, colour: [i32; 3], count: i32) {
        let mut item = MacroItem::new(0x4c);
        item.args[26..35].copy_from_slice(&[
            rect.x1, rect.y1, rect.x2, rect.y2, buffer, colour[0], colour[1], colour[2], count,
        ]);
        self.stack_macro(item);
        self.gfx.fade_color(rect, buffer, colour, count);
    }

    // ---- effects -------------------------------------------------------------------

    /// A `#SEL` preset primed to run from buffer 1 to 0.
    pub fn copy_sel(&self, index: i32) -> Effect {
        self.game.ini.sel(index, self.now())
    }

    /// One step of the scenario's current transition.
    pub fn run_effect(&mut self, effect: &mut Effect) {
        if effect.active() {
            self.mouse.start_pdt_draw();
        }
        let now = self.now();
        let skip = self.check_skip();
        let mut runner = std::mem::take(&mut self.effect_runner);
        let mut rng = self.rng;
        let mut random = |n: i32| {
            rng = rng
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            if n <= 0 {
                0
            } else {
                ((rng >> 33) % n as u64) as i32
            }
        };
        runner.step(effect, &mut self.gfx, now, skip, &mut random);
        self.rng = rng;
        self.effect_runner = runner;
    }

    /// The novel-mode window fade.
    pub fn window_in_effect(&mut self) -> bool {
        if !self.window_effect.active() {
            return false;
        }
        self.mouse.start_pdt_draw();
        let now = self.now();
        let skip = self.check_skip();
        let mut effect = self.window_effect;
        let mut runner = std::mem::take(&mut self.window_runner);
        runner.step(&mut effect, &mut self.gfx, now, skip, &mut |_| 0);
        self.window_runner = runner;
        self.window_effect = effect;
        true
    }

    // ---- screen shake -----------------------------------------------------------------

    pub fn shake_setup(&mut self, pattern: i32) {
        self.shake = Shake {
            pattern: pattern.clamp(0, 15) as usize,
            count: 0,
            base: self.now(),
            time: 0,
            offset: None,
        };
    }

    /// `true` once the pattern is exhausted.
    pub fn shake_step(&mut self) -> bool {
        let now = self.now();
        if now.saturating_sub(self.shake.base) > self.shake.time || self.check_skip() {
            let steps = &self.game.ini.shake[self.shake.pattern];
            if self.shake.count >= steps.len() || self.check_skip() {
                self.shake.offset = None;
                return true;
            }
            let [x, y, wait] = steps[self.shake.count];
            self.shake.base = now;
            self.shake.time = wait.max(0) as u64;
            self.shake.offset = Some((x, y));
            self.shake.count += 1;
        }
        false
    }

    // ---- animation ----------------------------------------------------------------------

    fn open_animation(&mut self, name: &str) -> Option<Rc<Avg32Animation>> {
        let animation = match self
            .game
            .resources
            .read("ANM", name)
            .and_then(Avg32Animation::parse)
        {
            Ok(animation) => Rc::new(animation),
            Err(error) => {
                self.warn(format!("ANM {name}: {error:#}"));
                return None;
            }
        };
        // `?`/`*` animate straight out of the working buffer (PDT1).
        let source = animation.source_pdt.clone();
        let image = self.load_named(&source);
        if image.is_some() || Self::is_working_name(source.as_bytes()) {
            self.gfx.load_file(image.as_ref(), ANMPDT as i32);
        }
        Some(animation)
    }

    fn anim_item(animation: &Avg32Animation, seen: usize, now: u64) -> Option<AnimItem> {
        let end_stream = animation.scene_stream_count(seen).ok()?;
        let frames = animation.stream_frames(seen, 0).unwrap_or_default();
        Some(AnimItem {
            seen,
            stream: 0,
            frame: 0,
            end_stream,
            frames,
            prev_time: now,
            wait: 0,
        })
    }

    /// Starts a single animation.
    pub fn animation_setup(&mut self, name: &str, seen: i32) -> bool {
        let Some(animation) = self.open_animation(name) else {
            self.anim.data = None;
            return false;
        };
        let now = self.now();
        let Some(item) = Self::anim_item(&animation, seen.max(0) as usize, now) else {
            return false;
        };
        self.anim.single = Some(item);
        self.anim.data = Some(animation);
        self.anim.name = name.to_owned();
        true
    }

    /// Advances an animation item; `false` once a single-shot item finishes.
    fn anim_advance(
        item: &mut AnimItem,
        animation: &Avg32Animation,
        now: u64,
        looping: bool,
    ) -> Option<crate::animation::AnimationCell> {
        if item.frame >= item.frames.len() {
            item.frame = 0;
            item.stream += 1;
            if item.stream >= item.end_stream {
                if !looping {
                    return None;
                }
                item.stream = 0;
            }
            item.frames = animation
                .stream_frames(item.seen, item.stream)
                .unwrap_or_default();
        }
        let _ = now;
        let cell = item.frames.get(item.frame).copied();
        item.frame += 1;
        cell.or(Some(crate::animation::AnimationCell {
            source_rect: [0, 0, -1, -1],
            destination: [0, 0],
            wait_microseconds: 0,
        }))
    }

    /// `true` once the animation has finished.
    pub fn animation_exec(&mut self) -> bool {
        let Some(animation) = self.anim.data.clone() else {
            return true;
        };
        let Some(mut item) = self.anim.single.take() else {
            return true;
        };
        let now = self.now();
        if now.saturating_sub(item.prev_time) < item.wait && !self.check_skip() {
            self.anim.single = Some(item);
            return false;
        }
        item.prev_time += item.wait;
        if item.prev_time + 1000 < now {
            item.prev_time = now;
        }
        match Self::anim_advance(&mut item, &animation, now, false) {
            None => {
                self.anim.data = None;
                true
            }
            Some(cell) => {
                item.wait = u64::from(cell.wait_microseconds);
                self.draw_cell(cell);
                self.anim.single = Some(item);
                false
            }
        }
    }

    fn draw_cell(&mut self, cell: crate::animation::AnimationCell) {
        let [x1, y1, x2, y2] = cell.source_rect;
        if x2 >= x1 && y2 >= y1 {
            self.gfx.copy(
                Rect::new(x1, y1, x2, y2),
                ANMPDT as i32,
                cell.destination[0],
                cell.destination[1],
                0,
                0,
            );
        }
    }

    /// Starts a multi-stream animation.
    pub fn multi_animation_setup(&mut self, name: &str, seen: i32) {
        let seen = seen.max(0) as usize;
        if self.anim.multi.iter().any(|item| item.seen == seen) {
            return;
        }
        if !self.anim.name.eq_ignore_ascii_case(name) {
            self.anim.data = None;
        }
        if self.anim.data.is_none() {
            let Some(animation) = self.open_animation(name) else {
                return;
            };
            self.anim.data = Some(animation);
            self.anim.multi.clear();
            self.anim.name = name.to_owned();
        }
        let now = self.now();
        let animation = self.anim.data.clone().expect("opened above");
        if self.anim.multi.len() < 64 {
            if let Some(item) = Self::anim_item(&animation, seen, now) {
                self.anim.multi.push(item);
            }
        }
    }

    /// Advances the multi-stream animation.
    pub fn multi_animation_exec(&mut self) {
        let Some(animation) = self.anim.data.clone() else {
            return;
        };
        let now = self.now();
        let mut items = std::mem::take(&mut self.anim.multi);
        for item in &mut items {
            if now.saturating_sub(item.prev_time) < item.wait {
                continue;
            }
            item.prev_time = now;
            if let Some(cell) = Self::anim_advance(item, &animation, now, true) {
                item.wait = u64::from(cell.wait_microseconds);
                self.draw_cell(cell);
            }
        }
        self.anim.multi = items;
    }

    pub fn multi_animation_clear(&mut self) {
        self.anim.data = None;
        self.anim.name.clear();
        self.anim.multi.clear();
    }

    pub fn multi_animation_stop(&mut self, seen: i32) {
        self.anim.multi.retain(|item| item.seen as i32 != seen);
    }

    // ---- names / titles -------------------------------------------------------------

    pub fn set_window_title(&mut self, title: Vec<u8>) {
        let caption = self.game.ini.caption.clone();
        let text = nls::decode(&title);
        self.window_title = if text.is_empty() {
            caption
        } else {
            format!("{caption} {text}")
        };
        self.title = title;
    }

    pub fn name(&self, index: i32) -> &[u8] {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.names.get(index))
            .map_or(&[], Vec::as_slice)
    }

    pub fn set_name(&mut self, index: i32, value: &[u8]) {
        if let Some(slot) = usize::try_from(index)
            .ok()
            .and_then(|index| self.names.get_mut(index))
        {
            *slot = value
                .iter()
                .copied()
                .take_while(|byte| *byte != 0)
                .take(15)
                .collect();
        }
    }

    pub fn open_name_input(&mut self, titles: [Vec<u8>; 2], indices: [i32; 2]) {
        let values = std::array::from_fn(|field| {
            if indices[field] > 0 {
                nls::decode(self.name(indices[field] - 1))
            } else {
                String::new()
            }
        });
        self.name_input = Some(NameInput {
            titles: titles.map(|title| nls::decode(&title)),
            indices,
            values,
            focus: 0,
            inline_box: None,
        });
    }

    // ---- menus -------------------------------------------------------------------------

    pub fn open_context_menu(&mut self) {
        self.refresh_slots();
        self.menu = Some(MenuState::context(self));
    }

    pub fn open_load_picker(&mut self) {
        self.refresh_slots();
        self.menu_result = None;
        self.menu = Some(MenuState::load_picker(self));
    }

    pub fn menu_enable(&mut self, item: i32, enabled: bool) {
        if let Some(slot) = usize::try_from(item)
            .ok()
            .and_then(|item| self.menu_enabled.get_mut(item))
        {
            *slot = enabled;
        }
    }

    pub fn menu_enabled(&self, item: i32) -> bool {
        usize::try_from(item)
            .ok()
            .and_then(|item| self.menu_enabled.get(item))
            .copied()
            .unwrap_or(false)
    }

    // ---- save / load ------------------------------------------------------------------------

    fn banner(&self) -> Vec<u8> {
        nls::encode(&self.game.ini.save_header)
    }

    pub fn refresh_slots(&mut self) {
        let count = self.game.ini.save_file_count.clamp(1, 32) as usize;
        self.slots = if self.persist {
            savedata::list_slots(&self.save_path, self.version, count)
        } else {
            vec![SlotSummary::default(); count]
        };
    }

    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    pub fn load_global_flags(&mut self) {
        if !self.persist {
            return;
        }
        let banner = self.banner();
        let Some(global) = savedata::load_global(&self.save_path, self.version, &banner) else {
            return;
        };
        for (index, value) in global.values.iter().enumerate() {
            self.flags.set_value(1000 + index as i32, *value);
        }
        for (index, bit) in global.bits.iter().enumerate() {
            self.flags.set_bit(1000 + index as i32, *bit);
        }
        if (1..=3).contains(&global.window_style) {
            self.mes.style = global.window_style;
        }
        if let Some(names) = global.names {
            self.names = names;
        }
    }

    pub fn save_global_flags(&mut self) {
        if !self.persist {
            return;
        }
        let banner = self.banner();
        if let Err(error) = savedata::save_global(
            &self.save_path,
            self.version,
            &banner,
            &self.flags,
            self.mes.style,
            &self.names,
        ) {
            self.warn(format!("save global flags: {error:#}"));
        }
    }

    /// Saves to a slot (0-based).
    pub fn save(&mut self, slot: i32, seen: i32, position: i32) {
        if !(0..32).contains(&slot) || !self.persist {
            return;
        }
        self.save_global_flags();
        let date = self.date_time(1);
        let time = self.date_time(2);
        let data = SlotData {
            summary: SlotSummary {
                valid: true,
                month: date / 100,
                day: date % 100,
                hour: time / 100,
                minute: time % 100,
                title: self.title.clone(),
            },
            year: self.date_time(3),
            seen,
            position,
            bgm: nls::encode(self.sound.current_bgm()),
            stack: self.flags.saved_stack().to_vec(),
            macros: self.macros.clone(),
            window_style_force: self.mes.style_force,
            ..SlotData::default()
        };
        let banner = self.banner();
        if let Err(error) = savedata::save_slot(
            &self.save_path,
            self.version,
            &banner,
            slot as usize,
            &self.flags,
            &data,
        ) {
            self.warn(format!("save slot {slot}: {error:#}"));
        }
        self.refresh_slots();
    }

    /// Loads a slot (0-based): restores variables and schedules
    /// the screen rebuild; returns the resume point.
    pub fn load(&mut self, slot: i32) -> Option<(i32, i32)> {
        self.save_global_flags();
        let data = savedata::load_slot(&self.save_path, self.version, usize::try_from(slot).ok()?)?;
        for (index, value) in data.values.iter().enumerate() {
            self.flags.set_value(index as i32, *value);
        }
        for (index, bit) in data.bits.iter().enumerate() {
            self.flags.set_bit(index as i32, *bit);
        }
        for (index, string) in data.strings.iter().enumerate() {
            self.flags.set_string(index as i32, string);
        }
        self.flags.restore_stack(data.stack.clone());
        self.macros = data.macros.clone();
        self.mes.style_force = data.window_style_force;
        self.loading = Some(Loading {
            count: 0,
            time: self.now(),
            effect: Effect::default(),
            title: data.summary.title.clone(),
            bgm: data.bgm.clone(),
        });
        Some((data.seen, data.position))
    }

    /// `true` once the restored screen is showing.
    pub fn loading_proc(&mut self) -> bool {
        let Some(mut loading) = self.loading.take() else {
            return true;
        };
        let now = self.now();
        if loading.count == 0 {
            self.mouse.start_pdt_draw();
        }
        if loading.count < 16 {
            if now.saturating_sub(loading.time) >= 50 || self.check_skip() {
                loading.time = now;
                self.screen_fade(1, loading.count, [0, 0, 0]);
                loading.count += 1;
            }
        } else if loading.count == 16 {
            self.gfx.present_enabled = false;
            self.redraw_macros();
            self.gfx.all_copy(0, BACKUPPDT as i32, 0);
            self.gfx.fill_rect(Rect::full(), 0, [0, 0, 0]);
            self.gfx.present_enabled = true;
            let title = loading.title.clone();
            self.set_window_title(title);
            loading.count += 1;
            loading.effect = Effect {
                sx2: 639,
                sy2: 479,
                cmd: 4,
                steptime: 50,
                prevtime: now,
                srcpdt: BACKUPPDT as i32,
                dstpdt: 0,
                step: 1,
                ..Effect::default()
            };
        } else {
            let mut effect = loading.effect;
            self.run_effect(&mut effect);
            loading.effect = effect;
            if !effect.active() {
                let bgm = nls::decode(&loading.bgm);
                if !bgm.is_empty() {
                    self.bgm_play(&bgm, true, 0);
                }
                return true;
            }
        }
        self.loading = Some(loading);
        false
    }

    // ---- sound helpers -----------------------------------------------------------------------

    pub fn bgm_play(&mut self, name: &str, looped: bool, fade_ms: u32) {
        let (ini, resources) = (&self.game.ini, &self.game.resources);
        self.sound.bgm_play(ini, resources, name, looped, fade_ms);
    }

    pub fn wav_play(&mut self, name: &str, looped: bool, channel: Option<i32>) {
        let (ini, resources) = (&self.game.ini, &self.game.resources);
        self.sound.wav_play(ini, resources, name, looped, channel);
    }

    /// SE 0-3 also sound automatically (cursor move,
    /// decision, window open, page advance).
    pub fn play_se(&mut self, index: i32) {
        if self.check_skip() && index >= 2 {
            return;
        }
        let (ini, resources) = (&self.game.ini, &self.game.resources);
        self.sound.se_play(ini, resources, index);
    }

    pub fn koe_play(&mut self, id: i32) {
        self.last_voice = Some(id);
        let version = self.version;
        self.sound.koe_play(&self.game.ini, version, id);
    }

    // ---- presentation -----------------------------------------------------------------------

    /// The frame to show: the presented screen, shifted while shaking, with
    /// system overlays (menus, name entry) on top.
    pub fn frame_rgba(&mut self) -> Vec<u8> {
        let width = AVG32_WIDTH as usize;
        let height = AVG32_HEIGHT as usize;
        let screen = self.gfx.screen();
        let mut rgba = vec![0u8; width * height * 4];
        let (ox, oy) = self.shake.offset.unwrap_or((0, 0));
        for y in 0..height {
            let sy = y as i32 - oy;
            if !(0..height as i32).contains(&sy) {
                continue;
            }
            for x in 0..width {
                let sx = x as i32 - ox;
                if !(0..width as i32).contains(&sx) {
                    continue;
                }
                let s = (sy as usize * width + sx as usize) * 3;
                let d = (y * width + x) * 4;
                rgba[d..d + 3].copy_from_slice(&screen[s..s + 3]);
                rgba[d + 3] = 255;
            }
        }
        for pixel in rgba.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        crate::menu::draw_overlays(self, &mut rgba);
        if let Some(cursor) = &self.cursor {
            if self.mouse.visible && self.mouse.x >= 0 && self.mouse.y >= 0 {
                let frame = (self.now() / 150) as usize;
                cursor.draw(&mut rgba, self.mouse.x, self.mouse.y, frame);
            }
        }
        rgba
    }

    /// Back to a clean presentation state (return to menu,
    /// load).
    pub fn reset(&mut self) {
        self.anim = Animation::default();
        self.mes.reset();
        self.sel = Selection::default();
        self.skip_enabled = true;
        self.mes.sub_window = false;
        let style = self.mes.style;
        self.mes_setup(style);
        self.reset_timer();
        self.sound.wav_stop(None);
        self.sound.koe_stop();
        self.shake.offset = None;
    }
}
