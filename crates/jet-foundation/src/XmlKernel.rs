//! One XML operation kernel for AOT, JIT, comptime, and interpreter adapters.

use crate::XmlPull as xml;

fn without_untrusted_lexical_evidence(mut value: xml::Value) -> xml::Value {
    xml::invalidate_untrusted_lexical_evidence(&mut value);
    value
}

fn checked(value: &xml::Value) -> xml::Value {
    without_untrusted_lexical_evidence(value.clone())
}

pub fn parse_document(text: &str) -> Result<xml::Value, xml::Error> {
    xml::parse_document(text).map(without_untrusted_lexical_evidence)
}

pub fn parse_document_with(
    text: &str,
    options: &xml::ParseOptions,
) -> Result<xml::Value, xml::Error> {
    xml::parse_document_with(text, options).map(without_untrusted_lexical_evidence)
}

pub fn parse_document_bytes_with(
    bytes: &[u8],
    options: &xml::ParseOptions,
) -> Result<xml::Value, xml::Error> {
    xml::parse_document_bytes_with(bytes, options).map(without_untrusted_lexical_evidence)
}

pub fn parse_document_bytes(bytes: &[u8]) -> Result<xml::Value, xml::Error> {
    parse_document_bytes_with(bytes, &xml::ParseOptions::safe())
}

pub fn render_document(value: &xml::Value) -> Result<String, xml::Error> {
    xml::render_document(&checked(value))
}

pub fn render_document_bytes(
    value: &xml::Value,
    encoding: xml::RenderEncoding,
    lexical: xml::LexicalPolicy,
) -> Result<Vec<u8>, xml::Error> {
    xml::render_document_bytes(&checked(value), encoding, lexical)
}

pub fn canonical_document(
    value: &xml::Value,
    options: &xml::CanonicalOptions,
) -> Result<String, xml::Error> {
    xml::canonical_document(&checked(value), options)
}

pub fn document_root(value: &xml::Value) -> Result<xml::Value, xml::Error> {
    xml::document_root(&checked(value))
}

pub fn expanded_name_parts(
    value: &xml::Value,
) -> Result<(String, Option<String>, String, Option<String>), xml::Error> {
    xml::expanded_name_parts(&checked(value))
}

pub fn lookup_attribute(value: &xml::Value, name: &str) -> Result<Option<String>, xml::Error> {
    xml::lookup_attribute(&checked(value), name)
}

pub fn element_content(value: &xml::Value) -> Result<Vec<xml::Value>, xml::Error> {
    xml::element_content(&checked(value))
}

pub fn project_document_for_decode(value: &xml::Value) -> Result<xml::Value, xml::Error> {
    xml::project_document_for_decode(&checked(value))
}
