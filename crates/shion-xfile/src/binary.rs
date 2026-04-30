use crate::error::{Error, Result};
use crate::guid::Guid;
use crate::header::{FloatSize, XFileHeader};
use crate::model::{
    PrimitiveValue, ReferenceTarget, Separator, XDataObject, XFile, XObjectElement, XTemplateDef,
    XTemplateMember, XTemplateRestriction,
};

const TOKEN_NAME: u16 = 1;
const TOKEN_STRING: u16 = 2;
const TOKEN_INTEGER: u16 = 3;
const TOKEN_GUID: u16 = 5;
const TOKEN_INTEGER_LIST: u16 = 6;
const TOKEN_FLOAT_LIST: u16 = 7;
const TOKEN_OBRACE: u16 = 10;
const TOKEN_CBRACE: u16 = 11;
const TOKEN_OPAREN: u16 = 12;
const TOKEN_CPAREN: u16 = 13;
const TOKEN_OBRACKET: u16 = 14;
const TOKEN_CBRACKET: u16 = 15;
const TOKEN_OANGLE: u16 = 16;
const TOKEN_CANGLE: u16 = 17;
const TOKEN_DOT: u16 = 18;
const TOKEN_COMMA: u16 = 19;
const TOKEN_SEMICOLON: u16 = 20;
const TOKEN_TEMPLATE: u16 = 31;
const TOKEN_WORD: u16 = 40;
const TOKEN_DWORD: u16 = 41;
const TOKEN_FLOAT: u16 = 42;
const TOKEN_DOUBLE: u16 = 43;
const TOKEN_CHAR: u16 = 44;
const TOKEN_UCHAR: u16 = 45;
const TOKEN_SWORD: u16 = 46;
const TOKEN_SDWORD: u16 = 47;
const TOKEN_VOID: u16 = 48;
const TOKEN_LPSTR: u16 = 49;
const TOKEN_UNICODE: u16 = 50;
const TOKEN_CSTRING: u16 = 51;
const TOKEN_ARRAY: u16 = 52;

#[derive(Debug, Clone, PartialEq)]
pub enum BinaryTokenKind {
    Template,
    OpenBrace,
    CloseBrace,
    OpenParen,
    CloseParen,
    OpenBracket,
    CloseBracket,
    OpenAngle,
    CloseAngle,
    Dot,
    Comma,
    Semicolon,
    Array,
    Identifier(String),
    String(String),
    Integer(i64),
    Float(f64),
    Guid(Guid),
}

#[derive(Debug, Clone, PartialEq)]
pub struct BinaryTokenRecord {
    pub offset: usize,
    pub kind: BinaryTokenKind,
}

pub fn parse_binary_file(header: XFileHeader, body: &[u8]) -> Result<XFile> {
    let tokens = tokenize_binary(body, header.float_size)?;
    let mut parser = Parser {
        tokens: &tokens,
        cursor: 0,
    };
    let mut templates = Vec::new();
    let mut objects = Vec::new();

    while !parser.is_eof() {
        match parser.peek_kind() {
            Some(BinaryTokenKind::Template) => templates.push(parser.parse_template()?),
            Some(BinaryTokenKind::Identifier(_)) => objects.push(parser.parse_data_object()?),
            Some(BinaryTokenKind::Semicolon | BinaryTokenKind::Comma) => {
                parser.bump();
            }
            Some(other) => {
                return Err(Error::Parse(format!(
                    "unexpected binary token at top level: {:?}",
                    other
                )))
            }
            None => break,
        }
    }

    Ok(XFile {
        header,
        templates,
        objects,
    })
}

