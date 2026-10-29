//! Public generated Siglus project data shared by native tests.
//!
//! Only `siglus_scene_vm` public paths are used, so other crates can include
//! this file with `#[path]`.
#![allow(dead_code)]

use siglus_scene_vm::{formats::keys::GAMEEXE_KEY, runtime::constants::cd};

pub fn put(out: &mut [u8], word: usize, value: usize) {
    out[word * 4..word * 4 + 4].copy_from_slice(&(value as i32).to_le_bytes());
}

pub fn put_at(out: &mut [u8], offset: usize, value: usize) {
    out[offset..offset + 4].copy_from_slice(&(value as i32).to_le_bytes());
}

/// Native LZSS container holding only literal runs.
pub fn literals(raw: &[u8]) -> Vec<u8> {
    let mut out = vec![0; 8];
    for chunk in raw.chunks(8) {
        out.push(((1u16 << chunk.len()) - 1) as u8);
        out.extend_from_slice(chunk);
    }
    let len = out.len();
    put(&mut out, 0, len);
    put(&mut out, 1, raw.len());
    out
}

/// Encrypted native Gameexe.dat with the given screen width and menu scene.
pub fn gameexe(width: usize, menu: &str) -> Vec<u8> {
    let text = format!(
        "#SCREEN_SIZE={width},720\r\n#START_SCENE=\"start\",0\r\n#MENU_SCENE=\"{menu}\",0\r\n"
    );
    let utf16: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let packed = literals(&utf16);
    let mut out = vec![0; 8]; // native header version=0, exe-key mode=0
    out.extend(
        packed
            .iter()
            .enumerate()
            .map(|(i, b)| b ^ GAMEEXE_KEY[i % 256]),
    );
    out
}

/// One compiled scene: `code` starts at z-label 0; strings use the native XOR codec.
pub fn scene_chunk(code: &[u8], strings: &[&str]) -> Vec<u8> {
    let index = 132 + code.len();
    let string_base = index + strings.len() * 8;
    let mut out = vec![0; string_base];
    out[132..index].copy_from_slice(code);
    let mut units = 0;
    for (id, text) in strings.iter().enumerate() {
        let chars: Vec<u16> = text.encode_utf16().collect();
        put_at(&mut out, index + id * 8, units);
        put_at(&mut out, index + id * 8 + 4, chars.len());
        for ch in &chars {
            out.extend_from_slice(&(ch ^ (28807u32.wrapping_mul(id as u32) as u16)).to_le_bytes());
        }
        units += chars.len();
    }
    let flags = out.len();
    out.extend_from_slice(&42i32.to_le_bytes());
    let labels = out.len();
    out.extend_from_slice(&0i32.to_le_bytes());
    let end = out.len();
    for word in [3, 5, 7, 9, 11, 13, 15, 17, 19, 21, 23, 25, 27, 29, 31] {
        put(&mut out, word, end);
    }
    put(&mut out, 0, 132);
    put(&mut out, 1, 132);
    put(&mut out, 2, code.len());
    put(&mut out, 3, index);
    put(&mut out, 4, strings.len());
    put(&mut out, 5, string_base);
    put(&mut out, 6, strings.len());
    put(&mut out, 7, labels);
    put(&mut out, 8, 1);
    put(&mut out, 9, labels);
    put(&mut out, 10, 1);
    put(&mut out, 31, flags);
    put(&mut out, 32, 1);
    out
}

/// Uncompressed easy-link Scene.pck containing the named scene chunks in order.
pub fn scene_pack(scenes: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let name_index = 92;
    let name_data = name_index + scenes.len() * 8;
    let mut out = vec![0; name_data];
    let mut units = 0;
    for (i, (name, _)) in scenes.iter().enumerate() {
        put(&mut out, name_index / 4 + i * 2, units);
        put(&mut out, name_index / 4 + i * 2 + 1, name.len());
        for ch in name.encode_utf16() {
            out.extend_from_slice(&ch.to_le_bytes());
        }
        units += name.len();
    }
    // Header offsets are byte offsets, not necessarily four-byte aligned.
    let data_index = out.len();
    out.resize(data_index + scenes.len() * 8, 0);
    let data_base = out.len();
    for (i, (_, chunk)) in scenes.iter().enumerate() {
        let offset = out.len() - data_base;
        out[data_index + i * 8..data_index + i * 8 + 4]
            .copy_from_slice(&(offset as i32).to_le_bytes());
        out[data_index + i * 8 + 4..data_index + i * 8 + 8]
            .copy_from_slice(&(chunk.len() as i32).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    put(&mut out, 0, 92);
    for word in [1, 3, 5, 7, 9, 11] {
        put(&mut out, word, 92);
    }
    put(&mut out, 13, name_index);
    put(&mut out, 14, scenes.len());
    put(&mut out, 15, name_data);
    put(&mut out, 16, scenes.len());
    put(&mut out, 17, data_index);
    put(&mut out, 18, scenes.len());
    put(&mut out, 19, data_base);
    put(&mut out, 20, scenes.len());
    out // native uncompressed/easy-link scene mode; strings retain the normal XOR codec
}

/// Bytecode assembler for the few statements the lifecycle fixtures need.
#[derive(Default)]
pub struct Code(pub Vec<u8>);

impl Code {
    fn word(&mut self, value: i32) -> &mut Self {
        self.0.extend_from_slice(&value.to_le_bytes());
        self
    }

    pub fn line(&mut self, line: i32) -> &mut Self {
        self.0.push(cd::NL);
        self.word(line)
    }

    /// `element(int args...)` as a void CD_COMMAND.
    pub fn command(&mut self, element: &[i32], args: &[i32]) -> &mut Self {
        const FM_INT: i32 = 10;
        const FM_VOID: i32 = 0;
        self.0.push(cd::ELM_POINT);
        for &code in element.iter().chain(args) {
            self.0.push(cd::PUSH);
            self.word(FM_INT).word(code);
        }
        self.0.push(cd::COMMAND);
        self.word(0).word(args.len() as i32);
        for _ in args {
            self.word(FM_INT);
        }
        self.word(0).word(FM_VOID)
    }

    pub fn eof(&mut self) -> Vec<u8> {
        self.0.push(cd::EOF);
        std::mem::take(&mut self.0)
    }
}
