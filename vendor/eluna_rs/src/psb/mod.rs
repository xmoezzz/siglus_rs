use flate2::read::ZlibDecoder;
use lz4_flex::frame::FrameDecoder;
use std::error::Error;
use std::fmt;
use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;

pub const PSB_SIGNATURE: u32 = 0x0042_5350;
pub const MDF_SIGNATURE: [u8; 3] = *b"mdf";
pub const LZ4_FRAME_SIGNATURE: u32 = 0x184D_2204;

const PSB_TYPE_NULL: u8 = 0x01;
const PSB_TYPE_FALSE: u8 = 0x02;
const PSB_TYPE_TRUE: u8 = 0x03;
const PSB_TYPE_INTEGER_N: u8 = 0x04;
const PSB_TYPE_INTEGER_ARRAY_N: u8 = 0x0c;
const PSB_TYPE_STRING_N: u8 = 0x14;
const PSB_TYPE_RESOURCE_N: u8 = 0x18;
const PSB_TYPE_FLOAT0: u8 = 0x1d;
const PSB_TYPE_FLOAT: u8 = 0x1e;
const PSB_TYPE_DOUBLE: u8 = 0x1f;
const PSB_TYPE_LIST: u8 = 0x20;
const PSB_TYPE_OBJECT: u8 = 0x21;

// Some PSB variants place extra-resource references directly after the object
// tag range. The old PSB code used only TYPE_RESOURCE_N1..N4. Keep this path
// explicit and non-overlapping with TYPE_OBJECT.
const PSB_TYPE_EXTRA_RESOURCE_N: u8 = 0x21;

const PSB_COMPILER_INTEGER: u8 = 0x80;
const PSB_COMPILER_STRING: u8 = 0x81;
const PSB_COMPILER_RESOURCE: u8 = 0x82;
const PSB_COMPILER_DECIMAL: u8 = 0x83;
const PSB_COMPILER_ARRAY: u8 = 0x84;
const PSB_COMPILER_BOOL: u8 = 0x85;
const PSB_COMPILER_BINARY_TREE: u8 = 0x86;

const PSB_FLAG_HEADER_ENCRYPTED: u16 = 0x0001;
const PSB_FLAG_BODY_ENCRYPTED: u16 = 0x0002;
const PSB_HEADER_FIXED_SIZE_V2: usize = 0x28;
const PSB_HEADER_ENCRYPTED_BYTES_V2: usize = 0x20;
const PSB_HEADER_ENCRYPTED_BYTES_V3: usize = 0x24;
const MAX_WRAPPER_DEPTH: usize = 4;
const MAX_VALUE_DEPTH: usize = 512;

#[derive(Debug, Clone)]
pub struct PsbFile {
    pub header: PsbHeader,
    pub version: u16,
    pub encrypted: bool,
    /// Historical field kept for compatibility with earlier eluna scaffolds.
    /// In Emote PSB v3 this is the header Adler-32 checksum word at offset 0x28.
    pub checksum: Option<u32>,
    pub names: Vec<String>,
    pub strings: Vec<String>,
    pub resources: Vec<PsbResourceRange>,
    pub extra_resources: Vec<PsbResourceRange>,
    pub root: PsbValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PsbHeader {
    pub signature: u32,
    pub version: u16,
    pub flags: u16,
    /// Header/encryption offset field from the legacy PSB reader.
    pub header_offset: u32,
    /// Name btree table offset.
    pub name_offset: u32,
    /// String offset array table offset.
    pub string_offset: u32,
    /// String pool offset.
    pub string_data_offset: u32,
    /// Resource chunk-offset array table offset.
    pub resource_offset: u32,
    /// Resource chunk-length array table offset.
    pub resource_length_offset: u32,
    /// Resource data pool offset.
    pub resource_data_offset: u32,
    /// Root value bytecode offset.
    pub root_offset: u32,
    /// Header checksum word present in PSB v3 headers.
    ///
    /// The value is Adler-32 over the little-endian offset fields at
    /// `0x08..0x28`. It is read after header decryption when the header
    /// encryption flag is set.
    pub extra: Option<u32>,
}

impl PsbHeader {
    pub fn read(data: &[u8]) -> Result<Self, PsbError> {
        if data.len() < PSB_HEADER_FIXED_SIZE_V2 {
            return Err(PsbError::UnexpectedEof);
        }

        let signature = read_u32_at(data, 0)?;
        if signature != PSB_SIGNATURE {
            return Err(PsbError::InvalidSignature(signature));
        }

        let version = read_u16_at(data, 4)?;
        let flags = read_u16_at(data, 6)?;
        let header_offset = read_u32_at(data, 8)?;
        let name_offset = read_u32_at(data, 12)?;
        let string_offset = read_u32_at(data, 16)?;
        let string_data_offset = read_u32_at(data, 20)?;
        let resource_offset = read_u32_at(data, 24)?;
        let resource_length_offset = read_u32_at(data, 28)?;
        let resource_data_offset = read_u32_at(data, 32)?;
        let root_offset = read_u32_at(data, 36)?;
        let extra = if version >= 3 && data.len() >= PSB_HEADER_FIXED_SIZE_V2 + 4 {
            Some(read_u32_at(data, 40)?)
        } else {
            None
        };

        Ok(Self {
            signature,
            version,
            flags,
            header_offset,
            name_offset,
            string_offset,
            string_data_offset,
            resource_offset,
            resource_length_offset,
            resource_data_offset,
            root_offset,
            extra,
        })
    }

    pub fn stored_checksum(self) -> Option<u32> {
        self.extra
    }

