//! GAN animation support (ported from the original the original implementation implementation).

use anyhow::{bail, Context, Result};
use encoding_rs::SHIFT_JIS;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
pub struct GanPat {
    pub pat_no: i32,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub tr: u8,
    pub wait: i32,
    pub keika_time: i32,
}

#[derive(Debug, Clone, Default)]
pub struct GanSet {
    pub pat_list: Vec<GanPat>,
    pub total_time: i32,
}

#[derive(Debug, Clone, Default)]
pub struct GanData {
    pub g00_file_name: String,
    pub set_list: Vec<GanSet>,
}

impl GanData {
    pub fn load(path: &Path) -> Result<Self> {
        let buf = std::fs::read(path).with_context(|| format!("read gan: {:?}", path))?;
        if buf.len() < 8 {
            bail!("gan too short: {:?}", path);
        }
        let mut data = GanData::default();
        data.analize(&buf)?;
        Ok(data)
    }

    fn analize(&mut self, buf: &[u8]) -> Result<()> {
        let mut sp = 0usize;
        let code = read_i32(buf, &mut sp)?;
        if code != 10000 {
            bail!("gan bad version code: {code}");
        }
        let version = read_i32(buf, &mut sp)?;
        if version != 10000 {
            bail!("gan unsupported version: {version}");
        }

        loop {
            let code = read_i32(buf, &mut sp)?;
            if code == 10100 {
                let len = read_i32(buf, &mut sp)? as usize;
                let s = read_bytes(buf, &mut sp, len)?;
                self.g00_file_name = decode_sjis(s);
                continue;
            }
            if code == 20000 {
                let set_cnt = read_i32(buf, &mut sp)?;
                if set_cnt > 0 {
                    for _ in 0..set_cnt {
                        let mut set = GanSet::default();
                        let mut keika_time = 0i32;
                        analize_set(buf, &mut sp, &mut set, &mut keika_time)?;
                        self.set_list.push(set);
                    }
                }
                return Ok(());
            }

            bail!("gan unexpected code: {code}");
        }
    }
}

fn analize_set(buf: &[u8], sp: &mut usize, set: &mut GanSet, keika_time: &mut i32) -> Result<()> {
    let code = read_i32(buf, sp)?;
    if code != 30000 {
        bail!("gan set missing PAT_COUNT: {code}");
    }
    let pat_cnt = read_i32(buf, sp)?;
    for _ in 0..pat_cnt {
        let pat = analize_pat(buf, sp, keika_time)?;
        set.pat_list.push(pat);
    }
    set.total_time = *keika_time;
    Ok(())
}

fn analize_pat(buf: &[u8], sp: &mut usize, keika_time: &mut i32) -> Result<GanPat> {
    let mut pat = GanPat {
        tr: 255,
        ..Default::default()
    };
    loop {
        let code = read_i32(buf, sp)?;
        if code == 999999 {
            break;
        }
        match code {
            30100 => pat.pat_no = read_i32(buf, sp)?,
            30101 => pat.x = read_i32(buf, sp)?,
            30102 => pat.y = read_i32(buf, sp)?,
            30103 => pat.wait = read_i32(buf, sp)?,
            30104 => {
                let v = read_i32(buf, sp)?;
                pat.tr = v.clamp(0, 255) as u8;
            }
            30105 => pat.z = read_i32(buf, sp)?,
            _ => bail!("gan unexpected pat code: {code}"),
        }
    }
    *keika_time += pat.wait;
    pat.keika_time = *keika_time;
    Ok(pat)
}

fn decode_sjis(bytes: &[u8]) -> String {
    let (cow, _, had_err) = SHIFT_JIS.decode(bytes);
    if had_err {
        String::from_utf8_lossy(bytes).into_owned()
    } else {
        cow.into_owned()
    }
}

fn read_i32(buf: &[u8], sp: &mut usize) -> Result<i32> {
    let bytes = read_bytes(buf, sp, 4)?;
    Ok(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_bytes<'a>(buf: &'a [u8], sp: &mut usize, len: usize) -> Result<&'a [u8]> {
    if *sp + len > buf.len() {
        bail!("gan out of range");
    }
    let out = &buf[*sp..*sp + len];
    *sp += len;
    Ok(out)
}

#[derive(Debug, Default, Clone)]
pub struct GanState {
    data: Option<Arc<GanData>>,
    current_pat: Option<GanPat>,

    gan_name: String,
    now_time: i32,
    anm_set_no: i32,
    next_anm_set_no: i32,

