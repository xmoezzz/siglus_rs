use crate::apiwrapper::YuvBuffer;
use crate::codec::{Comment, Info, LegacyTheoraInfo};
use crate::decinfo::{th_decode_headerin, SetupInfo};
use crate::decoder::{th_decode_alloc, DecoderContext};
use crate::encoder::{th_encode_alloc, EncoderContext};
use crate::error::{Result, TheoraError};
use crate::legacy::LegacyState;
use crate::packet::OggPacket;

pub type ThInfo = Info;
pub type ThComment = Comment;
pub type ThSetupInfo = SetupInfo;
pub type ThDecCtx = DecoderContext;
pub type ThEncCtx = EncoderContext;
pub type TheoraInfo = LegacyTheoraInfo;
pub type TheoraComment = Comment;
pub type TheoraState = LegacyState;
pub type TheoraYuvBuffer = YuvBuffer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderStatus {
    NeedMoreHeaders(i32),
    Ready,
}

#[derive(Debug, Clone, Default)]
pub struct HeaderParser {
    pub info: Info,
    pub comment: Comment,
    pub setup: Option<SetupInfo>,
}

impl HeaderParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, packet: &OggPacket) -> Result<HeaderStatus> {
        let remaining = th_decode_headerin(
            &mut self.info,
            Some(&mut self.comment),
            Some(&mut self.setup),
            packet,
        )?;
        if remaining == 0 {
            Ok(HeaderStatus::Ready)
        } else {
            Ok(HeaderStatus::NeedMoreHeaders(remaining))
        }
    }

    pub fn is_ready(&self) -> bool {
        self.setup.is_some()
    }

    pub fn setup_ref(&self) -> Result<&SetupInfo> {
        self.setup.as_ref().ok_or(TheoraError::BadHeader)
    }

    pub fn decoder(&self) -> Result<DecoderContext> {
        let setup = self.setup_ref()?;
        th_decode_alloc(&self.info, setup)
    }

    pub fn into_parts(self) -> (Info, Comment, Option<SetupInfo>) {
        (self.info, self.comment, self.setup)
    }
}

pub fn decoder_from_headers(info: &Info, setup: &SetupInfo) -> Result<DecoderContext> {
    th_decode_alloc(info, setup)
}

pub fn encoder_from_info(info: &Info) -> Result<EncoderContext> {
    th_encode_alloc(info)
}
