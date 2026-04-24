use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShaderKind {
    Vertex,
    Pixel,
}

impl ShaderKind {
    pub fn profile_prefix(self) -> &'static str {
        match self {
            ShaderKind::Vertex => "vs",
            ShaderKind::Pixel => "ps",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Opcode {
    Nop,
    Mov,
    Add,
    Sub,
    Mad,
    Mul,
    Rcp,
    Rsq,
    Dp3,
    Dp4,
    Min,
    Max,
    Slt,
    Sge,
    Exp,
    Log,
    Lit,
    Dst,
    Lrp,
    Frc,
    M4x4,
    M4x3,
    M3x4,
    M3x3,
    M3x2,
    Call,
    CallNZ,
    Loop,
    Ret,
    EndLoop,
    Label,
    Dcl,
    Pow,
    Crs,
    Sgn,
    Abs,
    Nrm,
    SinCos,
    Rep,
    EndRep,
    If,
    IfC,
    Else,
    EndIf,
    Break,
    BreakC,
    MovA,
    DefB,
    DefI,
    TexCoord,
    TexKill,
    Tex,
    TexBem,
    TexBeml,
    TexReg2AR,
    TexReg2GB,
    TexM3x2Pad,
    TexM3x2Tex,
    TexM3x3Pad,
    TexM3x3Tex,
    TexM3x3Diff,
    TexM3x3Spec,
    TexM3x3VSpec,
    ExpP,
    LogP,
    Cnd,
    Def,
    TexReg2RGB,
    TexDP3Tex,
    TexM3x2Depth,
    TexDP3,
    TexM3x3,
    TexDepth,
    Cmp,
    Bem,
    Dp2Add,
    Dsx,
    Dsy,
    TexLdd,
    SetP,
    TexLdl,
    BreakP,
    Phase,
    Comment,
    End,
    Unknown(u16),
}

impl Opcode {
    pub fn from_u16(v: u16) -> Self {
        match v {
            0 => Self::Nop,
            1 => Self::Mov,
            2 => Self::Add,
            3 => Self::Sub,
            4 => Self::Mad,
            5 => Self::Mul,
            6 => Self::Rcp,
            7 => Self::Rsq,
            8 => Self::Dp3,
            9 => Self::Dp4,
            10 => Self::Min,
            11 => Self::Max,
            12 => Self::Slt,
            13 => Self::Sge,
            14 => Self::Exp,
            15 => Self::Log,
            16 => Self::Lit,
            17 => Self::Dst,
            18 => Self::Lrp,
            19 => Self::Frc,
            20 => Self::M4x4,
            21 => Self::M4x3,
            22 => Self::M3x4,
            23 => Self::M3x3,
            24 => Self::M3x2,
            25 => Self::Call,
            26 => Self::CallNZ,
            27 => Self::Loop,
            28 => Self::Ret,
            29 => Self::EndLoop,
            30 => Self::Label,
            31 => Self::Dcl,
            32 => Self::Pow,
            33 => Self::Crs,
            34 => Self::Sgn,
            35 => Self::Abs,
            36 => Self::Nrm,
            37 => Self::SinCos,
            38 => Self::Rep,
            39 => Self::EndRep,
            40 => Self::If,
            41 => Self::IfC,
            42 => Self::Else,
            43 => Self::EndIf,
            44 => Self::Break,
            45 => Self::BreakC,
            46 => Self::MovA,
            47 => Self::DefB,
            48 => Self::DefI,
            64 => Self::TexCoord,
            65 => Self::TexKill,
            66 => Self::Tex,
            67 => Self::TexBem,
            68 => Self::TexBeml,
            69 => Self::TexReg2AR,
            70 => Self::TexReg2GB,
            71 => Self::TexM3x2Pad,
            72 => Self::TexM3x2Tex,
            73 => Self::TexM3x3Pad,
            74 => Self::TexM3x3Tex,
            75 => Self::TexM3x3Diff,
            76 => Self::TexM3x3Spec,
            77 => Self::TexM3x3VSpec,
            78 => Self::ExpP,
            79 => Self::LogP,
            80 => Self::Cnd,
            81 => Self::Def,
            82 => Self::TexReg2RGB,
            83 => Self::TexDP3Tex,
            84 => Self::TexM3x2Depth,
            85 => Self::TexDP3,
            86 => Self::TexM3x3,
            87 => Self::TexDepth,
            88 => Self::Cmp,
            89 => Self::Bem,
            90 => Self::Dp2Add,
            91 => Self::Dsx,
            92 => Self::Dsy,
            93 => Self::TexLdd,
            94 => Self::SetP,
            95 => Self::TexLdl,
            96 => Self::BreakP,
            0xfffd => Self::Phase,
            0xfffe => Self::Comment,
            0xffff => Self::End,
            x => Self::Unknown(x),
        }
    }

    pub fn code(self) -> u16 {
        match self {
            Self::Nop => 0,
            Self::Mov => 1,
            Self::Add => 2,
            Self::Sub => 3,
            Self::Mad => 4,
            Self::Mul => 5,
            Self::Rcp => 6,
            Self::Rsq => 7,
            Self::Dp3 => 8,
            Self::Dp4 => 9,
            Self::Min => 10,
            Self::Max => 11,
            Self::Slt => 12,
            Self::Sge => 13,
            Self::Exp => 14,
            Self::Log => 15,
            Self::Lit => 16,
            Self::Dst => 17,
            Self::Lrp => 18,
            Self::Frc => 19,
            Self::M4x4 => 20,
            Self::M4x3 => 21,
            Self::M3x4 => 22,
            Self::M3x3 => 23,
            Self::M3x2 => 24,
            Self::Call => 25,
            Self::CallNZ => 26,
            Self::Loop => 27,
            Self::Ret => 28,
            Self::EndLoop => 29,
            Self::Label => 30,
            Self::Dcl => 31,
            Self::Pow => 32,
            Self::Crs => 33,
            Self::Sgn => 34,
            Self::Abs => 35,
            Self::Nrm => 36,
            Self::SinCos => 37,
            Self::Rep => 38,
            Self::EndRep => 39,
            Self::If => 40,
            Self::IfC => 41,
            Self::Else => 42,
            Self::EndIf => 43,
            Self::Break => 44,
            Self::BreakC => 45,
            Self::MovA => 46,
            Self::DefB => 47,
            Self::DefI => 48,
            Self::TexCoord => 64,
            Self::TexKill => 65,
            Self::Tex => 66,
            Self::TexBem => 67,
            Self::TexBeml => 68,
            Self::TexReg2AR => 69,
            Self::TexReg2GB => 70,
            Self::TexM3x2Pad => 71,
            Self::TexM3x2Tex => 72,
            Self::TexM3x3Pad => 73,
            Self::TexM3x3Tex => 74,
            Self::TexM3x3Diff => 75,
            Self::TexM3x3Spec => 76,
            Self::TexM3x3VSpec => 77,
            Self::ExpP => 78,
            Self::LogP => 79,
            Self::Cnd => 80,
            Self::Def => 81,
            Self::TexReg2RGB => 82,
            Self::TexDP3Tex => 83,
            Self::TexM3x2Depth => 84,
            Self::TexDP3 => 85,
            Self::TexM3x3 => 86,
            Self::TexDepth => 87,
            Self::Cmp => 88,
            Self::Bem => 89,
            Self::Dp2Add => 90,
            Self::Dsx => 91,
            Self::Dsy => 92,
            Self::TexLdd => 93,
            Self::SetP => 94,
            Self::TexLdl => 95,
            Self::BreakP => 96,
            Self::Phase => 0xfffd,
            Self::Comment => 0xfffe,
            Self::End => 0xffff,
            Self::Unknown(x) => x,
        }
    }

    pub fn mnemonic(self) -> &'static str {
        match self {
            Self::Nop => "nop",
            Self::Mov => "mov",
            Self::Add => "add",
            Self::Sub => "sub",
            Self::Mad => "mad",
            Self::Mul => "mul",
            Self::Rcp => "rcp",
            Self::Rsq => "rsq",
            Self::Dp3 => "dp3",
            Self::Dp4 => "dp4",
            Self::Min => "min",
            Self::Max => "max",
            Self::Slt => "slt",
            Self::Sge => "sge",
            Self::Exp => "exp",
            Self::Log => "log",
            Self::Lit => "lit",
            Self::Dst => "dst",
            Self::Lrp => "lrp",
            Self::Frc => "frc",
            Self::M4x4 => "m4x4",
            Self::M4x3 => "m4x3",
            Self::M3x4 => "m3x4",
            Self::M3x3 => "m3x3",
            Self::M3x2 => "m3x2",
            Self::Call => "call",
            Self::CallNZ => "callnz",
            Self::Loop => "loop",
            Self::Ret => "ret",
            Self::EndLoop => "endloop",
            Self::Label => "label",
            Self::Dcl => "dcl",
            Self::Pow => "pow",
            Self::Crs => "crs",
            Self::Sgn => "sgn",
            Self::Abs => "abs",
            Self::Nrm => "nrm",
            Self::SinCos => "sincos",
            Self::Rep => "rep",
            Self::EndRep => "endrep",
            Self::If => "if",
            Self::IfC => "ifc",
            Self::Else => "else",
            Self::EndIf => "endif",
            Self::Break => "break",
            Self::BreakC => "breakc",
            Self::MovA => "mova",
            Self::DefB => "defb",
            Self::DefI => "defi",
            Self::TexCoord => "texcoord",
            Self::TexKill => "texkill",
            Self::Tex => "texld",
            Self::TexBem => "texbem",
            Self::TexBeml => "texbeml",
            Self::TexReg2AR => "texreg2ar",
            Self::TexReg2GB => "texreg2gb",
            Self::TexM3x2Pad => "texm3x2pad",
            Self::TexM3x2Tex => "texm3x2tex",
            Self::TexM3x3Pad => "texm3x3pad",
            Self::TexM3x3Tex => "texm3x3tex",
            Self::TexM3x3Diff => "texm3x3diff",
            Self::TexM3x3Spec => "texm3x3spec",
            Self::TexM3x3VSpec => "texm3x3vspec",
            Self::ExpP => "expp",
            Self::LogP => "logp",
            Self::Cnd => "cnd",
            Self::Def => "def",
            Self::TexReg2RGB => "texreg2rgb",
            Self::TexDP3Tex => "texdp3tex",
            Self::TexM3x2Depth => "texm3x2depth",
            Self::TexDP3 => "texdp3",
            Self::TexM3x3 => "texm3x3",
            Self::TexDepth => "texdepth",
            Self::Cmp => "cmp",
            Self::Bem => "bem",
            Self::Dp2Add => "dp2add",
            Self::Dsx => "dsx",
            Self::Dsy => "dsy",
            Self::TexLdd => "texldd",
            Self::SetP => "setp",
            Self::TexLdl => "texldl",
            Self::BreakP => "breakp",
            Self::Phase => "phase",
            Self::Comment => "comment",
            Self::End => "end",
            Self::Unknown(_) => "unknown",
        }
    }

    pub fn has_destination(self) -> bool {
        matches!(
            self,
            Self::Abs
                | Self::Add
                | Self::Bem
                | Self::Cmp
                | Self::Cnd
                | Self::Crs
                | Self::Dcl
                | Self::Def
                | Self::DefB
                | Self::DefI
                | Self::Dp2Add
                | Self::Dp3
                | Self::Dp4
                | Self::Dst
                | Self::Dsx
                | Self::Dsy
                | Self::Exp
                | Self::ExpP
                | Self::Frc
                | Self::Lit
                | Self::Log
                | Self::LogP
                | Self::Lrp
                | Self::M3x2
                | Self::M3x3
                | Self::M3x4
                | Self::M4x3
                | Self::M4x4
                | Self::Mad
                | Self::Max
                | Self::Min
                | Self::Mov
                | Self::MovA
                | Self::Mul
                | Self::Nrm
                | Self::Pow
                | Self::Rcp
                | Self::Rsq
                | Self::SetP
                | Self::Sge
                | Self::Sgn
                | Self::SinCos
                | Self::Slt
                | Self::Sub
                | Self::Tex
                | Self::TexBem
                | Self::TexBeml
                | Self::TexCoord
                | Self::TexDepth
                | Self::TexDP3
                | Self::TexDP3Tex
                | Self::TexKill
                | Self::TexLdd
                | Self::TexLdl
                | Self::TexM3x2Depth
                | Self::TexM3x2Pad
                | Self::TexM3x2Tex
                | Self::TexM3x3
                | Self::TexM3x3Diff
                | Self::TexM3x3Pad
                | Self::TexM3x3Spec
                | Self::TexM3x3Tex
                | Self::TexM3x3VSpec
                | Self::TexReg2AR
                | Self::TexReg2GB
                | Self::TexReg2RGB
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum RegisterType {
    Temp,
    Input,
    Const,
    Texture,
    RastOut,
    AttrOut,
    Output,
    ConstInt,
    ColorOut,
    DepthOut,
    Sampler,
    Const2,
    Const3,
    Const4,
    ConstBool,
    Loop,
    TempFloat16,
    MiscType,
    Label,
    Predicate,
    Unknown(u8),
}

impl RegisterType {
    pub fn from_param_token(token: u32) -> Self {
        let raw = (((token >> 28) & 0x7) | ((token >> 8) & 0x18)) as u8;
        match raw {
            0 => Self::Temp,
            1 => Self::Input,
            2 => Self::Const,
            3 => Self::Texture,
            4 => Self::RastOut,
            5 => Self::AttrOut,
            6 => Self::Output,
            7 => Self::ConstInt,
            8 => Self::ColorOut,
            9 => Self::DepthOut,
            10 => Self::Sampler,
            11 => Self::Const2,
            12 => Self::Const3,
            13 => Self::Const4,
            14 => Self::ConstBool,
            15 => Self::Loop,
            16 => Self::TempFloat16,
            17 => Self::MiscType,
            18 => Self::Label,
            19 => Self::Predicate,
            x => Self::Unknown(x),
        }
    }

    pub fn asm_prefix(self) -> &'static str {
        match self {
            Self::Temp => "r",
            Self::Input => "v",
            Self::Const => "c",
            Self::Texture => "t",
            Self::RastOut => "o",
            Self::AttrOut => "oD",
            Self::Output => "o",
            Self::ConstInt => "i",
            Self::ColorOut => "oC",
            Self::DepthOut => "oDepth",
            Self::Sampler => "s",
            Self::ConstBool => "b",
            Self::Loop => "aL",
            Self::TempFloat16 => "r",
            Self::MiscType => "v",
            Self::Label => "l",
            Self::Predicate => "p",
            Self::Const2 | Self::Const3 | Self::Const4 => "c",
            Self::Unknown(_) => "u",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct RegisterKey {
    pub ty: RegisterType,
    pub number: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceModifier {
    None,
    Negate,
    Bias,
    BiasAndNegate,
    Sign,
    SignAndNegate,
    Complement,
    X2,
    X2AndNegate,
    DivideByZ,
    DivideByW,
    Abs,
    AbsAndNegate,
    Not,
    Unknown(u8),
}

impl SourceModifier {
    pub fn from_param_token(token: u32) -> Self {
        match ((token >> 24) & 0xf) as u8 {
            0 => Self::None,
            1 => Self::Negate,
            2 => Self::Bias,
            3 => Self::BiasAndNegate,
            4 => Self::Sign,
            5 => Self::SignAndNegate,
            6 => Self::Complement,
            7 => Self::X2,
            8 => Self::X2AndNegate,
            9 => Self::DivideByZ,
            10 => Self::DivideByW,
            11 => Self::Abs,
            12 => Self::AbsAndNegate,
            13 => Self::Not,
            x => Self::Unknown(x),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResultModifier {
    pub saturate: bool,
    pub partial_precision: bool,
    pub centroid: bool,
    pub raw: u8,
}

impl ResultModifier {
    pub fn from_dest_token(token: u32) -> Self {
        let raw = ((token >> 20) & 0xf) as u8;
        Self {
            saturate: (raw & 1) != 0,
            partial_precision: (raw & 2) != 0,
            centroid: (raw & 4) != 0,
            raw,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SamplerTextureType {
    Unknown,
    TwoD,
    Cube,
    Volume,
}

impl SamplerTextureType {
    pub fn from_decl_token(token: u32) -> Self {
        match ((token >> 27) & 0xf) as u8 {
            2 => Self::TwoD,
            3 => Self::Cube,
            4 => Self::Volume,
            _ => Self::Unknown,
        }
    }

    pub fn asm_name(self) -> &'static str {
        match self {
            Self::TwoD => "2d",
            Self::Cube => "cube",
            Self::Volume => "volume",
            Self::Unknown => "unknown",
        }
    }

    pub fn hlsl_dim(self) -> usize {
        match self {
            Self::Cube | Self::Volume => 3,
            Self::TwoD | Self::Unknown => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclUsage {
    Position,
    BlendWeight,
    BlendIndices,
    Normal,
    PSize,
    TexCoord,
    Tangent,
    Binormal,
    TessFactor,
    PositionT,
    Color,
    Fog,
    Depth,
    Sample,
    Unknown(u8),
}

impl DeclUsage {
    pub fn from_decl_token(token: u32) -> Self {
        match (token & 0x1f) as u8 {
            0 => Self::Position,
            1 => Self::BlendWeight,
            2 => Self::BlendIndices,
            3 => Self::Normal,
            4 => Self::PSize,
            5 => Self::TexCoord,
            6 => Self::Tangent,
            7 => Self::Binormal,
            8 => Self::TessFactor,
            9 => Self::PositionT,
            10 => Self::Color,
            11 => Self::Fog,
            12 => Self::Depth,
            13 => Self::Sample,
            x => Self::Unknown(x),
        }
    }

    pub fn semantic_prefix(self) -> &'static str {
        match self {
            Self::Position => "POSITION",
            Self::BlendWeight => "BLENDWEIGHT",
            Self::BlendIndices => "BLENDINDICES",
            Self::Normal => "NORMAL",
            Self::PSize => "PSIZE",
            Self::TexCoord => "TEXCOORD",
            Self::Tangent => "TANGENT",
            Self::Binormal => "BINORMAL",
            Self::TessFactor => "TESSFACTOR",
            Self::PositionT => "POSITIONT",
            Self::Color => "COLOR",
            Self::Fog => "FOG",
            Self::Depth => "DEPTH",
            Self::Sample => "SAMPLE",
            Self::Unknown(_) => "TEXCOORD",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Instruction {
    pub offset: usize,
    pub token: u32,
    pub opcode: Opcode,
    pub params: Vec<u32>,
}

impl Instruction {
    pub fn comparison(self: &Self) -> u8 {
        ((self.token >> 16) & 7) as u8
    }

    pub fn texld_controls(self: &Self) -> u8 {
        ((self.token >> 16) & 3) as u8
    }

    pub fn dest_param_index(&self) -> usize {
        if self.opcode == Opcode::Dcl { 1 } else { 0 }
    }

    pub fn dest_token(&self) -> Option<u32> {
        self.params.get(self.dest_param_index()).copied()
    }

    pub fn dest_register(&self) -> Option<RegisterKey> {
        let t = self.dest_token()?;
        Some(RegisterKey { ty: RegisterType::from_param_token(t), number: (t & 0x7ff) as u16 })
    }

    pub fn dest_write_mask(&self) -> u8 {
        let Some(t) = self.dest_token() else { return 0xf; };
        let mask = ((t >> 16) & 0xf) as u8;
        if mask == 0 { 0xf } else { mask }
    }

    pub fn dest_modifier(&self) -> ResultModifier {
        ResultModifier::from_dest_token(self.dest_token().unwrap_or(0))
    }

    pub fn source_register(&self, param_index: usize) -> Option<RegisterKey> {
        let t = *self.params.get(param_index)?;
        Some(RegisterKey { ty: RegisterType::from_param_token(t), number: (t & 0x7ff) as u16 })
    }

    pub fn source_modifier(&self, param_index: usize) -> SourceModifier {
        self.params.get(param_index).copied().map(SourceModifier::from_param_token).unwrap_or(SourceModifier::None)
    }

    pub fn source_swizzle(&self, param_index: usize) -> [usize; 4] {
        let t = self.params.get(param_index).copied().unwrap_or(0);
        let swz = ((t >> 16) & 0xff) as usize;
        [swz & 3, (swz >> 2) & 3, (swz >> 4) & 3, (swz >> 6) & 3]
    }

    pub fn decl_usage(&self) -> DeclUsage {
        self.params.first().copied().map(DeclUsage::from_decl_token).unwrap_or(DeclUsage::TexCoord)
    }

    pub fn decl_index(&self) -> u8 {
        self.params.first().map(|v| ((v >> 16) & 0x0f) as u8).unwrap_or(0)
    }

    pub fn decl_sampler_type(&self) -> SamplerTextureType {
        self.params.first().copied().map(SamplerTextureType::from_decl_token).unwrap_or(SamplerTextureType::Unknown)
    }

    pub fn get_float_param(&self, index: usize) -> f32 {
        f32::from_bits(*self.params.get(index).unwrap_or(&0))
    }

    pub fn get_int_param(&self, index: usize) -> i32 {
        *self.params.get(index).unwrap_or(&0) as i32
    }
}

#[derive(Debug, Clone)]
pub struct ShaderModel {
    pub kind: ShaderKind,
    pub major: u8,
    pub minor: u8,
    pub instructions: Vec<Instruction>,
}

#[derive(Debug, Clone)]
pub enum ShaderParseError {
    TooShort,
    BadVersion(u32),
    Truncated { offset: usize, need: usize, len: usize },
    MissingEnd,
}

impl fmt::Display for ShaderParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort => write!(f, "shader bytecode is too short"),
            Self::BadVersion(v) => write!(f, "unsupported shader version token 0x{v:08x}"),
            Self::Truncated { offset, need, len } => write!(f, "truncated shader token stream at 0x{offset:x}: need {need} bytes, len {len}"),
            Self::MissingEnd => write!(f, "shader token stream has no END token"),
        }
    }
}

impl std::error::Error for ShaderParseError {}

fn read_u32(data: &[u8], off: usize) -> Result<u32, ShaderParseError> {
    let b = data.get(off..off + 4).ok_or(ShaderParseError::Truncated { offset: off, need: 4, len: data.len() })?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

pub fn parse_shader(data: &[u8]) -> Result<ShaderModel, ShaderParseError> {
    if data.len() < 4 {
        return Err(ShaderParseError::TooShort);
    }
    let version = read_u32(data, 0)?;
    let minor = (version & 0xff) as u8;
    let major = ((version >> 8) & 0xff) as u8;
    let kind = match version & 0xffff_0000 {
        0xfffe_0000 => ShaderKind::Vertex,
        0xffff_0000 => ShaderKind::Pixel,
        _ => return Err(ShaderParseError::BadVersion(version)),
    };

    let mut instructions = Vec::new();
    let mut off = 4usize;
    while off + 4 <= data.len() {
        let token = read_u32(data, off)?;
        let opcode = Opcode::from_u16((token & 0xffff) as u16);
        let param_count = if opcode == Opcode::Comment {
            ((token >> 16) & 0x7fff) as usize
        } else if major == 1 {
            fixed_size_param_count(opcode)
        } else {
            ((token >> 24) & 0x0f) as usize
        };
        let params_start = off + 4;
        let params_bytes = param_count.saturating_mul(4);
        if params_start + params_bytes > data.len() {
            return Err(ShaderParseError::Truncated { offset: off, need: 4 + params_bytes, len: data.len() });
        }
        let mut params = Vec::with_capacity(param_count);
        for p in 0..param_count {
            params.push(read_u32(data, params_start + p * 4)?);
        }
        instructions.push(Instruction { offset: off, token, opcode, params });
        off = params_start + params_bytes;
        if opcode == Opcode::End {
            return Ok(ShaderModel { kind, major, minor, instructions });
        }
    }

    Err(ShaderParseError::MissingEnd)
}

fn fixed_size_param_count(opcode: Opcode) -> usize {
    match opcode {
        Opcode::Comment => 0,
        Opcode::Def => 5,
        Opcode::TexCoord | Opcode::TexKill | Opcode::Tex | Opcode::TexBem | Opcode::TexBeml | Opcode::TexReg2AR | Opcode::TexReg2GB | Opcode::TexM3x2Pad | Opcode::TexM3x2Tex | Opcode::TexM3x3Pad | Opcode::TexM3x3Tex | Opcode::TexM3x3Diff | Opcode::TexM3x3Spec | Opcode::TexM3x3VSpec | Opcode::TexReg2RGB | Opcode::TexDP3Tex | Opcode::TexM3x2Depth | Opcode::TexDP3 | Opcode::TexM3x3 | Opcode::TexDepth => 2,
        Opcode::Dcl => 2,
        Opcode::End | Opcode::Nop | Opcode::Phase | Opcode::Ret | Opcode::Else | Opcode::EndIf | Opcode::EndLoop | Opcode::EndRep | Opcode::Break => 0,
        _ if opcode.has_destination() => 1 + num_inputs(opcode),
        _ => num_inputs(opcode),
    }
}

pub fn num_inputs(opcode: Opcode) -> usize {
    match opcode {
        Opcode::Abs | Opcode::CallNZ | Opcode::Dsx | Opcode::Dsy | Opcode::Exp | Opcode::ExpP | Opcode::Frc | Opcode::Lit | Opcode::Log | Opcode::LogP | Opcode::Loop | Opcode::Mov | Opcode::MovA | Opcode::Nrm | Opcode::Rcp | Opcode::Rsq | Opcode::SinCos | Opcode::TexKill | Opcode::If | Opcode::Rep => 1,
        Opcode::Add | Opcode::Bem | Opcode::Crs | Opcode::Dp3 | Opcode::Dp4 | Opcode::Dst | Opcode::M3x2 | Opcode::M3x3 | Opcode::M3x4 | Opcode::M4x3 | Opcode::M4x4 | Opcode::Max | Opcode::Min | Opcode::Mul | Opcode::Pow | Opcode::SetP | Opcode::Sge | Opcode::Slt | Opcode::Sub | Opcode::Tex | Opcode::TexLdd | Opcode::TexLdl | Opcode::BreakC | Opcode::IfC => 2,
        Opcode::Cmp | Opcode::Cnd | Opcode::Dp2Add | Opcode::Lrp | Opcode::Mad | Opcode::Sgn => 3,
        _ => 0,
    }
}

pub fn mask_len(mask: u8) -> usize {
    (0..4).filter(|i| (mask & (1 << i)) != 0).count().max(1)
}

pub fn mask_suffix(mask: u8) -> String {
    let m = if mask == 0 { 0xf } else { mask };
    if m == 0xf {
        String::new()
    } else {
        let mut s = String::from(".");
        for (i, c) in ['x', 'y', 'z', 'w'].iter().enumerate() {
            if (m & (1 << i)) != 0 {
                s.push(*c);
            }
        }
        s
    }
}

pub fn swizzle_suffix(swizzle: [usize; 4], count: usize) -> String {
    let count = count.clamp(1, 4);
    let identity = [0usize, 1, 2, 3];
    if count == 4 && swizzle == identity {
        return String::new();
    }
    if count < 4 && swizzle[..count] == identity[..count] {
        return String::new();
    }
    let names = ['x', 'y', 'z', 'w'];
    let mut s = String::from(".");
    for i in 0..count {
        s.push(names[swizzle[i]]);
    }
    s
}

pub fn asm_register_name(reg: RegisterKey) -> String {
    match reg.ty {
        RegisterType::RastOut => match reg.number {
            0 => "oPos".to_string(),
            1 => "oFog".to_string(),
            2 => "oPts".to_string(),
            _ => format!("o{}", reg.number),
        },
        RegisterType::DepthOut => "oDepth".to_string(),
        RegisterType::MiscType => match reg.number {
            0 => "vPos".to_string(),
            1 => "vFace".to_string(),
            _ => format!("vMisc{}", reg.number),
        },
        _ => format!("{}{}", reg.ty.asm_prefix(), reg.number),
    }
}

fn format_source_param(inst: &Instruction, param_index: usize, count: usize) -> String {
    let Some(reg) = inst.source_register(param_index) else { return "<?>".to_string(); };
    let base = asm_register_name(reg);
    let swz = swizzle_suffix(inst.source_swizzle(param_index), count);
    let value = format!("{base}{swz}");
    match inst.source_modifier(param_index) {
        SourceModifier::None => value,
        SourceModifier::Negate => format!("-{value}"),
        SourceModifier::Abs => format!("abs({value})"),
        SourceModifier::AbsAndNegate => format!("-abs({value})"),
        SourceModifier::Bias => format!("{value}_bias"),
        SourceModifier::BiasAndNegate => format!("-{value}_bias"),
        SourceModifier::Sign => format!("{value}_bx2"),
        SourceModifier::SignAndNegate => format!("-{value}_bx2"),
        SourceModifier::Complement => format!("1-{value}"),
        SourceModifier::X2 => format!("{value}_x2"),
        SourceModifier::X2AndNegate => format!("-{value}_x2"),
        SourceModifier::DivideByZ => format!("{value}_dz"),
        SourceModifier::DivideByW => format!("{value}_dw"),
        SourceModifier::Not => format!("!{value}"),
        SourceModifier::Unknown(_) => value,
    }
}

fn format_dest_param(inst: &Instruction) -> String {
    let Some(reg) = inst.dest_register() else { return "<?>".to_string(); };
    format!("{}{}", asm_register_name(reg), mask_suffix(inst.dest_write_mask()))
}

fn format_decl(inst: &Instruction) -> String {
    let Some(reg) = inst.dest_register() else { return "dcl".to_string(); };
    if reg.ty == RegisterType::Sampler {
        return format!("dcl_{} {}", inst.decl_sampler_type().asm_name(), asm_register_name(reg));
    }
    let mut sem = inst.decl_usage().semantic_prefix().to_ascii_lowercase();
    let idx = inst.decl_index();
    if idx != 0 || inst.decl_usage() == DeclUsage::TexCoord || inst.decl_usage() == DeclUsage::Color {
        sem.push_str(&idx.to_string());
    }
    format!("dcl_{} {}", sem, asm_register_name(reg))
}

fn format_def(inst: &Instruction) -> String {
    let Some(reg) = inst.dest_register() else { return "def".to_string(); };
    match inst.opcode {
        Opcode::Def => format!(
            "def {}, {:.9}, {:.9}, {:.9}, {:.9}",
            asm_register_name(reg),
            inst.get_float_param(1),
            inst.get_float_param(2),
            inst.get_float_param(3),
            inst.get_float_param(4)
        ),
        Opcode::DefI => format!(
            "defi {}, {}, {}, {}, {}",
            asm_register_name(reg),
            inst.get_int_param(1),
            inst.get_int_param(2),
            inst.get_int_param(3),
            inst.get_int_param(4)
        ),
        Opcode::DefB => format!("defb {}, {}", asm_register_name(reg), inst.get_int_param(1)),
        _ => String::new(),
    }
}

pub fn format_instruction_asm(inst: &Instruction) -> Option<String> {
    match inst.opcode {
        Opcode::Comment => None,
        Opcode::End => Some("end".to_string()),
        Opcode::Dcl => Some(format_decl(inst)),
        Opcode::Def | Opcode::DefI | Opcode::DefB => Some(format_def(inst)),
        Opcode::Nop => Some("nop".to_string()),
        Opcode::If | Opcode::Rep | Opcode::Loop => Some(format!("{} {}", inst.opcode.mnemonic(), format_source_param(inst, 0, 1))),
        Opcode::IfC | Opcode::BreakC => Some(format!("{} {}, {}", inst.opcode.mnemonic(), format_source_param(inst, 0, 4), format_source_param(inst, 1, 4))),
        Opcode::Else | Opcode::EndIf | Opcode::Break | Opcode::EndLoop | Opcode::EndRep | Opcode::Ret | Opcode::Phase => Some(inst.opcode.mnemonic().to_string()),
        _ if inst.opcode.has_destination() => {
            let dest = format_dest_param(inst);
            let mut parts = vec![dest];
            let first_src = if inst.opcode == Opcode::Dcl { 2 } else { 1 };
            let count = mask_len(inst.dest_write_mask());
            for i in first_src..inst.params.len() {
                parts.push(format_source_param(inst, i, count));
            }
            Some(format!("{} {}", inst.opcode.mnemonic(), parts.join(", ")))
        }
        _ => Some(inst.opcode.mnemonic().to_string()),
    }
}

pub fn disassemble(data: &[u8]) -> String {
    match parse_shader(data) {
        Ok(shader) => disassemble_model(&shader),
        Err(_) => String::new(),
    }
}

pub fn disassemble_model(shader: &ShaderModel) -> String {
    let mut out = String::new();
    out.push_str(&format!("{}_{}_{}\n", shader.kind.profile_prefix(), shader.major, shader.minor));
    for inst in &shader.instructions {
        if let Some(line) = format_instruction_asm(inst) {
            out.push_str(&line);
            out.push('\n');
        }
    }
    out
}