pub fn tokenize_binary(body: &[u8], float_size: FloatSize) -> Result<Vec<BinaryTokenRecord>> {
    let mut reader = BinaryReader::new(body, float_size);
    let mut tokens = Vec::new();
    while !reader.is_eof() {
        let offset = reader.cursor;
        let token = reader.read_u16()?;
        match token {
            TOKEN_NAME => tokens.push(BinaryTokenRecord {
                offset,
                kind: BinaryTokenKind::Identifier(reader.read_counted_ascii()?),
            }),
            TOKEN_STRING => {
                let s = reader.read_counted_ascii()?;
                reader.consume_string_terminator()?;
                tokens.push(BinaryTokenRecord {
                    offset,
                    kind: BinaryTokenKind::String(s),
                });
            }
            TOKEN_INTEGER => tokens.push(BinaryTokenRecord {
                offset,
                kind: BinaryTokenKind::Integer(reader.read_u32()? as i64),
            }),
            TOKEN_GUID => tokens.push(BinaryTokenRecord {
                offset,
                kind: BinaryTokenKind::Guid(reader.read_guid()?),
            }),
            TOKEN_INTEGER_LIST => {
                let count = reader.read_u32()? as usize;
                for _ in 0..count {
                    tokens.push(BinaryTokenRecord {
                        offset,
                        kind: BinaryTokenKind::Integer(reader.read_u32()? as i64),
                    });
                }
            }
            TOKEN_FLOAT_LIST => {
                let count = reader.read_u32()? as usize;
                for _ in 0..count {
                    tokens.push(BinaryTokenRecord {
                        offset,
                        kind: BinaryTokenKind::Float(reader.read_real()? as f64),
                    });
                }
            }
            TOKEN_OBRACE => tokens.push(BinaryTokenRecord {
                offset,
                kind: BinaryTokenKind::OpenBrace,
            }),
            TOKEN_CBRACE => tokens.push(BinaryTokenRecord {
                offset,
                kind: BinaryTokenKind::CloseBrace,
            }),
            TOKEN_OPAREN => tokens.push(BinaryTokenRecord {
                offset,
                kind: BinaryTokenKind::OpenParen,
            }),
            TOKEN_CPAREN => tokens.push(BinaryTokenRecord {
                offset,
                kind: BinaryTokenKind::CloseParen,
            }),
            TOKEN_OBRACKET => tokens.push(BinaryTokenRecord {
                offset,
                kind: BinaryTokenKind::OpenBracket,
            }),
            TOKEN_CBRACKET => tokens.push(BinaryTokenRecord {
                offset,
                kind: BinaryTokenKind::CloseBracket,
            }),
            TOKEN_OANGLE => tokens.push(BinaryTokenRecord {
                offset,
                kind: BinaryTokenKind::OpenAngle,
            }),
            TOKEN_CANGLE => tokens.push(BinaryTokenRecord {
                offset,
                kind: BinaryTokenKind::CloseAngle,
            }),
            TOKEN_DOT => tokens.push(BinaryTokenRecord {
                offset,
                kind: BinaryTokenKind::Dot,
            }),
            TOKEN_COMMA => tokens.push(BinaryTokenRecord {
                offset,
                kind: BinaryTokenKind::Comma,
            }),
            TOKEN_SEMICOLON => tokens.push(BinaryTokenRecord {
                offset,
                kind: BinaryTokenKind::Semicolon,
            }),
            TOKEN_TEMPLATE => tokens.push(BinaryTokenRecord {
                offset,
                kind: BinaryTokenKind::Template,
            }),
            TOKEN_ARRAY => tokens.push(BinaryTokenRecord {
                offset,
                kind: BinaryTokenKind::Array,
            }),
            TOKEN_WORD => tokens.push(BinaryTokenRecord {
                offset,
                kind: BinaryTokenKind::Identifier("WORD".to_string()),
            }),
            TOKEN_DWORD => tokens.push(BinaryTokenRecord {
                offset,
                kind: BinaryTokenKind::Identifier("DWORD".to_string()),
            }),
            TOKEN_FLOAT => tokens.push(BinaryTokenRecord {
                offset,
                kind: BinaryTokenKind::Identifier("FLOAT".to_string()),
            }),
            TOKEN_DOUBLE => tokens.push(BinaryTokenRecord {
                offset,
                kind: BinaryTokenKind::Identifier("DOUBLE".to_string()),
            }),
            TOKEN_CHAR => tokens.push(BinaryTokenRecord {
                offset,
                kind: BinaryTokenKind::Identifier("CHAR".to_string()),
            }),
            TOKEN_UCHAR => tokens.push(BinaryTokenRecord {
                offset,
                kind: BinaryTokenKind::Identifier("UCHAR".to_string()),
            }),
            TOKEN_SWORD => tokens.push(BinaryTokenRecord {
                offset,
                kind: BinaryTokenKind::Identifier("SWORD".to_string()),
            }),
            TOKEN_SDWORD => tokens.push(BinaryTokenRecord {
                offset,
                kind: BinaryTokenKind::Identifier("SDWORD".to_string()),
            }),
            TOKEN_VOID => tokens.push(BinaryTokenRecord {
                offset,
                kind: BinaryTokenKind::Identifier("void".to_string()),
            }),
            TOKEN_LPSTR => tokens.push(BinaryTokenRecord {
                offset,
                kind: BinaryTokenKind::Identifier("string".to_string()),
            }),
            TOKEN_UNICODE => tokens.push(BinaryTokenRecord {
                offset,
                kind: BinaryTokenKind::Identifier("unicode".to_string()),
            }),
            TOKEN_CSTRING => tokens.push(BinaryTokenRecord {
                offset,
                kind: BinaryTokenKind::Identifier("cstring".to_string()),
            }),
            other => {
                return Err(Error::Parse(format!(
                    "invalid or non-DirectX binary token {} at body offset 0x{:X}",
                    other, offset
                )))
            }
        }
    }
    Ok(tokens)
}


