//! OMV (Ogg/Theora wrapper) parser.
//!
//! OMV stores a fixed metadata header, Theora page/packet seek tables, and the
//! raw Ogg bitstream. Native playback keeps the file open and reads Ogg pages
//! on demand; this parser follows that layout and does not read the whole OMV
//! merely to inspect its header.

use anyhow::{Context, Result, anyhow, bail};
#[cfg(any(feature="nfs", feature="smb"))]
use game_network::fs::File;
#[cfg(not(any(feature="nfs", feature="smb")))]
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

pub const OMV_THEORA_TYPE_RGB: u32 = 0;
pub const OMV_THEORA_TYPE_RGBA: u32 = 1;
pub const OMV_THEORA_TYPE_YUV: u32 = 2;

const OMV_HEADER_MIN_SIZE: usize = 0x58;
const OMV_THEORA_PAGE_SIZE: usize = 0x1C;
const OMV_THEORA_PACKET_SIZE: usize = 0x20;
const OGG_SCAN_CHUNK: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct OmvHeader {
    pub header_size: u32,
    pub version: u32,
    pub theora_type: u32,
    pub display_width: u32,
    pub display_height: u32,
    pub frame_time_us: u32,
    pub max_data_size: u32,
    pub page_count_hint: u32,
    pub packet_count_hint: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct OmvTheoraPage {
    pub own_page_no: i32,
    pub is_eos: bool,
    pub is_key_page: bool,
    pub page_size: i32,
    pub seek_offset: i32,
    pub seek_page_no: i32,
    pub packet_count: i32,
    pub top_packet_no: i32,
}

#[derive(Debug, Clone, Copy)]
pub struct OmvTheoraPacket {
    pub own_packet_no: i32,
    pub own_page_no: i32,
    pub own_packet_no_in_page: i32,
    pub is_key_frame: bool,
    pub key_frame_packet_no: i32,
    pub key_frame_page_no: i32,
    pub frame_time_start: i32,
    pub frame_time_end: i32,
}

#[derive(Debug, Clone)]
pub struct OmvFile {
    pub header: OmvHeader,
    pub pages: Vec<OmvTheoraPage>,
    pub packets: Vec<OmvTheoraPacket>,
    /// Absolute file offset that the page-table seek offsets are relative to.
    pub seek_top: u64,
    pub ogg_data_offset: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OmvSeekPoint {
    /// Absolute file offset of the first Ogg page that must be fed to rebuild
    /// the packet containing the selected key frame.
    pub file_offset: u64,
    /// Absolute start of the page whose completed packets are numbered from
    /// `first_packet_no`. Packets completed on earlier back-pages are discarded.
    pub key_page_file_offset: u64,
    pub seek_page_no: usize,
    /// Data-packet number tona3 assigns after the back-pages have been fed
    /// and drained, i.e. `top_packet_no` of the key-frame page.  The seek page
    /// itself may legitimately have `top_packet_no == -1` when it only starts
    /// a packet that completes on a later page.
    pub first_packet_no: usize,
    pub key_frame_page_no: usize,
    pub key_frame_packet_no: usize,
    pub target_packet_no: usize,
}

impl OmvFile {
    /// Read only the fixed metadata prefix required by [`OmvHeader`].
    /// This is used by READY/metadata paths so they do not parse seek tables or
    /// scan the movie payload before playback actually starts.
    pub fn read_header(path: impl AsRef<Path>) -> Result<OmvHeader> {
        let path = path.as_ref();
        let mut file = File::open(path).with_context(|| format!("open OMV: {}", path.display()))?;
        let mut header_prefix = [0u8; OMV_HEADER_MIN_SIZE];
        file.read_exact(&mut header_prefix)
            .with_context(|| format!("read OMV header: {}", path.display()))?;
        read_header(&header_prefix)
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let mut file = File::open(path).with_context(|| format!("open OMV: {}", path.display()))?;
        let mut header_prefix = [0u8; OMV_HEADER_MIN_SIZE];
        file.read_exact(&mut header_prefix)
            .with_context(|| format!("read OMV header: {}", path.display()))?;
        let header = read_header(&header_prefix)?;
        let header_size = usize::try_from(header.header_size)
            .map_err(|_| anyhow!("OMV header size does not fit usize"))?;
        if header_size < OMV_HEADER_MIN_SIZE {
            bail!("invalid OMV header size: {:#x}", header.header_size);
        }

        file.seek(SeekFrom::Start(header.header_size as u64))
            .with_context(|| format!("seek OMV tables: {}", path.display()))?;

        let page_count = usize::try_from(header.page_count_hint)
            .map_err(|_| anyhow!("OMV page count does not fit usize"))?;
        let packet_count = usize::try_from(header.packet_count_hint)
            .map_err(|_| anyhow!("OMV packet count does not fit usize"))?;
        let pages_bytes = page_count
            .checked_mul(OMV_THEORA_PAGE_SIZE)
            .ok_or_else(|| anyhow!("OMV page table size overflow"))?;
        let packets_bytes = packet_count
            .checked_mul(OMV_THEORA_PACKET_SIZE)
            .ok_or_else(|| anyhow!("OMV packet table size overflow"))?;
        let seek_top = (header_size as u64)
            .checked_add(pages_bytes as u64)
            .and_then(|offset| offset.checked_add(packets_bytes as u64))
            .ok_or_else(|| anyhow!("OMV table end offset overflow"))?;
        let file_len = file
            .metadata()
            .with_context(|| format!("stat OMV: {}", path.display()))?
            .len();
        if seek_top > file_len {
            bail!(
                "OMV seek tables exceed file size: table_end={} file_size={}",
                seek_top,
                file_len
            );
        }

        let mut page_table = vec![0u8; pages_bytes];
        if !page_table.is_empty() {
            file.read_exact(&mut page_table)
                .with_context(|| format!("read OMV page table: {}", path.display()))?;
        }
        let mut packet_table = vec![0u8; packets_bytes];
        if !packet_table.is_empty() {
            file.read_exact(&mut packet_table)
                .with_context(|| format!("read OMV packet table: {}", path.display()))?;
        }

        let pages = parse_pages(&page_table, page_count)?;
        let packets = parse_packets(&packet_table, packet_count)?;

        let candidate = pages
            .first()
            .and_then(|page| u64::try_from(page.seek_offset).ok())
            .map(|offset| seek_top.saturating_add(offset));
        let ogg_data_offset = match candidate {
            Some(offset) if file_has_ogg_magic(&mut file, offset)? => offset,
            _ => find_ogg_offset_in_file(&mut file, seek_top)?,
        };

        Ok(Self {
            header,
            pages,
            packets,
            seek_top,
            ogg_data_offset,
        })
    }

    /// Resolve the original tona3 key-page seek plan for a video frame.
    ///
    /// `seek_page_no` may precede the key-frame page because an Ogg packet can
    /// continue across page boundaries. The decoder must begin at that page,
    /// discard packets until the indexed key frame, and then decode forward to
    /// the requested packet.
    pub fn seek_point_for_frame(&self, frame_no: usize) -> Result<OmvSeekPoint> {
        let packet = self
            .packets
            .get(frame_no)
            .ok_or_else(|| anyhow!("OMV frame index out of range: {frame_no}"))?;
        if packet.own_packet_no != frame_no as i32 {
            bail!(
                "OMV packet table row {} identifies itself as {}",
                frame_no,
                packet.own_packet_no
            );
        }
        let key_frame_packet_no = usize::try_from(packet.key_frame_packet_no)
            .map_err(|_| anyhow!("OMV key-frame packet is negative for frame {frame_no}"))?;
        if key_frame_packet_no > frame_no || key_frame_packet_no >= self.packets.len() {
            bail!(
                "invalid OMV key-frame packet {} for frame {}",
                key_frame_packet_no,
                frame_no
            );
        }
        let key_packet = &self.packets[key_frame_packet_no];
        if !key_packet.is_key_frame {
            bail!(
                "OMV indexed key packet {} is not marked as a key frame",
                key_frame_packet_no
            );
        }
        let key_frame_page_no = usize::try_from(packet.key_frame_page_no)
            .map_err(|_| anyhow!("OMV key-frame page is negative for frame {frame_no}"))?;
        if key_packet.own_page_no != packet.key_frame_page_no {
            bail!(
                "OMV key packet {} belongs to page {}, not indexed page {}",
                key_frame_packet_no,
                key_packet.own_page_no,
                packet.key_frame_page_no
            );
        }
        let key_page = self.pages.get(key_frame_page_no).ok_or_else(|| {
            anyhow!(
                "OMV key-frame page {} is out of range for frame {}",
                key_frame_page_no,
                frame_no
            )
        })?;
        if key_page.own_page_no != packet.key_frame_page_no {
            bail!(
                "OMV page table row {} identifies itself as {}",
                key_frame_page_no,
                key_page.own_page_no
            );
        }
        let seek_page_no = usize::try_from(key_page.seek_page_no).map_err(|_| {
            anyhow!(
                "OMV seek page is negative for key-frame page {}",
                key_frame_page_no
            )
        })?;
        if seek_page_no > key_frame_page_no {
            bail!(
                "OMV seek page {} is after key-frame page {}",
                seek_page_no,
                key_frame_page_no
            );
        }
        let seek_page = self.pages.get(seek_page_no).ok_or_else(|| {
            anyhow!(
                "OMV seek page {} is out of range for frame {}",
                seek_page_no,
                frame_no
            )
        })?;
        if seek_page.own_page_no != seek_page_no as i32 {
            bail!(
                "OMV page table row {} identifies itself as {}",
                seek_page_no,
                seek_page.own_page_no
            );
        }
        // Match tona3 exactly: `seek_page_no` is only the first Ogg page that
        // must be fed back into the stream so a packet spanning page borders
        // can be reconstructed.  tona3 drains those back-pages, then starts
        // `decode_packet_no` from *the key-frame page's* `top_packet_no`.
        //
        // Therefore `seek_page.top_packet_no == -1` is valid (and common when
        // that page contains only the beginning of a continued packet).  The
        // old implementation incorrectly treated it as a corrupt index and
        // fell back to reopening/decoding the Ogg stream from frame zero.
        let first_packet_no = usize::try_from(key_page.top_packet_no).map_err(|_| {
            anyhow!(
                "OMV first packet is negative for key-frame page {}",
                key_frame_page_no
            )
        })?;
        if first_packet_no > key_frame_packet_no {
            bail!(
                "OMV key-frame page {} starts at packet {}, after key packet {}",
                key_frame_page_no,
                first_packet_no,
                key_frame_packet_no
            );
        }
        let seek_offset = u64::try_from(seek_page.seek_offset)
            .map_err(|_| anyhow!("OMV seek offset is negative for page {}", seek_page_no))?;
        let file_offset = self
            .seek_top
            .checked_add(seek_offset)
            .ok_or_else(|| anyhow!("OMV seek offset overflow for page {seek_page_no}"))?;
        let key_page_offset = u64::try_from(key_page.seek_offset)
            .map_err(|_| anyhow!("OMV seek offset is negative for page {key_frame_page_no}"))?;
        let key_page_file_offset = self
            .seek_top
            .checked_add(key_page_offset)
            .ok_or_else(|| anyhow!("OMV key-page seek offset overflow"))?;
        if key_page_file_offset < file_offset {
            bail!("OMV key-frame page precedes its seek page in the file");
        }

        Ok(OmvSeekPoint {
            file_offset,
            key_page_file_offset,
            seek_page_no,
            first_packet_no,
            key_frame_page_no,
            key_frame_packet_no,
            target_packet_no: frame_no,
        })
    }

    /// Open a buffered reader positioned at the embedded Ogg bitstream.
    pub fn open_embedded_ogg_reader(&self, path: impl AsRef<Path>) -> Result<BufReader<File>> {
        let path = path.as_ref();
        let mut file = File::open(path).with_context(|| format!("open OMV: {}", path.display()))?;
        file.seek(SeekFrom::Start(self.ogg_data_offset))
            .with_context(|| format!("seek embedded Ogg: {}", path.display()))?;
        Ok(BufReader::new(file))
    }

    /// Read the embedded Ogg bitstream as bytes for legacy/full-decode callers.
    /// Header inspection and streaming playback should use [`Self::open`] and
    /// [`Self::open_embedded_ogg_reader`] instead.
    pub fn read_embedded_ogg(path: impl AsRef<Path>) -> Result<Vec<u8>> {
        let path = path.as_ref();
        let omv = Self::open(path)?;
        let mut reader = omv.open_embedded_ogg_reader(path)?;
        let mut out = Vec::new();
        reader
            .read_to_end(&mut out)
            .with_context(|| format!("read embedded Ogg: {}", path.display()))?;
        Ok(out)
    }
}

fn read_header(buf: &[u8]) -> Result<OmvHeader> {
    if buf.len() < OMV_HEADER_MIN_SIZE {
        bail!("OMV header too small");
    }

    let header_size = read_u32(buf, 0x00)?;
    let version = read_u32(buf, 0x04)?;
    let theora_type = read_u32(buf, 0x28)?;
    let display_width = read_u32(buf, 0x2c)?;
    let display_height = read_u32(buf, 0x30)?;
    let frame_time_us = read_u32(buf, 0x3c)?;
    // This field is the Theora serial number in the original v1.1 header. Keep
    // the historical public name for API compatibility.
    let max_data_size = read_u32(buf, 0x40)?;
    let page_count_hint = read_u32(buf, 0x4c)?;
    let packet_count_hint = read_u32(buf, 0x50)?;

    if header_size < OMV_HEADER_MIN_SIZE as u32 {
        bail!("invalid OMV header size: {header_size:#x}");
    }
    if theora_type > OMV_THEORA_TYPE_YUV {
        bail!("invalid OMV theora type: {theora_type}");
    }
    if display_width == 0 || display_height == 0 {
        bail!(
            "invalid OMV display size: {}x{}",
            display_width,
            display_height
        );
    }

    Ok(OmvHeader {
        header_size,
        version,
        theora_type,
        display_width,
        display_height,
        frame_time_us,
        max_data_size,
        page_count_hint,
        packet_count_hint,
    })
}

fn parse_pages(buf: &[u8], count: usize) -> Result<Vec<OmvTheoraPage>> {
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let base = i * OMV_THEORA_PAGE_SIZE;
        let row = buf
            .get(base..base + OMV_THEORA_PAGE_SIZE)
            .ok_or_else(|| anyhow!("truncated OMV page table at row {i}"))?;
        out.push(OmvTheoraPage {
            own_page_no: read_i32(row, 0x00)?,
            is_eos: row[0x04] != 0,
            is_key_page: row[0x05] != 0,
            page_size: read_i32(row, 0x08)?,
            seek_offset: read_i32(row, 0x0c)?,
            seek_page_no: read_i32(row, 0x10)?,
            packet_count: read_i32(row, 0x14)?,
            top_packet_no: read_i32(row, 0x18)?,
        });
    }
    Ok(out)
}

fn parse_packets(buf: &[u8], count: usize) -> Result<Vec<OmvTheoraPacket>> {
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let base = i * OMV_THEORA_PACKET_SIZE;
        let row = buf
            .get(base..base + OMV_THEORA_PACKET_SIZE)
            .ok_or_else(|| anyhow!("truncated OMV packet table at row {i}"))?;
        out.push(OmvTheoraPacket {
            own_packet_no: read_i32(row, 0x00)?,
            own_page_no: read_i32(row, 0x04)?,
            own_packet_no_in_page: read_i32(row, 0x08)?,
            is_key_frame: row[0x0c] != 0,
            key_frame_packet_no: read_i32(row, 0x10)?,
            key_frame_page_no: read_i32(row, 0x14)?,
            frame_time_start: read_i32(row, 0x18)?,
            frame_time_end: read_i32(row, 0x1c)?,
        });
    }
    Ok(out)
}

