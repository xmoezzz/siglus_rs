use crate::codec::{Info, LegacyTheoraInfo, YCbCrBuffer};
use crate::decinfo::SetupInfo;
use crate::decoder::DecoderContext;
use crate::encoder::EncoderContext;

#[derive(Debug, Clone, Default)]
pub struct ApiWrapper {
    pub setup: Option<SetupInfo>,
    pub decode: Option<DecoderContext>,
    pub encode: Option<EncoderContext>,
}

impl ApiWrapper {
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[derive(Debug, Clone, Default)]
pub struct ApiInfo {
    pub api: ApiWrapper,
    pub info: LegacyTheoraInfo,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct YuvBuffer {
    pub y_width: i32,
    pub y_height: i32,
    pub y_stride: i32,
    pub uv_width: i32,
    pub uv_height: i32,
    pub uv_stride: i32,
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
}

impl YuvBuffer {
    pub fn from_ycbcr(buf: &YCbCrBuffer) -> Self {
        Self {
            y_width: buf[0].width,
            y_height: buf[0].height,
            y_stride: buf[0].stride,
            uv_width: buf[1].width,
            uv_height: buf[1].height,
            uv_stride: buf[1].stride,
            y: buf[0].data.clone(),
            u: buf[1].data.clone(),
            v: buf[2].data.clone(),
        }
    }

    pub fn to_ycbcr(&self) -> YCbCrBuffer {
        [
            crate::codec::ImgPlane {
                width: self.y_width,
                height: self.y_height,
                stride: self.y_stride,
                data: self.y.clone(),
                data_offset: 0,
            },
            crate::codec::ImgPlane {
                width: self.uv_width,
                height: self.uv_height,
                stride: self.uv_stride,
                data: self.u.clone(),
                data_offset: 0,
            },
            crate::codec::ImgPlane {
                width: self.uv_width,
                height: self.uv_height,
                stride: self.uv_stride,
                data: self.v.clone(),
                data_offset: 0,
            },
        ]
    }
}

#[derive(Debug, Clone, Default)]
pub struct CompatState {
    pub internal_decode: bool,
    pub internal_encode: bool,
    pub granulepos: i64,
    pub info: Option<LegacyTheoraInfo>,
    pub api: Option<ApiWrapper>,
    pub th_info: Option<Info>,
}