fn is_known_binary_token(token: u16) -> bool {
    matches!(
        token,
        TOKEN_NAME
            | TOKEN_STRING
            | TOKEN_INTEGER
            | TOKEN_GUID
            | TOKEN_INTEGER_LIST
            | TOKEN_FLOAT_LIST
            | TOKEN_OBRACE
            | TOKEN_CBRACE
            | TOKEN_OPAREN
            | TOKEN_CPAREN
            | TOKEN_OBRACKET
            | TOKEN_CBRACKET
            | TOKEN_OANGLE
            | TOKEN_CANGLE
            | TOKEN_DOT
            | TOKEN_COMMA
            | TOKEN_SEMICOLON
            | TOKEN_TEMPLATE
            | TOKEN_WORD
            | TOKEN_DWORD
            | TOKEN_FLOAT
            | TOKEN_DOUBLE
            | TOKEN_CHAR
            | TOKEN_UCHAR
            | TOKEN_SWORD
            | TOKEN_SDWORD
            | TOKEN_VOID
            | TOKEN_LPSTR
            | TOKEN_UNICODE
            | TOKEN_CSTRING
            | TOKEN_ARRAY
    )
}

struct BinaryReader<'a> {
    data: &'a [u8],
    cursor: usize,
    float_size: FloatSize,
}

impl<'a> BinaryReader<'a> {
    fn new(data: &'a [u8], float_size: FloatSize) -> Self {
        Self {
            data,
            cursor: 0,
            float_size,
        }
    }

    fn is_eof(&self) -> bool {
        self.cursor >= self.data.len()
    }

    fn ensure(&self, n: usize) -> Result<()> {
        match self.cursor.checked_add(n) {
            Some(end) if end <= self.data.len() => Ok(()),
            _ => Err(Error::UnexpectedEof),
        }
    }

    fn read_u16(&mut self) -> Result<u16> {
        self.ensure(2)?;
        let v = u16::from_le_bytes([self.data[self.cursor], self.data[self.cursor + 1]]);
        self.cursor += 2;
        Ok(v)
    }

    fn read_u32(&mut self) -> Result<u32> {
        self.ensure(4)?;
        let v = u32::from_le_bytes([
            self.data[self.cursor],
            self.data[self.cursor + 1],
            self.data[self.cursor + 2],
            self.data[self.cursor + 3],
        ]);
        self.cursor += 4;
        Ok(v)
    }

    fn read_f32(&mut self) -> Result<f32> {
        self.ensure(4)?;
        let v = f32::from_le_bytes([
            self.data[self.cursor],
            self.data[self.cursor + 1],
            self.data[self.cursor + 2],
            self.data[self.cursor + 3],
        ]);
        self.cursor += 4;
        Ok(v)
    }