    anm_start: bool,
    anm_pause: bool,
    anm_loop_flag: bool,
    anm_real_time_flag: bool,
    next_anm_flag: bool,
    next_anm_loop_flag: bool,
    next_anm_real_time_flag: bool,
}

impl GanState {
    pub fn reset(&mut self) {
        *self = GanState::default();
    }

    pub fn current_pat(&self) -> Option<&GanPat> {
        self.current_pat.as_ref()
    }

    pub fn load_gan(&mut self, project_dir: &Path, append_dir: &str, name: &str) -> Result<()> {
        self.reset();
        self.load_gan_only(project_dir, append_dir, name)
    }

    pub fn load_gan_only(&mut self, project_dir: &Path, append_dir: &str, name: &str) -> Result<()> {
        if name.trim().is_empty() {
            return Ok(());
        }
        self.gan_name = name.to_string();
        let path = resolve_gan_path(project_dir, append_dir, name)
            .with_context(|| format!("resolve gan path: {name}"))?;
        let data = GanData::load(&path)?;
        self.data = Some(Arc::new(data));
        Ok(())
    }

    pub fn start_anm(&mut self, set_no: i32, loop_flag: bool, real_time_flag: bool) {
        self.now_time = 0;
        self.anm_start = true;
        self.anm_pause = false;
        self.anm_set_no = set_no;
        self.anm_loop_flag = loop_flag;
        self.anm_real_time_flag = real_time_flag;
        self.next_anm_flag = false;
    }

    pub fn next_anm(&mut self, set_no: i32, loop_flag: bool, real_time_flag: bool) {
        if self.anm_start {
            if self.next_anm_flag {
                self.start_anm(self.next_anm_set_no, false, self.next_anm_real_time_flag);
            } else {
                self.anm_loop_flag = false;
            }
            self.next_anm_flag = true;
            self.next_anm_set_no = set_no;
            self.next_anm_loop_flag = loop_flag;
            self.next_anm_real_time_flag = real_time_flag;
        } else {
            self.start_anm(set_no, loop_flag, real_time_flag);
        }
    }

    pub fn pause_anm(&mut self) {
        self.anm_pause = true;
    }

    pub fn resume_anm(&mut self) {
        self.anm_pause = false;
    }

    pub fn update_time(&mut self, past_game_time: i32, past_real_time: i32) {
        let mut game = past_game_time.max(0);
        let mut real = past_real_time.max(0);

        let Some(data) = self.data.as_ref() else {
            self.current_pat = None;
            return;
        };
        if self.anm_set_no < 0 || self.anm_set_no as usize >= data.set_list.len() {
            self.current_pat = None;
            return;
        }
        let set = &data.set_list[self.anm_set_no as usize];
        if set.pat_list.is_empty() {
            self.current_pat = None;
            return;
        }
        if !self.anm_start || set.total_time <= 0 {
            self.current_pat = Some(set.pat_list[0].clone());
            return;
        }

        if !self.anm_pause {
            if self.anm_real_time_flag {
                self.now_time += real;
            } else {
                self.now_time += game;
            }
        }

        if !self.anm_loop_flag {
            if self.now_time >= set.total_time {
                if self.next_anm_flag {
                    self.start_anm(self.next_anm_set_no, self.next_anm_loop_flag, self.next_anm_real_time_flag);
                    let overshoot = self.now_time - set.total_time;
                    game -= overshoot;
                    real -= overshoot;
                    self.update_time(game, real);
                } else {
                    self.now_time = set.total_time;
                    self.current_pat = Some(set.pat_list[set.pat_list.len() - 1].clone());
                }
                return;
            }
        }

        if set.total_time > 0 {
            self.now_time %= set.total_time;
        }

        for pat in &set.pat_list {
            if pat.keika_time >= self.now_time {
                self.current_pat = Some(pat.clone());
                break;
            }
        }
    }
}

fn resolve_gan_path(project_dir: &Path, append_dir: &str, name: &str) -> Result<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    let mut norm = name.replace('\\', "/");
    if !norm.ends_with(".gan") && !norm.contains('.') {
        norm.push_str(".gan");
    }

    let path = PathBuf::from(&norm);
    if path.is_absolute() {
        if path.exists() {
            return Ok(path);
        }
    }

    if !append_dir.is_empty() {
        candidates.push(project_dir.join(append_dir).join("gan").join(&norm));
        candidates.push(project_dir.join(append_dir).join(&norm));
    }
    candidates.push(project_dir.join("gan").join(&norm));
    candidates.push(project_dir.join(&norm));
    candidates.push(project_dir.join("dat").join(&norm));

    for cand in candidates {
        if cand.exists() {
            return Ok(cand);
        }
    }

    bail!("gan file not found: {name}")
}
