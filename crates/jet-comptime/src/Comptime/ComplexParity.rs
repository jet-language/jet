//! Comptime/REPL marshalling for the shared Complex Prelude value.

mod math_lib_pure {
    include!("../../../jet-codegen/src/Prelude/CoreLib/Top/MathLibPure.rs");
}

use crate::AST::{CtFloat, CtValue};

pub(crate) fn part(value: &CtValue) -> Option<f64> {
    match value {
        CtValue::Int(value) => Some(*value as f64),
        CtValue::Float(value) => Some(value.as_f64()),
        _ => None,
    }
}

pub(crate) fn from_parts(real: f64, imaginary: f64) -> CtValue {
    let value = math_lib_pure::JetComplex::from_parts(real, imaginary);
    CtValue::Struct {
        type_name: crate::Syntax::TYPE_COMPLEX.to_string(),
        fields: vec![
            ("real".to_string(), CtValue::Float(CtFloat::f64(value.real))),
            (
                "imaginary".to_string(),
                CtValue::Float(CtFloat::f64(value.imaginary)),
            ),
        ],
    }
}

fn value(value: &CtValue) -> Option<math_lib_pure::JetComplex> {
    let CtValue::Struct { type_name, fields } = value else {
        return None;
    };
    if type_name != crate::Syntax::TYPE_COMPLEX {
        return None;
    }
    let field = |wanted: &str| {
        fields
            .iter()
            .find(|(name, _)| name == wanted)
            .and_then(|(_, value)| part(value))
    };
    Some(math_lib_pure::JetComplex::from_parts(
        field("real")?,
        field("imaginary")?,
    ))
}

pub(crate) fn binary(method: &str, left: &CtValue, right: &CtValue) -> Option<CtValue> {
    let left = value(left)?;
    let right = value(right)?;
    let result = match method {
        "add" => left.add(&right),
        "sub" => left.sub(&right),
        "mul" => left.mul(&right),
        "div" => left.div(&right),
        _ => return None,
    };
    Some(from_parts(result.real, result.imaginary))
}

pub(crate) fn abs(input: &CtValue) -> Option<f64> {
    Some(value(input)?.abs())
}

pub(crate) fn to_string(input: &CtValue) -> Option<String> {
    Some(value(input)?.to_string_rep())
}