    fn read_f64(&mut self) -> Result<f64> {
        self.ensure(8)?;
        let v = f64::from_le_bytes([
            self.data[self.cursor],
            self.data[self.cursor + 1],
            self.data[self.cursor + 2],
            self.data[self.cursor + 3],
            self.data[self.cursor + 4],
            self.data[self.cursor + 5],
            self.data[self.cursor + 6],
            self.data[self.cursor + 7],
        ]);
        self.cursor += 8;
        Ok(v)
    }

    fn read_real(&mut self) -> Result<f64> {
        match self.float_size {
            FloatSize::F32 => Ok(self.read_f32()? as f64),
            FloatSize::F64 => self.read_f64(),
        }
    }

    fn read_bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        self.ensure(n)?;
        let out = &self.data[self.cursor..self.cursor + n];
        self.cursor += n;
        Ok(out)
    }

    fn read_counted_ascii(&mut self) -> Result<String> {
        let len = self.read_u32()? as usize;
        let bytes = self.read_bytes(len)?;
        let s = std::str::from_utf8(bytes)
            .map_err(|_| Error::Parse("binary string is not valid UTF-8/ASCII".to_string()))?;
        Ok(s.to_string())
    }

    fn read_guid(&mut self) -> Result<Guid> {
        let data1 = self.read_u32()?;
        let data2 = self.read_u16()?;
        let data3 = self.read_u16()?;
        let data4 = self.read_bytes(8)?;
        Guid::parse(&format!(
            "{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
            data1,
            data2,
            data3,
            data4[0],
            data4[1],
            data4[2],
            data4[3],
            data4[4],
            data4[5],
            data4[6],
            data4[7],
        ))
    }

    fn consume_string_terminator(&mut self) -> Result<()> {
        if self.cursor + 2 > self.data.len() {
            return Err(Error::UnexpectedEof);
        }

        let value16 = u16::from_le_bytes([self.data[self.cursor], self.data[self.cursor + 1]]);
        if value16 == TOKEN_COMMA || value16 == TOKEN_SEMICOLON {
            // Assimp and most real-world .x files encode this terminator as a WORD
            // token. The Microsoft token-record table describes a DWORD-sized field;
            // accept that spelling too when the upper WORD is zero and the following
            // stream position still starts at a plausible token boundary.
            if self.cursor + 4 <= self.data.len() {
                let value32 = u32::from_le_bytes([
                    self.data[self.cursor],
                    self.data[self.cursor + 1],
                    self.data[self.cursor + 2],
                    self.data[self.cursor + 3],
                ]);
                if (value32 == TOKEN_COMMA as u32 || value32 == TOKEN_SEMICOLON as u32)
                    && self.cursor + 4 <= self.data.len()
                    && (self.cursor + 4 == self.data.len()
                        || (self.cursor + 6 <= self.data.len()
                            && is_known_binary_token(u16::from_le_bytes([
                                self.data[self.cursor + 4],
                                self.data[self.cursor + 5],
                            ]))))
                {
                    self.cursor += 4;
                    return Ok(());
                }
            }
            self.cursor += 2;
            return Ok(());
        }
        Err(Error::Parse(
            "binary string record is missing trailing comma/semicolon token".to_string(),
        ))
    }
}

struct Parser<'a> {
    tokens: &'a [BinaryTokenRecord],
    cursor: usize,
}

impl<'a> Parser<'a> {
    fn is_eof(&self) -> bool {
        self.cursor >= self.tokens.len()
    }

