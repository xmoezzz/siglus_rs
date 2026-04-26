use crate::error::Result;

#[derive(Clone)]
pub struct Ac3AudioChunk {
    pub pts_ms: i64,
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

pub struct Ac3AudioDecoder {
    buf: Vec<u8>,
    state: oxideav_ac3::audblk::Ac3State,
    next_pts_ms: Option<i64>,
}

impl Default for Ac3AudioDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Ac3AudioDecoder {
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            state: oxideav_ac3::audblk::Ac3State::new(),
            next_pts_ms: None,
        }
    }

    pub fn push_with<F>(&mut self, data: &[u8], pts_ms: Option<i64>, mut on_chunk: F) -> Result<()>
    where
        F: FnMut(Ac3AudioChunk),
    {
        if let Some(pts) = pts_ms {
            self.next_pts_ms = Some(pts);
        }
        self.buf.extend_from_slice(data);

        let mut pos = 0usize;
        while pos + 5 <= self.buf.len() {
            let Some(sync_pos) = find_syncword(&self.buf, pos) else {
                // Keep at most one trailing byte in case it is the first half of 0x0B77.
                if !self.buf.is_empty() {
                    let keep = if *self.buf.last().unwrap() == 0x0B { 1 } else { 0 };
                    let drain_to = self.buf.len().saturating_sub(keep);
                    if drain_to > 0 {
                        self.buf.drain(0..drain_to);
                    }
                }
                return Ok(());
            };

            if sync_pos > pos && (std::env::var_os("SG_MOVIE_TRACE").is_some()
                || std::env::var_os("SG_DEBUG").is_some())
            {
                eprintln!(
                    "[SG_DEBUG][MOV] ac3.resync.drop bytes={}",
                    sync_pos - pos
                );
            }
            pos = sync_pos;

            let si = match oxideav_ac3::syncinfo::parse(&self.buf[pos..]) {
                Ok(si) => si,
                Err(err) => {
                    if std::env::var_os("SG_MOVIE_TRACE").is_some()
                        || std::env::var_os("SG_DEBUG").is_some()
                    {
                        eprintln!("[SG_DEBUG][MOV] ac3.syncinfo.drop: {err}");
                    }
                    pos += 1;
                    continue;
                }
            };

            let frame_len = si.frame_length as usize;
            if frame_len == 0 {
                pos += 1;
                continue;
            }
            if pos + frame_len > self.buf.len() {
                break;
            }

            let frame = self.buf[pos..pos + frame_len].to_vec();
            pos += frame_len;

            let bsi = match oxideav_ac3::bsi::parse(&frame[5..]) {
                Ok(bsi) => bsi,
                Err(err) => {
                    if std::env::var_os("SG_MOVIE_TRACE").is_some()
                        || std::env::var_os("SG_DEBUG").is_some()
                    {
                        eprintln!("[SG_DEBUG][MOV] ac3.bsi.drop: {err}");
                    }
                    continue;
                }
            };

            let channels = bsi.nchans as u16;
            let sample_rate = si.sample_rate;
            if channels == 0 || sample_rate == 0 {
                continue;
            }

            let sample_count = oxideav_ac3::audblk::BLOCKS_PER_FRAME
                * oxideav_ac3::audblk::SAMPLES_PER_BLOCK
                * channels as usize;
            let mut samples = vec![0.0f32; sample_count];
            if let Err(err) = oxideav_ac3::audblk::decode_frame(
                &mut self.state,
                &si,
                &bsi,
                &frame,
                &mut samples,
            ) {
                if std::env::var_os("SG_MOVIE_TRACE").is_some()
                    || std::env::var_os("SG_DEBUG").is_some()
                {
                    eprintln!("[SG_DEBUG][MOV] ac3.decode.drop: {err}");
                }
                continue;
            }

            let pts0 = self.next_pts_ms.unwrap_or(0);
            on_chunk(Ac3AudioChunk {
                pts_ms: pts0,
                sample_rate,
                channels,
                samples,
            });

            let frame_ms = ((oxideav_ac3::audblk::BLOCKS_PER_FRAME
                * oxideav_ac3::audblk::SAMPLES_PER_BLOCK) as i64
                * 1000)
                / sample_rate as i64;
            self.next_pts_ms = Some(pts0 + frame_ms);
        }

        if pos > 0 {
            self.buf.drain(0..pos);
        }
        Ok(())
    }
}

fn find_syncword(buf: &[u8], from: usize) -> Option<usize> {
    if buf.len() < 2 || from >= buf.len().saturating_sub(1) {
        return None;
    }
    let mut i = from;
    while i + 1 < buf.len() {
        if buf[i] == 0x0B && buf[i + 1] == 0x77 {
            return Some(i);
        }
        i += 1;
    }
    None
}
