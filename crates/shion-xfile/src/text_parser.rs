use crate::error::{Error, Result};
use crate::header::XFileHeader;
use crate::model::*;
use crate::text_lexer::{lex_text, Token, TokenKind};

pub fn parse_text_file(header: XFileHeader, body: &[u8]) -> Result<XFile> {
    let tokens = lex_text(body)?;
    let mut parser = Parser { tokens: &tokens, cursor: 0 };
    let mut templates = Vec::new();
    let mut objects = Vec::new();

    while !parser.is_eof() {
        match parser.peek_kind() {
            Some(TokenKind::Template) => templates.push(parser.parse_template()?),
            Some(TokenKind::Identifier(_)) => objects.push(parser.parse_data_object()?),
            Some(TokenKind::Semicolon | TokenKind::Comma) => {
                parser.bump();
            }
            Some(other) => {
                return Err(Error::Parse(format!(
                    "unexpected token at top level: {:?}",
                    other
                )))
            }
            None => break,
        }
    }

    Ok(XFile { header, templates, objects })
}

struct Parser<'a> {
    tokens: &'a [Token],
    cursor: usize,
}

impl<'a> Parser<'a> {
    fn is_eof(&self) -> bool {
        self.cursor >= self.tokens.len()
    }

    fn peek(&self) -> Option<&'a Token> {
        self.tokens.get(self.cursor)
    }

    fn peek_n(&self, n: usize) -> Option<&'a Token> {
        self.tokens.get(self.cursor + n)
    }

    fn peek_kind(&self) -> Option<&'a TokenKind> {
        self.peek().map(|t| &t.kind)
    }

    fn bump(&mut self) -> Option<&'a Token> {
        let token = self.tokens.get(self.cursor);
        self.cursor += usize::from(token.is_some());
        token
    }

    fn parse_identifier(&mut self) -> Result<String> {
        match self.bump().map(|t| &t.kind) {
            Some(TokenKind::Identifier(s)) => Ok(s.clone()),
            other => Err(Error::Parse(format!("expected identifier, got {:?}", other))),
        }
    }

    fn parse_template(&mut self) -> Result<XTemplateDef> {
        self.expect_simple(TokenKind::Template)?;
        let name = self.parse_identifier()?;
        self.expect_simple(TokenKind::OpenBrace)?;
        let uuid = match self.bump().map(|t| &t.kind) {
            Some(TokenKind::Guid(g)) => g.clone(),
            other => return Err(Error::Parse(format!("expected template guid, got {:?}", other))),
        };

        let mut members = Vec::new();
        let mut restrictions = Vec::new();
        while !self.check_simple(&TokenKind::CloseBrace) {
            if self.check_simple(&TokenKind::OpenBracket) {
                restrictions = self.parse_restrictions()?;
                break;
            }
            members.push(self.parse_template_member()?);
        }
        self.expect_simple(TokenKind::CloseBrace)?;

        Ok(XTemplateDef { name, uuid, members, restrictions })
    }

    fn parse_template_member(&mut self) -> Result<XTemplateMember> {
        if self.check_simple(&TokenKind::Array) {
            self.bump();
            let ty = self.parse_identifier()?;
            let name = Some(self.parse_identifier()?);
            let mut dims = Vec::new();
            while self.check_simple(&TokenKind::OpenBracket) {
                self.bump();
                dims.push(match self.bump().map(|t| &t.kind) {
                    Some(TokenKind::Identifier(s)) => s.clone(),
                    Some(TokenKind::Integer(v)) => v.to_string(),
                    other => {
                        return Err(Error::Parse(format!(
                            "expected array dimension, got {:?}",
                            other
                        )))
                    }
                });
                self.expect_simple(TokenKind::CloseBracket)?;
            }
            self.expect_simple(TokenKind::Semicolon)?;
            Ok(XTemplateMember::Array { ty, name, dimensions: dims })
        } else {
            let ty = self.parse_identifier()?;
            let name = if self.check_simple(&TokenKind::Semicolon) { None } else { Some(self.parse_identifier()?) };
            self.expect_simple(TokenKind::Semicolon)?;
            Ok(XTemplateMember::Scalar { ty, name })
        }
    }

    fn parse_restrictions(&mut self) -> Result<Vec<XTemplateRestriction>> {
        self.expect_simple(TokenKind::OpenBracket)?;
        let mut out = Vec::new();
        while !self.check_simple(&TokenKind::CloseBracket) {
            match self.bump().map(|t| &t.kind) {
                Some(TokenKind::Identifier(s)) => out.push(XTemplateRestriction::Name(s.clone())),
                Some(TokenKind::Guid(g)) => out.push(XTemplateRestriction::Guid(g.clone())),
                Some(TokenKind::Dot) => {
                    self.expect_simple(TokenKind::Dot)?;
                    self.expect_simple(TokenKind::Dot)?;
                    out.push(XTemplateRestriction::Ellipsis);
                }
                Some(TokenKind::Comma) => {}
                other => out.push(XTemplateRestriction::Raw(format!("{:?}", other))),
            }
        }
        self.expect_simple(TokenKind::CloseBracket)?;
        Ok(out)
    }

    fn parse_data_object(&mut self) -> Result<XDataObject> {
        let class_name = self.parse_identifier()?;
        let object_name = match (self.peek_kind(), self.peek_n(1).map(|t| &t.kind)) {
            (Some(TokenKind::Identifier(_)), Some(TokenKind::OpenBrace)) => Some(self.parse_identifier()?),
            _ => None,
        };
        self.expect_simple(TokenKind::OpenBrace)?;

        let class_id = if matches!(self.peek_kind(), Some(TokenKind::Guid(_))) {
            match self.bump().map(|t| &t.kind) {
                Some(TokenKind::Guid(g)) => Some(g.clone()),
                _ => None,
            }
        } else {
            None
        };

        let mut elements = Vec::new();
        while !self.check_simple(&TokenKind::CloseBrace) {
            if self.check_simple(&TokenKind::OpenBrace) {
                elements.push(XObjectElement::Reference(self.parse_reference_block()?));
                continue;
            }
            if self.looks_like_nested_object() {
                elements.push(XObjectElement::NestedObject(self.parse_data_object()?));
                continue;
            }
            let token = self.bump().ok_or(Error::UnexpectedEof)?;
            match &token.kind {
                TokenKind::Integer(v) => elements.push(XObjectElement::Primitive(PrimitiveValue::Integer(*v))),
                TokenKind::Float(v) => elements.push(XObjectElement::Primitive(PrimitiveValue::Float(*v))),
                TokenKind::String(s) => elements.push(XObjectElement::Primitive(PrimitiveValue::String(s.clone()))),
                TokenKind::Identifier(s) => elements.push(XObjectElement::Primitive(PrimitiveValue::Identifier(s.clone()))),
                TokenKind::Guid(g) => elements.push(XObjectElement::Primitive(PrimitiveValue::Guid(g.clone()))),
                TokenKind::Comma => elements.push(XObjectElement::Separator(Separator::Comma)),
                TokenKind::Semicolon => elements.push(XObjectElement::Separator(Separator::Semicolon)),
                other => {
                    return Err(Error::Parse(format!(
                        "unexpected token inside object body: {:?}",
                        other
                    )))
                }
            }
        }
        self.expect_simple(TokenKind::CloseBrace)?;

        Ok(XDataObject { class_name, object_name, class_id, elements })
    }

    fn parse_reference_block(&mut self) -> Result<ReferenceTarget> {
        self.expect_simple(TokenKind::OpenBrace)?;
        let mut name = None;
        let mut uuid = None;
        while !self.check_simple(&TokenKind::CloseBrace) {
            match self.bump().map(|t| &t.kind) {
                Some(TokenKind::Identifier(s)) if name.is_none() => name = Some(s.clone()),
                Some(TokenKind::Guid(g)) if uuid.is_none() => uuid = Some(g.clone()),
                Some(TokenKind::Comma | TokenKind::Semicolon) => {}
                other => {
                    return Err(Error::Parse(format!(
                        "unexpected token inside reference block: {:?}",
                        other
                    )))
                }
            }
        }
        self.expect_simple(TokenKind::CloseBrace)?;
        Ok(ReferenceTarget { name, uuid })
    }

    fn looks_like_nested_object(&self) -> bool {
        matches!(
            (self.peek_kind(), self.peek_n(1).map(|t| &t.kind), self.peek_n(2).map(|t| &t.kind)),
            (Some(TokenKind::Identifier(_)), Some(TokenKind::OpenBrace), _)
                | (Some(TokenKind::Identifier(_)), Some(TokenKind::Identifier(_)), Some(TokenKind::OpenBrace))
        )
    }

    fn check_simple(&self, want: &TokenKind) -> bool {
        matches!(self.peek_kind(), Some(kind) if std::mem::discriminant(kind) == std::mem::discriminant(want))
    }

    fn expect_simple(&mut self, want: TokenKind) -> Result<()> {
        match self.bump() {
            Some(token) if std::mem::discriminant(&token.kind) == std::mem::discriminant(&want) => Ok(()),
            Some(token) => Err(Error::Parse(format!("expected {:?}, got {:?}", want, token.kind))),
            None => Err(Error::UnexpectedEof),
        }
    }
}