    fn peek(&self) -> Option<&'a BinaryTokenRecord> {
        self.tokens.get(self.cursor)
    }

    fn peek_n(&self, n: usize) -> Option<&'a BinaryTokenRecord> {
        self.tokens.get(self.cursor + n)
    }

    fn peek_kind(&self) -> Option<&'a BinaryTokenKind> {
        self.peek().map(|t| &t.kind)
    }

    fn bump(&mut self) -> Option<&'a BinaryTokenRecord> {
        let token = self.tokens.get(self.cursor);
        self.cursor += usize::from(token.is_some());
        token
    }

    fn parse_identifier(&mut self) -> Result<String> {
        match self.bump().map(|t| &t.kind) {
            Some(BinaryTokenKind::Identifier(s)) => Ok(s.clone()),
            other => Err(Error::Parse(format!(
                "expected identifier in binary stream, got {:?}",
                other
            ))),
        }
    }

    fn parse_template(&mut self) -> Result<XTemplateDef> {
        self.expect_simple(BinaryTokenKind::Template)?;
        let name = self.parse_identifier()?;
        self.expect_simple(BinaryTokenKind::OpenBrace)?;
        let uuid = match self.bump().map(|t| &t.kind) {
            Some(BinaryTokenKind::Guid(g)) => g.clone(),
            other => {
                return Err(Error::Parse(format!(
                    "expected template guid in binary stream, got {:?}",
                    other
                )))
            }
        };

        let mut members = Vec::new();
        let mut restrictions = Vec::new();
        while !self.check_simple(&BinaryTokenKind::CloseBrace) {
            if self.check_simple(&BinaryTokenKind::OpenBracket) {
                restrictions = self.parse_restrictions()?;
                break;
            }
            members.push(self.parse_template_member()?);
        }
        self.expect_simple(BinaryTokenKind::CloseBrace)?;

        Ok(XTemplateDef {
            name,
            uuid,
            members,
            restrictions,
        })
    }

    fn parse_template_member(&mut self) -> Result<XTemplateMember> {
        if self.check_simple(&BinaryTokenKind::Array) {
            self.bump();
            let ty = self.parse_identifier()?;
            let name = Some(self.parse_identifier()?);
            let mut dimensions = Vec::new();
            while self.check_simple(&BinaryTokenKind::OpenBracket) {
                self.bump();
                dimensions.push(match self.bump().map(|t| &t.kind) {
                    Some(BinaryTokenKind::Identifier(s)) => s.clone(),
                    Some(BinaryTokenKind::Integer(v)) => v.to_string(),
                    other => {
                        return Err(Error::Parse(format!(
                            "expected array dimension in binary stream, got {:?}",
                            other
                        )))
                    }
                });
                self.expect_simple(BinaryTokenKind::CloseBracket)?;
            }
            self.expect_simple(BinaryTokenKind::Semicolon)?;
            Ok(XTemplateMember::Array {
                ty,
                name,
                dimensions,
            })
        } else {
            let ty = self.parse_identifier()?;
            let name = if self.check_simple(&BinaryTokenKind::Semicolon) {
                None
            } else {
                Some(self.parse_identifier()?)
            };
            self.expect_simple(BinaryTokenKind::Semicolon)?;
            Ok(XTemplateMember::Scalar { ty, name })
        }
    }

    fn parse_restrictions(&mut self) -> Result<Vec<XTemplateRestriction>> {
        self.expect_simple(BinaryTokenKind::OpenBracket)?;
        let mut out = Vec::new();
        while !self.check_simple(&BinaryTokenKind::CloseBracket) {
            match self.bump().map(|t| &t.kind) {
                Some(BinaryTokenKind::Identifier(s)) => out.push(XTemplateRestriction::Name(s.clone())),
                Some(BinaryTokenKind::Guid(g)) => out.push(XTemplateRestriction::Guid(g.clone())),
                Some(BinaryTokenKind::Dot) => {
                    self.expect_simple(BinaryTokenKind::Dot)?;
                    self.expect_simple(BinaryTokenKind::Dot)?;
                    out.push(XTemplateRestriction::Ellipsis);
                }
                Some(BinaryTokenKind::Comma) => {}
                other => out.push(XTemplateRestriction::Raw(format!("{:?}", other))),
            }
        }
        self.expect_simple(BinaryTokenKind::CloseBracket)?;
        Ok(out)
    }

    fn parse_data_object(&mut self) -> Result<XDataObject> {
        let class_name = self.parse_identifier()?;
        let object_name = match (
            self.peek_kind(),
            self.peek_n(1).map(|t| &t.kind),
        ) {
            (Some(BinaryTokenKind::Identifier(_)), Some(BinaryTokenKind::OpenBrace)) => {
                Some(self.parse_identifier()?)
            }
            _ => None,
        };
        self.expect_simple(BinaryTokenKind::OpenBrace)?;

        let class_id = if matches!(self.peek_kind(), Some(BinaryTokenKind::Guid(_))) {
            match self.bump().map(|t| &t.kind) {
                Some(BinaryTokenKind::Guid(g)) => Some(g.clone()),
                _ => None,
            }
        } else {
            None
        };

        let mut elements = Vec::new();
        while !self.check_simple(&BinaryTokenKind::CloseBrace) {
            if self.check_simple(&BinaryTokenKind::OpenBrace) {
                elements.push(XObjectElement::Reference(self.parse_reference_block()?));
                continue;
            }
            if self.looks_like_nested_object() {
                elements.push(XObjectElement::NestedObject(self.parse_data_object()?));
                continue;
            }
            let token = self.bump().ok_or(Error::UnexpectedEof)?;
            match &token.kind {
                BinaryTokenKind::Integer(v) => {
                    elements.push(XObjectElement::Primitive(PrimitiveValue::Integer(*v)))
                }
                BinaryTokenKind::Float(v) => {
                    elements.push(XObjectElement::Primitive(PrimitiveValue::Float(*v)))
                }
                BinaryTokenKind::String(s) => {
                    elements.push(XObjectElement::Primitive(PrimitiveValue::String(s.clone())))
                }
                BinaryTokenKind::Identifier(s) => elements.push(XObjectElement::Primitive(
                    PrimitiveValue::Identifier(s.clone()),
                )),
                BinaryTokenKind::Guid(g) => {
                    elements.push(XObjectElement::Primitive(PrimitiveValue::Guid(g.clone())))
                }
                BinaryTokenKind::Comma => elements.push(XObjectElement::Separator(Separator::Comma)),
                BinaryTokenKind::Semicolon => {
                    elements.push(XObjectElement::Separator(Separator::Semicolon))
                }
                other => {
                    return Err(Error::Parse(format!(
                        "unexpected binary token inside object body: {:?}",
                        other
                    )))
                }
            }
        }
        self.expect_simple(BinaryTokenKind::CloseBrace)?;

        Ok(XDataObject {
            class_name,
            object_name,
            class_id,
            elements,
        })
    }

    fn parse_reference_block(&mut self) -> Result<ReferenceTarget> {
        self.expect_simple(BinaryTokenKind::OpenBrace)?;
        let mut name = None;
        let mut uuid = None;
        while !self.check_simple(&BinaryTokenKind::CloseBrace) {
            match self.bump().map(|t| &t.kind) {
                Some(BinaryTokenKind::Identifier(s)) if name.is_none() => name = Some(s.clone()),
                Some(BinaryTokenKind::Guid(g)) if uuid.is_none() => uuid = Some(g.clone()),
                Some(BinaryTokenKind::Comma | BinaryTokenKind::Semicolon) => {}
                other => {
                    return Err(Error::Parse(format!(
                        "unexpected binary token inside reference block: {:?}",
                        other
                    )))
                }
            }
        }
        self.expect_simple(BinaryTokenKind::CloseBrace)?;
        Ok(ReferenceTarget { name, uuid })
    }

    fn looks_like_nested_object(&self) -> bool {
        matches!(
            (
                self.peek_kind(),
                self.peek_n(1).map(|t| &t.kind),
                self.peek_n(2).map(|t| &t.kind)
            ),
            (Some(BinaryTokenKind::Identifier(_)), Some(BinaryTokenKind::OpenBrace), _)
                | (
                    Some(BinaryTokenKind::Identifier(_)),
                    Some(BinaryTokenKind::Identifier(_)),
                    Some(BinaryTokenKind::OpenBrace)
                )
        )
    }

    fn check_simple(&self, want: &BinaryTokenKind) -> bool {
        matches!(self.peek_kind(), Some(kind) if same_variant(kind, want))
    }

    fn expect_simple(&mut self, want: BinaryTokenKind) -> Result<()> {
        match self.bump() {
            Some(token) if same_variant(&token.kind, &want) => Ok(()),
            Some(token) => Err(Error::Parse(format!(
                "expected {:?}, got {:?}",
                want, token.kind
            ))),
            None => Err(Error::UnexpectedEof),
        }
    }
}

