//! The existing scalar contract, independent of the parser's evolving schema.

use granit_parser::{ScalarStyle, Tag};

use crate::{Error, ErrorKind, Value};

pub(crate) fn check_flow_indicator(
    text: &str,
    style: ScalarStyle,
    flow: bool,
) -> Result<(), Error> {
    // libyaml interpreted compact ?foo inside flow collections as an explicit
    // entry; granit can interpret a literal scalar instead. Reject rather than
    // silently change existing DNS keys or source values. Quoted text and the
    // unambiguous `? foo` spelling retain their ordinary parser paths.
    if flow && style == ScalarStyle::Plain && text.starts_with('?') {
        Err(Error::new(ErrorKind::AmbiguousFlowIndicator))
    } else {
        Ok(())
    }
}

pub(crate) fn resolve(text: &str, style: ScalarStyle, tag: Option<&Tag>) -> Result<Value, Error> {
    let suffix = tag
        .map(|tag| {
            tag.suffix_in_namespace("tag:yaml.org,2002:")
                .ok_or_else(tag_error)
        })
        .transpose()?;
    if let Some(suffix) = suffix {
        return match suffix.as_ref() {
            "bool" => boolean(text).map(Value::Bool).ok_or_else(type_error),
            "null" => {
                if null(text) {
                    Ok(Value::Null)
                } else {
                    Err(type_error())
                }
            }
            "int" => integer(text)?.ok_or_else(type_error),
            "float" => float(text)
                .map(|n| Value::Number(n.into()))
                .ok_or_else(type_error),
            // The previous backend treated other standard-namespace scalar
            // tags (including !!merge, !!binary and !!timestamp) as text.
            _ => Ok(Value::String(text.to_owned())),
        };
    }
    if style != ScalarStyle::Plain {
        return Ok(Value::String(text.to_owned()));
    }
    if text.is_empty() || null(text) {
        return Ok(Value::Null);
    }
    if let Some(value) = boolean(text) {
        return Ok(Value::Bool(value));
    }
    if let Some(value) = integer(text)? {
        return Ok(value);
    }
    if !leading_zero_decimal(text)
        && let Some(value) = float(text)
    {
        return Ok(Value::Number(value.into()));
    }
    Ok(Value::String(text.to_owned()))
}

pub(crate) fn container_tag(tag: Option<&Tag>, _expected: &str) -> Result<(), Error> {
    match tag {
        None => Ok(()),
        // Legacy Value visitors use the container's shape for standard tags.
        Some(tag) if tag.suffix_in_namespace("tag:yaml.org,2002:").is_some() => Ok(()),
        Some(_) => Err(tag_error()),
    }
}

fn null(text: &str) -> bool {
    matches!(text, "~" | "null" | "Null" | "NULL")
}

fn boolean(text: &str) -> Option<bool> {
    match text {
        "true" | "True" | "TRUE" => Some(true),
        "false" | "False" | "FALSE" => Some(false),
        _ => None,
    }
}

fn leading_zero_decimal(text: &str) -> bool {
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    digits.len() > 1 && digits.starts_with('0') && digits.bytes().all(|b| b.is_ascii_digit())
}

fn integer(text: &str) -> Result<Option<Value>, Error> {
    let (negative, digits) = if let Some(digits) = text.strip_prefix('-') {
        (true, digits)
    } else {
        (false, text.strip_prefix('+').unwrap_or(text))
    };
    if digits.starts_with(['+', '-']) || leading_zero_decimal(text) {
        return Ok(None);
    }
    let (radix, digits) = if let Some(digits) = digits.strip_prefix("0x") {
        (16, digits)
    } else if let Some(digits) = digits.strip_prefix("0o") {
        (8, digits)
    } else if let Some(digits) = digits.strip_prefix("0b") {
        (2, digits)
    } else {
        (10, digits)
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
        return Ok(None);
    }
    let Ok(magnitude) = u128::from_str_radix(digits, radix) else {
        // Legacy plain scalars beyond 128 bits may still resolve as finite
        // floats. An explicit !!int rejects this None at the caller boundary.
        return Ok(None);
    };
    if negative {
        if magnitude > (1_u128 << 127) {
            return Ok(None);
        }
        if magnitude > (1_u128 << 63) {
            return Err(type_error());
        }
        let signed = if magnitude == (1_u128 << 63) {
            i64::MIN
        } else {
            -(magnitude as i64)
        };
        Ok(Some(Value::Number(signed.into())))
    } else {
        let n = u64::try_from(magnitude).map_err(|_| type_error())?;
        Ok(Some(Value::Number(n.into())))
    }
}

fn float(text: &str) -> Option<f64> {
    match text {
        ".inf" | ".Inf" | ".INF" | "+.inf" | "+.Inf" | "+.INF" => Some(f64::INFINITY),
        "-.inf" | "-.Inf" | "-.INF" => Some(f64::NEG_INFINITY),
        ".nan" | ".NaN" | ".NAN" => Some(f64::NAN),
        _ => {
            let unsigned = text.strip_prefix('+').unwrap_or(text);
            if text.starts_with('+') && unsigned.starts_with(['+', '-']) {
                return None;
            }
            unsigned.parse::<f64>().ok().filter(|f| f.is_finite())
        }
    }
}

fn tag_error() -> Error {
    Error::new(ErrorKind::Tag)
}
fn type_error() -> Error {
    Error::new(ErrorKind::Type)
}
