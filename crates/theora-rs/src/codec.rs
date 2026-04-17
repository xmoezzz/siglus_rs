use crate::decinfo::SetupInfo;

pub const TH_VERSION_MAJOR: u8 = 3;
pub const TH_VERSION_MINOR: u8 = 2;
pub const TH_VERSION_SUB: u8 = 1;
pub const TH_NHUFFMAN_TABLES: usize = 80;
pub const TH_NDCT_TOKENS: usize = 32;
pub const TH_PF_NFORMATS: usize = 4;
pub const OC_VENDOR_STRING: &str = "Xiph.Org libtheora 1.2.0 20250329 (Ptalarbvorm)";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(i32)]
pub enum Colorspace {
    #[default]
    Unspecified = 0,
    ItuRec470M = 1,
    ItuRec470Bg = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(i32)]
pub enum PixelFmt {
    #[default]
    Pf420 = 0,
    Reserved = 1,
    Pf422 = 2,
    Pf444 = 3,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImgPlane {
    pub width: i32,
    pub height: i32,
    pub stride: i32,
    pub data: Vec<u8>,
    pub data_offset: usize,
}

pub type YCbCrBuffer = [ImgPlane; 3];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
    pub version_major: u8,
    pub version_minor: u8,
    pub version_subminor: u8,
    pub frame_width: u32,
    pub frame_height: u32,
    pub pic_width: u32,
    pub pic_height: u32,
    pub pic_x: u32,
    pub pic_y: u32,
    pub fps_numerator: u32,
    pub fps_denominator: u32,
    pub aspect_numerator: u32,
    pub aspect_denominator: u32,
    pub colorspace: Colorspace,
    pub pixel_fmt: PixelFmt,
    pub target_bitrate: i32,
    pub quality: i32,
    pub keyframe_granule_shift: i32,
}

impl Info {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn zeroed() -> Self {
        Self {
            version_major: 0,
            version_minor: 0,
            version_subminor: 0,
            frame_width: 0,
            frame_height: 0,
            pic_width: 0,
            pic_height: 0,
            pic_x: 0,
            pic_y: 0,
            fps_numerator: 0,
            fps_denominator: 0,
            aspect_numerator: 0,
            aspect_denominator: 0,
            colorspace: Colorspace::Unspecified,
            pixel_fmt: PixelFmt::Pf420,
            target_bitrate: 0,
            quality: 0,
            keyframe_granule_shift: 0,
        }
    }

    pub fn clear(&mut self) {
        *self = Self::zeroed();
    }
}

impl Default for Info {
    fn default() -> Self {
        Self {
            version_major: TH_VERSION_MAJOR,
            version_minor: TH_VERSION_MINOR,
            version_subminor: TH_VERSION_SUB,
            frame_width: 0,
            frame_height: 0,
            pic_width: 0,
            pic_height: 0,
            pic_x: 0,
            pic_y: 0,
            fps_numerator: 0,
            fps_denominator: 0,
            aspect_numerator: 0,
            aspect_denominator: 0,
            colorspace: Colorspace::Unspecified,
            pixel_fmt: PixelFmt::Pf420,
            target_bitrate: 0,
            quality: 0,
            keyframe_granule_shift: 6,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Comment {
    pub user_comments: Vec<Vec<u8>>,
    pub vendor: Vec<u8>,
}

impl Comment {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.user_comments.clear();
        self.vendor.clear();
    }

    pub fn comment_lengths(&self) -> Vec<i32> {
        self.user_comments.iter().map(|c| c.len() as i32).collect()
    }
}

pub type QuantBase = [u8; 64];

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QuantRanges {
    pub sizes: Vec<i32>,
    pub base_matrices: Vec<QuantBase>,
}

impl QuantRanges {
    pub fn nranges(&self) -> usize {
        self.sizes.len()
    }

    pub fn clear(&mut self) {
        self.sizes.clear();
        self.base_matrices.clear();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuantInfo {
    pub dc_scale: [u16; 64],
    pub ac_scale: [u16; 64],
    pub loop_filter_limits: [u8; 64],
    pub qi_ranges: [[QuantRanges; 3]; 2],
}

impl Default for QuantInfo {
    fn default() -> Self {
        Self {
            dc_scale: [0; 64],
            ac_scale: [0; 64],
            loop_filter_limits: [0; 64],
            qi_ranges: [
                [
                    QuantRanges::default(),
                    QuantRanges::default(),
                    QuantRanges::default(),
                ],
                [
                    QuantRanges::default(),
                    QuantRanges::default(),
                    QuantRanges::default(),
                ],
            ],
        }
    }
}

impl QuantInfo {
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HuffCode {
    pub pattern: u32,
    pub nbits: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LegacyTheoraInfo {
    pub width: u32,
    pub height: u32,
    pub frame_width: u32,
    pub frame_height: u32,
    pub offset_x: u32,
    pub offset_y: u32,
    pub fps_numerator: u32,
    pub fps_denominator: u32,
    pub aspect_numerator: u32,
    pub aspect_denominator: u32,
    pub colorspace: Colorspace,
    pub target_bitrate: i32,
    pub quality: i32,
    pub quick_p: i32,
    pub version_major: u8,
    pub version_minor: u8,
    pub version_subminor: u8,
    pub codec_setup: Option<SetupInfo>,
    pub dropframes_p: i32,
    pub keyframe_auto_p: i32,
    pub keyframe_frequency: u32,
    pub keyframe_frequency_force: u32,
    pub keyframe_data_target_bitrate: u32,
    pub keyframe_auto_threshold: i32,
    pub keyframe_mindistance: u32,
    pub noise_sensitivity: i32,
    pub sharpness: i32,
    pub pixelformat: PixelFmt,
}