fn same_variant(a: &BinaryTokenKind, b: &BinaryTokenKind) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::header::XFileHeader;
    use crate::semantic::Scene;

    fn push_u16(out: &mut Vec<u8>, v: u16) {
        out.extend_from_slice(&v.to_le_bytes());
    }

    fn push_u32(out: &mut Vec<u8>, v: u32) {
        out.extend_from_slice(&v.to_le_bytes());
    }

    fn push_f32(out: &mut Vec<u8>, v: f32) {
        out.extend_from_slice(&v.to_le_bytes());
    }

    fn push_name(out: &mut Vec<u8>, s: &str) {
        push_u16(out, TOKEN_NAME);
        push_u32(out, s.len() as u32);
        out.extend_from_slice(s.as_bytes());
    }

    fn push_integer(out: &mut Vec<u8>, v: u32) {
        push_u16(out, TOKEN_INTEGER);
        push_u32(out, v);
    }

    fn push_float_list(out: &mut Vec<u8>, values: &[f32]) {
        push_u16(out, TOKEN_FLOAT_LIST);
        push_u32(out, values.len() as u32);
        for value in values {
            push_f32(out, *value);
        }
    }

    fn push_integer_list(out: &mut Vec<u8>, values: &[u32]) {
        push_u16(out, TOKEN_INTEGER_LIST);
        push_u32(out, values.len() as u32);
        for value in values {
            push_u32(out, *value);
        }
    }

    fn minimal_binary_sample() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"xof 0303bin 0032");

        push_name(&mut out, "Frame");
        push_name(&mut out, "Root");
        push_u16(&mut out, TOKEN_OBRACE);

        push_name(&mut out, "FrameTransformMatrix");
        push_u16(&mut out, TOKEN_OBRACE);
        push_float_list(
            &mut out,
            &[
                1.0, 0.0, 0.0, 0.0,
                0.0, 1.0, 0.0, 0.0,
                0.0, 0.0, 1.0, 0.0,
                0.0, 0.0, 0.0, 1.0,
            ],
        );
        push_u16(&mut out, TOKEN_CBRACE);

        push_name(&mut out, "Mesh");
        push_name(&mut out, "Mesh0");
        push_u16(&mut out, TOKEN_OBRACE);
        push_integer(&mut out, 3);
        push_float_list(&mut out, &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
        push_integer(&mut out, 1);
        push_integer_list(&mut out, &[3, 0, 1, 2]);
        push_u16(&mut out, TOKEN_CBRACE);

        push_u16(&mut out, TOKEN_CBRACE);
        out
    }



    #[test]
    fn tokenizes_binary_string_with_dword_terminator() {
        let mut body = Vec::new();
        push_u16(&mut body, TOKEN_STRING);
        push_u32(&mut body, 3);
        body.extend_from_slice(b"abc");
        body.extend_from_slice(&(TOKEN_SEMICOLON as u32).to_le_bytes());
        push_name(&mut body, "Frame");
        let tokens = tokenize_binary(&body, FloatSize::F32).unwrap();
        assert!(matches!(tokens[0].kind, BinaryTokenKind::String(ref s) if s == "abc"));
        assert!(matches!(tokens[1].kind, BinaryTokenKind::Identifier(ref s) if s == "Frame"));
    }


    #[test]
    fn tokenizes_binary_sample() {
        let sample = minimal_binary_sample();
        let (header, header_len) = XFileHeader::parse(&sample).unwrap();
        let tokens = tokenize_binary(&sample[header_len..], header.float_size).unwrap();
        assert!(tokens.iter().any(|t| matches!(t.kind, BinaryTokenKind::Identifier(ref s) if s == "Frame")));
        assert!(tokens.iter().any(|t| matches!(t.kind, BinaryTokenKind::Float(v) if (v - 1.0).abs() < 0.0001)));
    }

    #[test]
    fn parses_binary_scene() {
        let sample = minimal_binary_sample();
        let (header, header_len) = XFileHeader::parse(&sample).unwrap();
        let file = parse_binary_file(header, &sample[header_len..]).unwrap();
        let scene = Scene::from_xfile(&file).unwrap();
        assert_eq!(scene.frames.len(), 1);
        assert_eq!(scene.frames[0].meshes.len(), 1);
        assert_eq!(scene.frames[0].meshes[0].vertices.len(), 3);
        assert_eq!(scene.frames[0].meshes[0].faces[0], vec![0, 1, 2]);
    }
}