    pub fn calculated_checksum_from_header_bytes(data: &[u8]) -> Result<Option<u32>, PsbError> {
        let header = Self::read(data)?;
        if header.version != 3 {
            return Ok(None);
        }
        let bytes = data.get(0x08..0x28).ok_or(PsbError::UnexpectedEof)?;
        Ok(Some(adler32(bytes)))
    }

    pub fn verify_checksum_from_header_bytes(data: &[u8]) -> Result<Option<bool>, PsbError> {
        let header = Self::read(data)?;
        let Some(stored) = header.stored_checksum() else {
            return Ok(None);
        };
        let Some(calculated) = Self::calculated_checksum_from_header_bytes(data)? else {
            return Ok(None);
        };
        Ok(Some(stored == calculated))
    }

    pub fn has_encryption_flag(self) -> bool {
        self.flags & (PSB_FLAG_HEADER_ENCRYPTED | PSB_FLAG_BODY_ENCRYPTED) != 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PsbResourceRange {
    pub offset: u64,
    pub length: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PsbValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f32),
    Double(f64),
    String(String),
    Resource(u32),
    ExtraResource(u32),
    List(Vec<PsbValue>),
    Object(Vec<(String, PsbValue)>),
    Compiler(PsbCompilerTag),
}

impl PsbValue {
    pub fn as_object(&self) -> Option<&[(String, PsbValue)]> {
        match self {
            PsbValue::Object(fields) => Some(fields),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[PsbValue]> {
        match self {
            PsbValue::List(values) => Some(values),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            PsbValue::String(value) => Some(value.as_str()),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            PsbValue::Int(value) => Some(*value),
            PsbValue::Float(value) => Some(*value as i64),
            PsbValue::Double(value) => Some(*value as i64),
            _ => None,
        }
    }

    pub fn as_u32(&self) -> Option<u32> {
        match self {
            PsbValue::Int(value) if *value >= 0 && *value <= u32::MAX as i64 => Some(*value as u32),
            PsbValue::Resource(index) => Some(*index),
            PsbValue::ExtraResource(index) => Some(*index),
            _ => None,
        }
    }

    pub fn as_f32(&self) -> Option<f32> {
        match self {
            PsbValue::Int(value) => Some(*value as f32),
            PsbValue::Float(value) => Some(*value),
            PsbValue::Double(value) => Some(*value as f32),
            _ => None,
        }
    }

    pub fn field(&self, name: &str) -> Option<&PsbValue> {
        self.as_object()?
            .iter()
            .find_map(|(key, value)| (key == name).then_some(value))
    }

    pub fn field_str(&self, name: &str) -> Option<&str> {
        self.field(name)?.as_str()
    }

    pub fn field_i64(&self, name: &str) -> Option<i64> {
        self.field(name)?.as_i64()
    }

    pub fn field_u32(&self, name: &str) -> Option<u32> {
        self.field(name)?.as_u32()
    }

    pub fn field_f32(&self, name: &str) -> Option<f32> {
        self.field(name)?.as_f32()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PsbCompilerTag {
    Integer,
    String,
    Resource,
    Decimal,
    Array,
    Bool,
    BinaryTree,
}

pub const EMOTE_PSB_KEY0: u32 = 0x075B_CD15;
pub const EMOTE_PSB_KEY1: u32 = 0x159A_55E5;
pub const EMOTE_PSB_KEY2: u32 = 0x1F12_3BB5;
pub const EMOTE_PSB_KEY4: u32 = 0;
pub const EMOTE_PSB_KEY5: u32 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PsbDecryptionKey {
    pub words: [u32; 6],
}

impl PsbDecryptionKey {
    pub const fn new(key0: u32, key1: u32, key2: u32, key3: u32, key4: u32, key5: u32) -> Self {
        Self {
            words: [key0, key1, key2, key3, key4, key5],
        }
    }

    pub const fn from_words(words: [u32; 6]) -> Self {
        Self { words }
    }

    pub const fn emote_key(key: u32) -> Self {
        Self::new(
            EMOTE_PSB_KEY0,
            EMOTE_PSB_KEY1,
            EMOTE_PSB_KEY2,
            key,
            EMOTE_PSB_KEY4,
            EMOTE_PSB_KEY5,
        )
    }

    pub const fn as_words(self) -> [u32; 6] {
        self.words
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PsbNormalizeOptions {
    pub decrypt_key: Option<PsbDecryptionKey>,
    pub decode_mdf: bool,
    pub decode_lz4: bool,
}

impl Default for PsbNormalizeOptions {
    fn default() -> Self {
        Self {
            decrypt_key: None,
            decode_mdf: true,
            decode_lz4: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PsbBruteforceOptions {
    /// Inclusive start key.
    pub start_key: u32,
    /// Inclusive end key.
    pub end_key: u32,
    /// Number of worker threads. `0` means use available parallelism.
    pub threads: usize,
    pub decode_mdf: bool,
    pub decode_lz4: bool,
}

impl Default for PsbBruteforceOptions {
    fn default() -> Self {
        Self {
            start_key: 0,
            end_key: u32::MAX,
            threads: 0,
            decode_mdf: true,
            decode_lz4: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PsbBruteforceResult {
    pub key: u32,
    pub tested_keys: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PsbError {
    UnexpectedEof,
    InvalidSignature(u32),
    EncryptedPsbRequiresKey,
    InvalidHeaderOffset { field: &'static str, offset: u64 },
    InvalidBruteforceRange { start: u32, end: u32 },
    InvalidOffset(u64),
    InvalidUtf8,
    InvalidValueType(u8),
    InvalidArrayType(u8),
    InvalidIndex { table: &'static str, index: u64 },
    MismatchedObjectArrays { names: usize, values: usize },
    MismatchedResourceArrays { offsets: usize, lengths: usize },
    MdfDecompressionFailed,
    MdfSizeMismatch { expected: usize, actual: usize },
    Lz4DecompressionFailed,
    WrapperDepthExceeded,
    RecursionLimit,
    IntegerOverflow,
}

impl fmt::Display for PsbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PsbError::UnexpectedEof => write!(f, "unexpected end of input"),
            PsbError::InvalidSignature(sig) => write!(f, "invalid PSB signature: 0x{sig:08x}"),
            PsbError::EncryptedPsbRequiresKey => write!(f, "encrypted PSB requires --key <u32>"),
            PsbError::InvalidHeaderOffset { field, offset } => {
                write!(f, "invalid PSB header offset {field}=0x{offset:x}")
            }
            PsbError::InvalidBruteforceRange { start, end } => {
                write!(
                    f,
                    "invalid brute-force range: start 0x{start:08x} > end 0x{end:08x}"
                )
            }
            PsbError::InvalidOffset(off) => write!(f, "invalid PSB offset: 0x{off:x}"),
            PsbError::InvalidUtf8 => write!(f, "invalid UTF-8 string"),
            PsbError::InvalidValueType(ty) => write!(f, "invalid PSB value type: 0x{ty:02x}"),
            PsbError::InvalidArrayType(ty) => write!(f, "invalid PSB array type: 0x{ty:02x}"),
            PsbError::InvalidIndex { table, index } => {
                write!(f, "invalid PSB index {index} in {table}")
            }
            PsbError::MismatchedObjectArrays { names, values } => {
                write!(
                    f,
                    "object name/value array length mismatch: {names} != {values}"
                )
            }
            PsbError::MismatchedResourceArrays { offsets, lengths } => {
                write!(
                    f,
                    "resource offset/length array length mismatch: {offsets} != {lengths}"
                )
            }
            PsbError::MdfDecompressionFailed => write!(f, "MDF zlib decompression failed"),
            PsbError::MdfSizeMismatch { expected, actual } => {
                write!(
                    f,
                    "MDF decompressed size mismatch: expected {expected}, got {actual}"
                )
            }
            PsbError::Lz4DecompressionFailed => write!(f, "LZ4 frame decompression failed"),
            PsbError::WrapperDepthExceeded => write!(f, "nested PSB wrapper depth exceeded"),
            PsbError::RecursionLimit => write!(f, "PSB value recursion limit exceeded"),
            PsbError::IntegerOverflow => write!(f, "integer overflow while parsing PSB"),
        }
    }
}

impl Error for PsbError {}

/// Converts an input blob into a plain PSB byte buffer.
///
/// This follows the legacy reader structure:
/// - unwrap MDF zlib and LZ4 frame containers before PSB parsing;
/// - if PSB header/body encryption flags are set, decrypt with the supplied
///   Emote key state `0x075BCD15, 0x159A55E5, 0x1F123BB5, key, 0, 0`;
/// - for PSB v2 files that do not set the body-encrypted bit, optionally try
///   body decryption when the root-code byte is not an object tag and a key was
///   supplied.
pub fn normalize_psb_input(
    data: &[u8],
    options: &PsbNormalizeOptions,
) -> Result<Vec<u8>, PsbError> {
    let mut plain = decode_wrappers(data, options)?;
    apply_psb_decryption(&mut plain, options.decrypt_key)?;
    Ok(plain)
}

/// Standard Adler-32, used by PSB v3 header checksums and MDF wrappers.
pub fn adler32(bytes: &[u8]) -> u32 {
    const MOD: u32 = 65_521;
    let mut a: u32 = 1;
    let mut b: u32 = 0;

    for &byte in bytes {
        a += byte as u32;
        if a >= MOD {
            a -= MOD;
        }
        b += a;
        b %= MOD;
    }

    (b << 16) | a
}

/// Brute-forces the single changing Emote private key DWORD.
///
/// This function first unwraps MDF/LZ4 containers, then tests candidates with
/// cheap structural checks. For PSB v3 files with encrypted headers, the
/// decrypted header Adler-32 is the primary fast reject. Candidate keys that
/// survive the fast reject are confirmed by full decryption plus full PSB parse.
pub fn bruteforce_emote_key(
    data: &[u8],
    options: PsbBruteforceOptions,
) -> Result<Option<PsbBruteforceResult>, PsbError> {
    if options.start_key > options.end_key {
        return Err(PsbError::InvalidBruteforceRange {
            start: options.start_key,
            end: options.end_key,
        });
    }

    let normalize = PsbNormalizeOptions {
        decrypt_key: None,
        decode_mdf: options.decode_mdf,
        decode_lz4: options.decode_lz4,
    };
    let encrypted = Arc::new(decode_wrappers(data, &normalize)?);
    let initial_header = PsbHeader::read(&encrypted)?;

    if !initial_header.has_encryption_flag()
        && root_code_is_plain_object(&encrypted, initial_header.root_offset)
    {
        return Ok(None);
    }

    let available_threads = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let requested_threads = if options.threads == 0 {
        available_threads
    } else {
        options.threads
    };
    let thread_count = requested_threads.max(1);

    let start = options.start_key as u64;
    let end = options.end_key as u64;
    let total = end - start + 1;
    let worker_count = thread_count.min(total as usize);
    let chunk = (total + worker_count as u64 - 1) / worker_count as u64;

    let found = Arc::new(AtomicBool::new(false));
    let found_key = Arc::new(AtomicU32::new(0));
    let mut handles = Vec::with_capacity(worker_count);

    for worker in 0..worker_count {
        let worker_start = start + chunk * worker as u64;
        if worker_start > end {
            break;
        }
        let worker_end_exclusive = (worker_start + chunk).min(end + 1);
        let data = Arc::clone(&encrypted);
        let found = Arc::clone(&found);
        let found_key = Arc::clone(&found_key);

        handles.push(thread::spawn(move || {
            let mut tested = 0u64;
            let mut key = worker_start;
            while key < worker_end_exclusive {
                if (tested & 0x3fff) == 0 && found.load(Ordering::Relaxed) {
                    break;
                }

                let key32 = key as u32;
                if emote_key_candidate_fast_check(&data, key32)
                    && emote_key_candidate_full_check(&data, key32)
                {
                    found_key.store(key32, Ordering::Relaxed);
                    found.store(true, Ordering::Release);
                    tested += 1;
                    break;
                }

                tested += 1;
                key += 1;
            }
            tested
        }));
    }

    let mut tested_keys = 0u64;
    for handle in handles {
        if let Ok(tested) = handle.join() {
            tested_keys = tested_keys.saturating_add(tested);
        }
    }

    if found.load(Ordering::Acquire) {
        Ok(Some(PsbBruteforceResult {
            key: found_key.load(Ordering::Relaxed),
            tested_keys,
        }))
    } else {
        Ok(None)
    }
}

impl PsbFile {
    pub fn parse(data: &[u8]) -> Result<Self, PsbError> {
        let header = PsbHeader::read(data)?;
        validate_header_offsets(data, header)?;

        let names = read_name_btree_at(data, header.name_offset as u64)?;
        let strings = read_strings_at(
            data,
            header.string_offset as u64,
            header.string_data_offset as u64,
        )?;
        let resources = read_resource_ranges_at(
            data,
            header.resource_offset as u64,
            header.resource_length_offset as u64,
            header.resource_data_offset as u64,
        )?;
        let extra_resources = Vec::new();
        let root = read_value_at(data, &names, &strings, header.root_offset as u64, 0)?;

        Ok(Self {
            header,
            version: header.version,
            encrypted: header.has_encryption_flag(),
            checksum: header.extra,
            names,
            strings,
            resources,
            extra_resources,
            root,
        })
    }

    pub fn parse_normalized(
        data: &[u8],
        options: &PsbNormalizeOptions,
    ) -> Result<(Vec<u8>, Self), PsbError> {
        let plain = normalize_psb_input(data, options)?;
        let psb = Self::parse(&plain)?;
        Ok((plain, psb))
    }

    pub fn resource_bytes<'a>(&self, data: &'a [u8], index: usize) -> Option<&'a [u8]> {
        let range = *self.resources.get(index)?;
        slice_range(data, range).ok()
    }

    pub fn extra_resource_bytes<'a>(&self, data: &'a [u8], index: usize) -> Option<&'a [u8]> {
        let range = *self.extra_resources.get(index)?;
        slice_range(data, range).ok()
    }
}

fn decode_wrappers(data: &[u8], options: &PsbNormalizeOptions) -> Result<Vec<u8>, PsbError> {
    let mut current = data.to_vec();

    for _ in 0..MAX_WRAPPER_DEPTH {
        if starts_with_psb(&current) {
            return Ok(current);
        }

        if options.decode_mdf && starts_with_mdf(&current) {
            current = decode_mdf(&current)?;
            continue;
        }

        if options.decode_lz4 && starts_with_lz4_frame(&current) {
            current = decode_lz4_frame(&current)?;
            continue;
        }

        let signature = if current.len() >= 4 {
            read_u32_at(&current, 0)?
        } else {
            0
        };
        return Err(PsbError::InvalidSignature(signature));
    }

    Err(PsbError::WrapperDepthExceeded)
}

fn starts_with_psb(data: &[u8]) -> bool {
    data.len() >= 4 && read_u32_at(data, 0).ok() == Some(PSB_SIGNATURE)
}

fn starts_with_mdf(data: &[u8]) -> bool {
    data.len() >= 8 && &data[0..3] == b"mdf"
}

fn starts_with_lz4_frame(data: &[u8]) -> bool {
    data.len() >= 4 && read_u32_at(data, 0).ok() == Some(LZ4_FRAME_SIGNATURE)
}

fn decode_mdf(data: &[u8]) -> Result<Vec<u8>, PsbError> {
    if data.len() < 8 {
        return Err(PsbError::UnexpectedEof);
    }

    let expected = read_u32_at(data, 4)? as usize;
    let mut decoder = ZlibDecoder::new(&data[8..]);
    let mut out = Vec::with_capacity(expected);
    decoder
        .read_to_end(&mut out)
        .map_err(|_| PsbError::MdfDecompressionFailed)?;

    if out.len() != expected {
        return Err(PsbError::MdfSizeMismatch {
            expected,
            actual: out.len(),
        });
    }

    Ok(out)
}

fn decode_lz4_frame(data: &[u8]) -> Result<Vec<u8>, PsbError> {
    let mut decoder = FrameDecoder::new(data);
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .map_err(|_| PsbError::Lz4DecompressionFailed)?;
    Ok(out)
}

fn emote_key_candidate_fast_check(data: &[u8], key: u32) -> bool {
    let mut header = match PsbHeader::read(data) {
        Ok(header) => header,
        Err(_) => return false,
    };
    let mut cipher = PsbDecryptor::new(PsbDecryptionKey::emote_key(key));

    if header.flags & PSB_FLAG_HEADER_ENCRYPTED != 0 {
        let encrypted_header_len = if header.version >= 3 {
            PSB_HEADER_ENCRYPTED_BYTES_V3
        } else {
            PSB_HEADER_ENCRYPTED_BYTES_V2
        };
        let end = match 0x8usize.checked_add(encrypted_header_len) {
            Some(end) => end,
            None => return false,
        };
        let Some(header_src) = data.get(..end) else {
            return false;
        };
        let mut header_bytes = header_src.to_vec();
        cipher.decrypt(&mut header_bytes[0x8..end]);
        header = match PsbHeader::read(&header_bytes) {
            Ok(header) => header,
            Err(_) => return false,
        };
        if validate_header_offsets(data, header).is_err() {
            return false;
        }
        if header.version == 3 {
            match PsbHeader::verify_checksum_from_header_bytes(&header_bytes) {
                Ok(Some(true)) => {}
                _ => return false,
            }
        }

        // For v3 header-encrypted PSB files, the decrypted header checksum is
        // already a strong 32-bit filter. Do not also require the first 512
        // body bytes to contain all name-btree arrays: real files can have a
        // large first array, and that stricter check rejects the correct key.
        // The full candidate check below will continue with the same cipher
        // state and perform complete PSB decryption/parsing.
        return true;
    } else if validate_header_offsets(data, header).is_err() {
        return false;
    }

    let should_decrypt_body = (header.flags & PSB_FLAG_BODY_ENCRYPTED != 0)
        || !root_code_is_plain_object(data, header.root_offset);
    if !should_decrypt_body {
        return false;
    }

    let start = header.name_offset as usize;
    let end = header.resource_offset as usize;
    if start >= end || end > data.len() {
        return false;
    }

    let sample_len = (end - start).min(512);
    if sample_len < 4 {
        return false;
    }
    let mut sample = [0u8; 512];
    sample[..sample_len].copy_from_slice(&data[start..start + sample_len]);
    cipher.decrypt(&mut sample[..sample_len]);

    // This branch is only for body-only encryption. Keep a cheap structural
    // filter, but do not make it part of the v3 encrypted-header path above.
    if !looks_like_psb_body_prefix(&sample[..sample_len], end - start) {
        return false;
    }

    let root_offset = header.root_offset as usize;
    if root_offset >= start && root_offset < start + sample_len {
        if sample[root_offset - start] != PSB_TYPE_OBJECT {
            return false;
        }
    }

    true
}

fn emote_key_candidate_full_check(data: &[u8], key: u32) -> bool {
    let mut plain = data.to_vec();
    let decrypt_key = PsbDecryptionKey::emote_key(key);
    if apply_psb_decryption(&mut plain, Some(decrypt_key)).is_err() {
        return false;
    }
    PsbFile::parse(&plain).is_ok()
}

fn looks_like_psb_body_prefix(sample: &[u8], total_body_len: usize) -> bool {
    let Some(first) = parse_uint_array_prefix(sample, 0, total_body_len) else {
        return false;
    };

    if first.length == 0 || first.length > 1_000_000 || first.item_size == 0 || first.item_size > 4
    {
        return false;
    }

    if first.total_size < sample.len() {
        let Some(second) = parse_uint_array_prefix(sample, first.total_size, total_body_len) else {
            return false;
        };
        if second.length == 0
            || second.length > 8_000_000
            || second.item_size == 0
            || second.item_size > 4
        {
            return false;
        }

        let third_offset = match first.total_size.checked_add(second.total_size) {
            Some(value) => value,
            None => return false,
        };
        if third_offset < sample.len() {
            let Some(third) = parse_uint_array_prefix(sample, third_offset, total_body_len) else {
                return false;
            };
            if third.length == 0
                || third.length > 1_000_000
                || third.item_size == 0
                || third.item_size > 4
            {
                return false;
            }
        }
    }

    true
}

#[derive(Debug, Clone, Copy)]
struct UintArrayPrefix {
    length: u64,
    item_size: u8,
    total_size: usize,
}

fn parse_uint_array_prefix(
    sample: &[u8],
    offset: usize,
    max_total_size: usize,
) -> Option<UintArrayPrefix> {
    let len_ty = *sample.get(offset)?;
    if !(PSB_TYPE_INTEGER_ARRAY_N + 1..=PSB_TYPE_INTEGER_ARRAY_N + 8).contains(&len_ty) {
        return None;
    }
    let len_size = (len_ty - PSB_TYPE_INTEGER_ARRAY_N) as usize;
    let len_start = offset.checked_add(1)?;
    let item_ty_offset = len_start.checked_add(len_size)?;
    let item_ty = *sample.get(item_ty_offset)?;
    if !(PSB_TYPE_INTEGER_ARRAY_N + 1..=PSB_TYPE_INTEGER_ARRAY_N + 8).contains(&item_ty) {
        return None;
    }
    let item_size = item_ty - PSB_TYPE_INTEGER_ARRAY_N;
    let length = read_partial_u64_from_prefix(sample, len_start, len_size)?;
    let payload_size = (length as usize).checked_mul(item_size as usize)?;
    let total_size = 1usize
        .checked_add(len_size)?
        .checked_add(1)?
        .checked_add(payload_size)?;
    if total_size > max_total_size {
        return None;
    }
    Some(UintArrayPrefix {
        length,
        item_size,
        total_size,
    })
}

fn read_partial_u64_from_prefix(sample: &[u8], offset: usize, size: usize) -> Option<u64> {
    if size > 8 || offset.checked_add(size)? > sample.len() {
        return None;
    }
    let mut bytes = [0u8; 8];
    bytes[..size].copy_from_slice(&sample[offset..offset + size]);
    Some(u64::from_le_bytes(bytes))
}

fn apply_psb_decryption(data: &mut [u8], key: Option<PsbDecryptionKey>) -> Result<(), PsbError> {
    let mut header = PsbHeader::read(data)?;

    if !header.has_encryption_flag() && root_code_is_plain_object(data, header.root_offset) {
        return Ok(());
    }

    let Some(key) = key else {
        return Err(PsbError::EncryptedPsbRequiresKey);
    };

    let mut cipher = PsbDecryptor::new(key);

    if header.flags & PSB_FLAG_HEADER_ENCRYPTED != 0 {
        let encrypted_header_len = if header.version >= 3 {
            PSB_HEADER_ENCRYPTED_BYTES_V3
        } else {
            PSB_HEADER_ENCRYPTED_BYTES_V2
        };
        let start = 0x8usize;
        let end = start
            .checked_add(encrypted_header_len)
            .ok_or(PsbError::IntegerOverflow)?;
        let bytes = data.get_mut(start..end).ok_or(PsbError::UnexpectedEof)?;
        cipher.decrypt(bytes);
        header = PsbHeader::read(data)?;
        validate_header_offsets(data, header)?;
    }

    let should_decrypt_body = (header.flags & PSB_FLAG_BODY_ENCRYPTED != 0)
        || !root_code_is_plain_object(data, header.root_offset);

    if should_decrypt_body {
        let start = header.name_offset as usize;
        let end = header.resource_offset as usize;
        if start > end || end > data.len() {
            return Err(PsbError::InvalidHeaderOffset {
                field: "encrypted_body_range",
                offset: end as u64,
            });
        }
        cipher.decrypt(&mut data[start..end]);
    }

    Ok(())
}

fn root_code_is_plain_object(data: &[u8], root_offset: u32) -> bool {
    data.get(root_offset as usize) == Some(&PSB_TYPE_OBJECT)
}

struct PsbDecryptor {
    state: [u32; 6],
}

impl PsbDecryptor {
    fn new(key: PsbDecryptionKey) -> Self {
        Self {
            state: key.as_words(),
        }
    }

    fn decrypt(&mut self, data: &mut [u8]) {
        for byte in data {
            if self.state[4] == 0 {
                let v5 = self.state[3];
                let v6 = self.state[0] ^ self.state[0].wrapping_shl(11);
                self.state[0] = self.state[1];
                self.state[1] = self.state[2];
                let eax = v6 ^ v5 ^ ((v6 ^ (v5 >> 11)) >> 8);
                self.state[2] = v5;
                self.state[3] = eax;
                self.state[4] = eax;
            }

            *byte ^= self.state[4] as u8;
            self.state[4] >>= 8;
        }
    }
}

fn validate_header_offsets(data: &[u8], header: PsbHeader) -> Result<(), PsbError> {
    validate_offset(data, "name_offset", header.name_offset as u64)?;
    validate_offset(data, "string_offset", header.string_offset as u64)?;
    validate_offset(data, "string_data_offset", header.string_data_offset as u64)?;
    validate_offset(data, "resource_offset", header.resource_offset as u64)?;
    validate_offset(
        data,
        "resource_length_offset",
        header.resource_length_offset as u64,
    )?;
    validate_offset(
        data,
        "resource_data_offset",
        header.resource_data_offset as u64,
    )?;
    validate_offset(data, "root_offset", header.root_offset as u64)?;
    Ok(())
}

fn validate_offset(data: &[u8], field: &'static str, offset: u64) -> Result<(), PsbError> {
    if checked_usize(offset)? > data.len() {
        return Err(PsbError::InvalidHeaderOffset { field, offset });
    }
    Ok(())
}

fn slice_range(data: &[u8], range: PsbResourceRange) -> Result<&[u8], PsbError> {
    let start = checked_usize(range.offset)?;
    let end = checked_usize(
        range
            .offset
            .checked_add(range.length)
            .ok_or(PsbError::IntegerOverflow)?,
    )?;
    data.get(start..end)
        .ok_or(PsbError::InvalidOffset(range.offset))
}

fn read_name_btree_at(data: &[u8], offset: u64) -> Result<Vec<String>, PsbError> {
    let mut r = Reader::new_at(data, offset)?;
    let offsets = read_uint_array(&mut r)?;
    let tree = read_uint_array(&mut r)?;
    let indexes = read_uint_array(&mut r)?;

    let mut out = Vec::with_capacity(indexes.len());
    for index in indexes {
        let mut id = *tree
            .get(checked_usize(index)?)
            .ok_or(PsbError::InvalidIndex {
                table: "name_btree.tree",
                index,
            })?;

        let mut name = Vec::new();
        while id != 0 {
            let next = *tree.get(checked_usize(id)?).ok_or(PsbError::InvalidIndex {
                table: "name_btree.tree",
                index: id,
            })?;
            let offset = *offsets
                .get(checked_usize(next)?)
                .ok_or(PsbError::InvalidIndex {
                    table: "name_btree.offsets",
                    index: next,
                })?;
            let decoded = id.checked_sub(offset).ok_or(PsbError::IntegerOverflow)?;
            if decoded > 0xff {
                return Err(PsbError::InvalidIndex {
                    table: "name_btree.decoded_byte",
                    index: decoded,
                });
            }
            id = next;
            name.push(decoded as u8);
        }

        name.reverse();
        let s = std::str::from_utf8(&name).map_err(|_| PsbError::InvalidUtf8)?;
        out.push(s.to_owned());
    }

    Ok(out)
}

fn read_strings_at(
    data: &[u8],
    string_offset: u64,
    string_data_start: u64,
) -> Result<Vec<String>, PsbError> {
    let mut r = Reader::new_at(data, string_offset)?;
    let offsets = read_uint_array(&mut r)?;
    let mut out = Vec::with_capacity(offsets.len());

    for off in offsets {
        let pos = string_data_start
            .checked_add(off)
            .ok_or(PsbError::IntegerOverflow)?;
        let start = checked_usize(pos)?;
        let bytes = data.get(start..).ok_or(PsbError::InvalidOffset(pos))?;
        let nul = bytes
            .iter()
            .position(|&b| b == 0)
            .ok_or(PsbError::UnexpectedEof)?;
        let s = std::str::from_utf8(&bytes[..nul]).map_err(|_| PsbError::InvalidUtf8)?;
        out.push(s.to_owned());
    }

    Ok(out)
}

fn read_resource_ranges_at(
    data: &[u8],
    resource_offset: u64,
    resource_lengths: u64,
    resource_data_start: u64,
) -> Result<Vec<PsbResourceRange>, PsbError> {
    let mut offset_reader = Reader::new_at(data, resource_offset)?;
    let offsets = read_uint_array(&mut offset_reader)?;

    let mut length_reader = Reader::new_at(data, resource_lengths)?;
    let lengths = read_uint_array(&mut length_reader)?;

    if offsets.len() != lengths.len() {
        return Err(PsbError::MismatchedResourceArrays {
            offsets: offsets.len(),
            lengths: lengths.len(),
        });
    }

    let mut out = Vec::with_capacity(offsets.len());
    for (off, len) in offsets.into_iter().zip(lengths.into_iter()) {
        let absolute = resource_data_start
            .checked_add(off)
            .ok_or(PsbError::IntegerOverflow)?;
        let end = absolute.checked_add(len).ok_or(PsbError::IntegerOverflow)?;
        if checked_usize(end)? > data.len() {
            return Err(PsbError::InvalidOffset(end));
        }
        out.push(PsbResourceRange {
            offset: absolute,
            length: len,
        });
    }

    Ok(out)
}

fn read_value_at(
    data: &[u8],
    names: &[String],
    strings: &[String],
    offset: u64,
    depth: usize,
) -> Result<PsbValue, PsbError> {
    if depth > MAX_VALUE_DEPTH {
        return Err(PsbError::RecursionLimit);
    }
    let mut r = Reader::new_at(data, offset)?;
    read_value(&mut r, names, strings, depth)
}

fn read_value(
    r: &mut Reader<'_>,
    names: &[String],
    strings: &[String],
    depth: usize,
) -> Result<PsbValue, PsbError> {
    let ty = r.read_u8()?;
    match ty {
        PSB_TYPE_NULL => Ok(PsbValue::Null),
        PSB_TYPE_FALSE => Ok(PsbValue::Bool(false)),
        PSB_TYPE_TRUE => Ok(PsbValue::Bool(true)),
        PSB_TYPE_FLOAT0 => Ok(PsbValue::Float(0.0)),
        PSB_TYPE_FLOAT => Ok(PsbValue::Float(r.read_f32()?)),
        PSB_TYPE_DOUBLE => Ok(PsbValue::Double(r.read_f64()?)),
        PSB_TYPE_LIST => read_list_value(r, names, strings, depth),
        PSB_TYPE_OBJECT => read_object_value(r, names, strings, depth),
        PSB_COMPILER_INTEGER => Ok(PsbValue::Compiler(PsbCompilerTag::Integer)),
        PSB_COMPILER_STRING => Ok(PsbValue::Compiler(PsbCompilerTag::String)),
        PSB_COMPILER_RESOURCE => Ok(PsbValue::Compiler(PsbCompilerTag::Resource)),
        PSB_COMPILER_DECIMAL => Ok(PsbValue::Compiler(PsbCompilerTag::Decimal)),
        PSB_COMPILER_ARRAY => Ok(PsbValue::Compiler(PsbCompilerTag::Array)),
        PSB_COMPILER_BOOL => Ok(PsbValue::Compiler(PsbCompilerTag::Bool)),
        PSB_COMPILER_BINARY_TREE => Ok(PsbValue::Compiler(PsbCompilerTag::BinaryTree)),
        ty if (PSB_TYPE_INTEGER_N..=PSB_TYPE_INTEGER_N + 8).contains(&ty) => {
            Ok(PsbValue::Int(r.read_partial_i64(ty - PSB_TYPE_INTEGER_N)?))
        }
        ty if (PSB_TYPE_STRING_N + 1..=PSB_TYPE_STRING_N + 4).contains(&ty) => {
            let index = r.read_partial_u64(ty - PSB_TYPE_STRING_N)?;
            let value = strings
                .get(checked_usize(index)?)
                .ok_or(PsbError::InvalidIndex {
                    table: "strings",
                    index,
                })?;
            Ok(PsbValue::String(value.clone()))
        }
        ty if (PSB_TYPE_RESOURCE_N + 1..=PSB_TYPE_RESOURCE_N + 4).contains(&ty) => {
            let index = r.read_partial_u64(ty - PSB_TYPE_RESOURCE_N)?;
            let index = u32::try_from(index).map_err(|_| PsbError::IntegerOverflow)?;
            Ok(PsbValue::Resource(index))
        }
        ty if (PSB_TYPE_EXTRA_RESOURCE_N + 1..=PSB_TYPE_EXTRA_RESOURCE_N + 4).contains(&ty) => {
            let index = r.read_partial_u64(ty - PSB_TYPE_EXTRA_RESOURCE_N)?;
            let index = u32::try_from(index).map_err(|_| PsbError::IntegerOverflow)?;
            Ok(PsbValue::ExtraResource(index))
        }
        other => Err(PsbError::InvalidValueType(other)),
    }
}

fn read_list_value(
    r: &mut Reader<'_>,
    names: &[String],
    strings: &[String],
    depth: usize,
) -> Result<PsbValue, PsbError> {
    let offsets = read_uint_array(r)?;
    let data_start = r.pos_u64();
    let mut values = Vec::with_capacity(offsets.len());
    for off in offsets {
        let value_pos = data_start
            .checked_add(off)
            .ok_or(PsbError::IntegerOverflow)?;
        values.push(read_value_at(r.data, names, strings, value_pos, depth + 1)?);
    }
    Ok(PsbValue::List(values))
}

fn read_object_value(
    r: &mut Reader<'_>,
    names: &[String],
    strings: &[String],
    depth: usize,
) -> Result<PsbValue, PsbError> {
    let name_ids = read_uint_array(r)?;
    let value_offsets = read_uint_array(r)?;
    if name_ids.len() != value_offsets.len() {
        return Err(PsbError::MismatchedObjectArrays {
            names: name_ids.len(),
            values: value_offsets.len(),
        });
    }

    let data_start = r.pos_u64();
    let mut fields = Vec::with_capacity(name_ids.len());
    for (name_id, value_off) in name_ids.into_iter().zip(value_offsets.into_iter()) {
        let key = names
            .get(checked_usize(name_id)?)
            .ok_or(PsbError::InvalidIndex {
                table: "names",
                index: name_id,
            })?;
        let value_pos = data_start
            .checked_add(value_off)
            .ok_or(PsbError::IntegerOverflow)?;
        let value = read_value_at(r.data, names, strings, value_pos, depth + 1)?;
        fields.push((key.clone(), value));
    }
    Ok(PsbValue::Object(fields))
}

fn read_uint_array(r: &mut Reader<'_>) -> Result<Vec<u64>, PsbError> {
    let len_ty = r.read_u8()?;
    if !(PSB_TYPE_INTEGER_ARRAY_N + 1..=PSB_TYPE_INTEGER_ARRAY_N + 8).contains(&len_ty) {
        return Err(PsbError::InvalidArrayType(len_ty));
    }
    let len = r.read_partial_u64(len_ty - PSB_TYPE_INTEGER_ARRAY_N)?;

    let item_ty = r.read_u8()?;
    if !(PSB_TYPE_INTEGER_ARRAY_N + 1..=PSB_TYPE_INTEGER_ARRAY_N + 8).contains(&item_ty) {
        return Err(PsbError::InvalidArrayType(item_ty));
    }
    let item_size = item_ty - PSB_TYPE_INTEGER_ARRAY_N;

    let mut out = Vec::with_capacity(checked_usize(len)?);
    for _ in 0..len {
        out.push(r.read_partial_u64(item_size)?);
    }
    Ok(out)
}

fn read_u16_at(data: &[u8], offset: usize) -> Result<u16, PsbError> {
    let bytes = data
        .get(offset..offset + 2)
        .ok_or(PsbError::UnexpectedEof)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32_at(data: &[u8], offset: usize) -> Result<u32, PsbError> {
    let bytes = data
        .get(offset..offset + 4)
        .ok_or(PsbError::UnexpectedEof)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn checked_usize(v: u64) -> Result<usize, PsbError> {
    usize::try_from(v).map_err(|_| PsbError::IntegerOverflow)
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new_at(data: &'a [u8], pos: u64) -> Result<Self, PsbError> {
        let pos = checked_usize(pos)?;
        if pos > data.len() {
            return Err(PsbError::InvalidOffset(pos as u64));
        }
        Ok(Self { data, pos })
    }

    fn pos_u64(&self) -> u64 {
        self.pos as u64
    }

    fn read_u8(&mut self) -> Result<u8, PsbError> {
        let value = *self.data.get(self.pos).ok_or(PsbError::UnexpectedEof)?;
        self.pos += 1;
        Ok(value)
    }

    fn read_f32(&mut self) -> Result<f32, PsbError> {
        Ok(f32::from_le_bytes(self.read_array()?))
    }

    fn read_f64(&mut self) -> Result<f64, PsbError> {
        Ok(f64::from_le_bytes(self.read_array()?))
    }

    fn read_partial_u64(&mut self, size: u8) -> Result<u64, PsbError> {
        if size > 8 {
            return Err(PsbError::InvalidArrayType(size));
        }
        let len = size as usize;
        let raw = self.read_slice(len)?;
        let mut bytes = [0u8; 8];
        bytes[..len].copy_from_slice(raw);
        Ok(u64::from_le_bytes(bytes))
    }

    fn read_partial_i64(&mut self, size: u8) -> Result<i64, PsbError> {
        if size > 8 {
            return Err(PsbError::InvalidValueType(size));
        }
        if size == 0 {
            return Ok(0);
        }
        let len = size as usize;
        let raw = self.read_slice(len)?;
        let mut bytes = [0u8; 8];
        bytes[..len].copy_from_slice(raw);
        if bytes[len - 1] & 0x80 != 0 {
            for b in &mut bytes[len..] {
                *b = 0xff;
            }
        }
        Ok(i64::from_le_bytes(bytes))
    }

    fn read_array<const N: usize>(&mut self) -> Result<[u8; N], PsbError> {
        let raw = self.read_slice(N)?;
        let mut out = [0u8; N];
        out.copy_from_slice(raw);
        Ok(out)
    }

    fn read_slice(&mut self, len: usize) -> Result<&'a [u8], PsbError> {
        let end = self.pos.checked_add(len).ok_or(PsbError::IntegerOverflow)?;
        let s = self
            .data
            .get(self.pos..end)
            .ok_or(PsbError::UnexpectedEof)?;
        self.pos = end;
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decryptor_matches_reference_sequence_shape() {
        let key = PsbDecryptionKey::emote_key(0x0102_0304);
        let mut a = [0u8; 16];
        let mut b = [0u8; 16];
        let mut dec1 = PsbDecryptor::new(key);
        let mut dec2 = PsbDecryptor::new(key);
        dec1.decrypt(&mut a);
        dec2.decrypt(&mut b[..8]);
        dec2.decrypt(&mut b[8..]);
        assert_eq!(a, b);
    }
}
