use crate::guid::Guid;

#[derive(Debug, Clone)]
pub struct XFile {
    pub header: crate::header::XFileHeader,
    pub templates: Vec<XTemplateDef>,
    pub objects: Vec<XDataObject>,
}

#[derive(Debug, Clone)]
pub enum TopLevelItem {
    Template(XTemplateDef),
    Object(XDataObject),
}

#[derive(Debug, Clone)]
pub struct XTemplateDef {
    pub name: String,
    pub uuid: Guid,
    pub members: Vec<XTemplateMember>,
    pub restrictions: Vec<XTemplateRestriction>,
}

#[derive(Debug, Clone)]
pub enum XTemplateMember {
    Scalar {
        ty: String,
        name: Option<String>,
    },
    Array {
        ty: String,
        name: Option<String>,
        dimensions: Vec<String>,
    },
}

#[derive(Debug, Clone)]
pub enum XTemplateRestriction {
    Name(String),
    Guid(Guid),
    Ellipsis,
    Raw(String),
}

#[derive(Debug, Clone)]
pub struct XDataObject {
    pub class_name: String,
    pub object_name: Option<String>,
    pub class_id: Option<Guid>,
    pub elements: Vec<XObjectElement>,
}

#[derive(Debug, Clone)]
pub enum XObjectElement {
    Primitive(PrimitiveValue),
    Separator(Separator),
    NestedObject(XDataObject),
    Reference(ReferenceTarget),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Separator {
    Comma,
    Semicolon,
}

#[derive(Debug, Clone)]
pub enum PrimitiveValue {
    Integer(i64),
    Float(f64),
    String(String),
    Identifier(String),
    Guid(Guid),
}

#[derive(Debug, Clone)]
pub struct ReferenceTarget {
    pub name: Option<String>,
    pub uuid: Option<Guid>,
}

impl XDataObject {
    pub fn nested_objects_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a XDataObject> + 'a {
        self.elements.iter().filter_map(move |element| match element {
            XObjectElement::NestedObject(obj) if obj.class_name.eq_ignore_ascii_case(name) => Some(obj),
            _ => None,
        })
    }
}
