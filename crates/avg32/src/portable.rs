//! Device-free service boundary for the original AVG32 Scenario interpreter.
//!
//! This module contains native policy/state and commands only. No framebuffers,
//! fonts, image codecs, mixers, filesystems, movies or transition algorithms.
use crate::{flags::Flags, nls::Nls};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Event {
    Message(Vec<u8>),
    Print {
        all: bool,
    },
    ClearMessage,
    HideMessage,
    LineFeed,
    MessageIcon(Option<i32>),
    LoadImage {
        name: Vec<u8>,
        buffer: i32,
    },
    ChoiceClear,
    Choice {
        text: Vec<u8>,
        enabled: i32,
        color: i32,
    },
    ChoiceWindow(bool),
    Title(Vec<u8>),
    SoundEffect(i32),
    Audio(AudioEvent),
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MessagePolicy {
    pub double_text: bool,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Ini {
    pub mes_wait: i32,
    pub music_type: i32,
}
impl Ini {
    pub fn value(&self, _key: &str) -> i32 {
        0
    }
}
pub use crate::input::Mouse;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct System {
    pub flags: Flags,
    pub running: bool,
    pub version: i32,
    pub mouse: Mouse,
    pub mes: MessagePolicy,
    pub sound: Sound,
    pub menu: Option<()>,
    pub menu_result: Option<i32>,
    pub events: VecDeque<Event>,
    pub error: Option<String>,
    pub skip_enabled: bool,
    pub skip_pressed: bool,
    pub time: u64,
    pub timer_base: u64,
    pub random_seed: u64,
    pub ini: Ini,
    /// Encoding is scoped to each run, not shared between client VMs.
    pub nls: Nls,
    #[serde(skip)]
    pub scenes: BTreeMap<i32, Vec<u8>>,
    pub choice_result: Option<i32>,
    pub frame_yielded: bool,
    pub instructions: u64,
}
impl Default for System {
    fn default() -> Self {
        Self {
            flags: Flags::default(),
            running: true,
            version: 1714,
            mouse: Mouse::default(),
            mes: MessagePolicy::default(),
            sound: Sound::default(),
            menu: None,
            menu_result: None,
            events: VecDeque::new(),
            error: None,
            skip_enabled: true,
            skip_pressed: false,
            time: 0,
            timer_base: 0,
            random_seed: 1,
            ini: Ini::default(),
            nls: Nls::default(),
            scenes: BTreeMap::new(),
            choice_result: None,
            frame_yielded: false,
            instructions: 0,
        }
    }
}
impl System {
    pub fn has_events(&self) -> bool {
        !self.events.is_empty() || !self.sound.events.is_empty()
    }
    pub fn next_event(&mut self) -> Option<Event> {
        self.events
            .pop_front()
            .or_else(|| self.sound.events.pop_front().map(Event::Audio))
    }
    pub fn bgm_play(&mut self, name: &str, looping: bool, fade_ms: u32) {
        self.sound.events.push_back(AudioEvent::Bgm {
            name: name.into(),
            looping,
            fade_ms,
        });
    }
    pub fn wav_play(&mut self, name: &str, looping: bool, channel: Option<i32>) {
        self.sound.events.push_back(AudioEvent::Wave {
            name: name.into(),
            looping,
            channel: channel.unwrap_or(-1),
        });
    }
    pub fn koe_play(&mut self, id: i32) {
        self.sound.events.push_back(AudioEvent::Voice(id));
    }
    pub fn read_seen(&self, seen: i32) -> Result<Vec<u8>> {
        match self.scenes.get(&seen) {
            Some(bytes) => Ok(bytes.clone()),
            None => bail!("unavailable native scene {seen}"),
        }
    }
    pub fn now(&self) -> u64 {
        self.time
    }
    pub fn ini(&self) -> &Ini {
        &self.ini
    }
    pub fn timer(&self) -> i32 {
        self.time.saturating_sub(self.timer_base) as i32
    }
    pub fn reset_timer(&mut self) {
        self.timer_base = self.time;
    }
    pub fn window_in_effect(&self) -> bool {
        false
    }
    pub fn check_skip(&self) -> bool {
        self.skip_enabled && self.skip_pressed
    }
    /// This profile has no novel mode: native check_novel_save is always true.
    pub fn check_novel_save(&self) -> bool {
        true
    }
    pub fn warn(&mut self, warning: String) {
        self.error = Some(warning);
        self.running = false;
    }
    pub fn unsupported(&mut self, opcode: u8, position: i32) {
        self.warn(format!(
            "AVG32 opcode {opcode:#04x} at {position:#x} has no portable host mapping"
        ));
    }
    pub fn date_time(&mut self, _part: i32) -> i32 {
        self.warn("AVG32 calendar query requires a host mapping".into());
        0
    }
    pub fn random(&mut self, low: i32, high: i32) -> i32 {
        crate::rng::random(&mut self.random_seed, low, high)
    }
    pub fn mes_set(&mut self, packet: &[u8]) {
        self.mouse.start_pdt_draw();
        self.events.push_back(Event::Message(packet.to_vec()));
    }
    pub fn mes_print(&mut self) -> i32 {
        self.print(false)
    }
    pub fn mes_print_all(&mut self) -> i32 {
        self.print(true)
    }
    fn print(&mut self, all: bool) -> i32 {
        self.events.push_back(Event::Print { all });
        1
    }
    pub fn mes_clear_window(&mut self) {
        self.events.push_back(Event::ClearMessage);
    }
    pub fn mes_clear(&mut self) {
        self.events.push_back(Event::ClearMessage);
    }
    pub fn mes_hide(&mut self) {
        self.events.push_back(Event::HideMessage);
    }
    pub fn mes_line_feed(&mut self) -> i32 {
        self.mes.double_text = false;
        self.events.push_back(Event::LineFeed);
        0
    }
    pub fn mes_draw_icon(&mut self, kind: i32) {
        self.events.push_back(Event::MessageIcon(Some(kind)));
    }
    pub fn mes_hide_icon(&mut self) {
        self.events.push_back(Event::MessageIcon(None));
    }
    pub fn play_se(&mut self, id: i32) {
        if !self.check_skip() || id < 2 {
            self.events.push_back(Event::SoundEffect(id));
        }
    }
    pub fn snr_load_file(&mut self, name: &[u8], buffer: i32) {
        self.events.push_back(Event::LoadImage {
            name: name.to_vec(),
            buffer,
        });
    }
    pub fn select_clear(&mut self) {
        self.events.push_back(Event::ChoiceClear);
    }
    pub fn select_add_item(&mut self, text: &[u8], enabled: i32, color: i32) {
        self.events.push_back(Event::Choice {
            text: text.to_vec(),
            enabled,
            color,
        });
    }
    pub fn select_sub_window_setup(&mut self) {
        self.events.push_back(Event::ChoiceWindow(true));
    }
    pub fn select_sub_window_close(&mut self) {
        self.events.push_back(Event::ChoiceWindow(false));
    }
    pub fn select(&mut self) -> i32 {
        self.choice_result.take().unwrap_or(-1)
    }
    pub fn open_load_picker(&mut self) {
        self.warn("AVG32 load picker requires a host mapping".into());
    }
    pub fn set_window_title(&mut self, title: Vec<u8>) {
        self.events.push_back(Event::Title(title));
    }
}

/// Native audio channel policy. Resource bytes and playing handles stay in the host.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AudioEvent {
    Bgm {
        name: String,
        looping: bool,
        fade_ms: u32,
    },
    StopBgm {
        fade_ms: u32,
    },
    Wave {
        name: String,
        looping: bool,
        channel: i32,
    },
    StopWave {
        channel: Option<i32>,
    },
    Voice(i32),
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Sound {
    pub bgm_active: bool,
    pub voice_active: bool,
    pub wave_active: bool,
    pub events: VecDeque<AudioEvent>,
}
impl Sound {
    pub fn bgm_playing(&self) -> bool {
        self.bgm_active
    }
    pub fn koe_playing(&self) -> bool {
        self.voice_active
    }
    pub fn wav_playing(&self) -> bool {
        self.wave_active
    }
    pub fn bgm_stop(&mut self, fade_ms: u32) {
        self.events.push_back(AudioEvent::StopBgm { fade_ms });
    }
    pub fn wav_stop(&mut self, channel: Option<i32>) {
        self.events.push_back(AudioEvent::StopWave { channel });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Scenario;
    fn scene(code: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0; 0x63];
        bytes[..5].copy_from_slice(b"TPC32");
        bytes[0x28..0x2c].copy_from_slice(&1i32.to_le_bytes());
        bytes[0x5f..0x63].copy_from_slice(&1i32.to_le_bytes());
        bytes.extend_from_slice(code);
        bytes
    }
    fn number(value: u8) -> u8 {
        assert!(value < 16);
        0x10 | value
    }
    fn host(code: &[u8]) -> System {
        let mut sys = System::default();
        sys.version = 1604;
        sys.scenes.insert(1, scene(code));
        sys
    }
    #[test]
    fn original_arithmetic_cross_scene_calls_and_waits_use_the_portable_host() {
        // V1=7; gosub SEEN2; wait 10 ms; V3=3; quit.
        let mut sys = host(&[
            0x3b,
            number(1),
            number(7),
            0x16,
            0x02,
            number(2),
            0x19,
            0x01,
            number(10),
            0x3b,
            number(3),
            number(3),
            0x60,
            0x20,
        ]);
        sys.scenes
            .insert(2, scene(&[0x3c, number(1), number(2), 0x20, 0x01]));
        let mut vm = Scenario::new(&mut sys, 1);
        vm.run_budget(&mut sys, 100);
        assert!(sys.error.is_none(), "{:?}", sys.error);
        assert_eq!(sys.flags.value(1), 9);
        assert_eq!(sys.flags.value(3), 0);
        assert!(sys.flags.stack().is_empty());
        sys.time = 9;
        vm.run_budget(&mut sys, 100);
        assert_eq!(sys.flags.value(3), 0);
        sys.time = 10;
        vm.run_budget(&mut sys, 100);
        vm.run_budget(&mut sys, 100);
        assert_eq!(sys.flags.value(3), 3);
        assert!(!sys.running);
    }
    #[test]
    fn original_text_image_and_sound_parsers_emit_owned_operations_and_wait_for_ack() {
        let mut sys = host(&[
            0x0b,
            0x09,
            b'b',
            b'g',
            0,
            number(0),
            0x0e,
            0x30,
            b's',
            b'e',
            0,
            0xfe,
            b'A',
            b'B',
            0,
            0x60,
            0x20,
        ]);
        let mut vm = Scenario::new(&mut sys, 1);
        vm.run_budget(&mut sys, 100);
        let position = vm.pos;
        vm.run_budget(&mut sys, 100);
        assert_eq!(vm.pos, position);
        assert_eq!(sys.next_event(), Some(Event::HideMessage));
        assert_eq!(
            sys.next_event(),
            Some(Event::LoadImage {
                name: b"bg".to_vec(),
                buffer: 0
            })
        );
        vm.run_budget(&mut sys, 100);
        assert_eq!(
            sys.next_event(),
            Some(Event::Audio(AudioEvent::Wave {
                name: "se".into(),
                looping: false,
                channel: -1
            }))
        );
        vm.run_budget(&mut sys, 100);
        assert_eq!(
            sys.next_event(),
            Some(Event::Message(vec![0xfe, b'A', b'B']))
        );
        vm.run_budget(&mut sys, 100);
        assert_eq!(sys.next_event(), Some(Event::Print { all: false }));
        vm.complete_message_print(0).unwrap();
        vm.run_budget(&mut sys, 100);
        assert!(!sys.running);
        assert!(sys.error.is_none(), "{:?}", sys.error);
    }
    #[test]
    fn unsupported_operations_fail_visibly_and_do_not_fall_back_to_an_engine() {
        for code in [&[0x13, 1][..], &[0x60, 0x03, 0x11][..], &[0x0e, 0x50][..]] {
            let mut sys = host(code);
            let mut vm = Scenario::new(&mut sys, 1);
            vm.run_budget(&mut sys, 100);
            assert!(!sys.running);
            assert!(sys.error.is_some());
        }
    }
    #[test]
    fn native_mouse_expiry_and_consumption_are_shared_with_the_original_host() {
        let mut mouse = Mouse::default();
        mouse.press(10, 20, 0, 5);
        assert!(mouse.button(204));
        assert!(!mouse.button(204));
        mouse.press(10, 20, 0, 5);
        assert!(!mouse.button(205));
    }
    #[test]
    fn client_encoding_scope_restores_on_unwind_and_is_thread_local() {
        crate::nls::set(Nls::Sjis);
        let _ = std::panic::catch_unwind(|| {
            let _scope = crate::nls::ScopedNls::enter(Nls::Utf8);
            panic!("test scope");
        });
        assert_eq!(crate::nls::current(), Nls::Sjis);
        std::thread::scope(|scope| {
            let a = scope.spawn(|| {
                let _scope = crate::nls::ScopedNls::enter(Nls::Utf8);
                assert_eq!(crate::nls::decode(b"abc"), "abc");
                crate::nls::current()
            });
            let b = scope.spawn(|| {
                let _scope = crate::nls::ScopedNls::enter(Nls::Gbk);
                crate::nls::current()
            });
            assert_eq!(a.join().unwrap(), Nls::Utf8);
            assert_eq!(b.join().unwrap(), Nls::Gbk);
        });
    }
}