fn read_u32(buf: &[u8], offset: usize) -> Result<u32> {
    let bytes: [u8; 4] = buf
        .get(offset..offset + 4)
        .ok_or_else(|| anyhow!("OMV field at {offset:#x} is truncated"))?
        .try_into()
        .map_err(|_| anyhow!("OMV field at {offset:#x} has invalid width"))?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_i32(buf: &[u8], offset: usize) -> Result<i32> {
    Ok(read_u32(buf, offset)? as i32)
}

fn file_has_ogg_magic(file: &mut File, offset: u64) -> Result<bool> {
    file.seek(SeekFrom::Start(offset))?;
    let mut magic = [0u8; 4];
    match file.read_exact(&mut magic) {
        Ok(()) => Ok(&magic == b"OggS"),
        Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => Ok(false),
        Err(err) => Err(err.into()),
    }
}

fn find_ogg_offset_in_file(file: &mut File, start: u64) -> Result<u64> {
    file.seek(SeekFrom::Start(start))?;
    let mut absolute = start;
    let mut carry = Vec::<u8>::new();
    let mut chunk = vec![0u8; OGG_SCAN_CHUNK];

    loop {
        let n = file.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        let mut window = Vec::with_capacity(carry.len() + n);
        window.extend_from_slice(&carry);
        window.extend_from_slice(&chunk[..n]);
        if let Some(pos) = window.windows(4).position(|bytes| bytes == b"OggS") {
            return Ok(absolute
                .saturating_sub(carry.len() as u64)
                .saturating_add(pos as u64));
        }
        carry.clear();
        let keep = window.len().min(3);
        carry.extend_from_slice(&window[window.len() - keep..]);
        absolute = absolute.saturating_add(n as u64);
    }

    bail!("OggS not found in OMV payload")
}

#[cfg(test)]
mod seek_index_tests {
    use super::{OMV_THEORA_TYPE_RGB, OmvFile, OmvHeader, OmvTheoraPacket, OmvTheoraPage};

    #[test]
    fn seek_plan_uses_back_page_and_packet_table() {
        let pages = vec![
            OmvTheoraPage {
                own_page_no: 0,
                is_eos: false,
                is_key_page: true,
                page_size: 100,
                seek_offset: 0,
                seek_page_no: 0,
                packet_count: 4,
                top_packet_no: 0,
            },
            OmvTheoraPage {
                own_page_no: 1,
                is_eos: false,
                is_key_page: false,
                page_size: 100,
                seek_offset: 100,
                seek_page_no: 0,
                packet_count: 0,
                // Legal tona3 index state: this back-page starts a packet but
                // does not complete one, so there is no packet number to emit
                // until the key-frame page is fed.
                top_packet_no: -1,
            },
            OmvTheoraPage {
                own_page_no: 2,
                is_eos: false,
                is_key_page: true,
                page_size: 100,
                seek_offset: 200,
                seek_page_no: 1,
                packet_count: 2,
                top_packet_no: 4,
            },
        ];
        let packets = (0..6)
            .map(|packet_no| OmvTheoraPacket {
                own_packet_no: packet_no,
                own_page_no: packet_no / 2,
                own_packet_no_in_page: packet_no % 2,
                is_key_frame: packet_no == 0 || packet_no == 4,
                key_frame_packet_no: if packet_no < 4 { 0 } else { 4 },
                key_frame_page_no: if packet_no < 4 { 0 } else { 2 },
                frame_time_start: packet_no * 33,
                frame_time_end: (packet_no + 1) * 33,
            })
            .collect();
        let omv = OmvFile {
            header: OmvHeader {
                header_size: 0xb4,
                version: 0x0001_0001,
                theora_type: OMV_THEORA_TYPE_RGB,
                display_width: 640,
                display_height: 480,
                frame_time_us: 33_333,
                max_data_size: 0,
                page_count_hint: 3,
                packet_count_hint: 6,
            },
            pages,
            packets,
            seek_top: 1_000,
            ogg_data_offset: 1_000,
        };

        let point = omv.seek_point_for_frame(5).expect("seek point");
        assert_eq!(point.seek_page_no, 1);
        assert_eq!(point.first_packet_no, 4);
        assert_eq!(point.key_frame_page_no, 2);
        assert_eq!(point.key_frame_packet_no, 4);
        assert_eq!(point.target_packet_no, 5);
        assert_eq!(point.file_offset, 1_100);
        assert_eq!(point.key_page_file_offset, 1_200);
    }
}
