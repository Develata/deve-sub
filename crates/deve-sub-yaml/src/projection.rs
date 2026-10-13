//! Expand only after whole-document resource and alias checks have passed.

use crate::{
    Error, ErrorKind, Mapping, Value,
    parser::{Kind, Node},
};

pub(crate) fn ordered(node: &Node) -> Result<Value, Error> {
    match &node.kind {
        Kind::Scalar { value, .. } => value.clone(),
        Kind::Sequence(children) => children
            .iter()
            .map(|n| ordered(n))
            .collect::<Result<_, _>>()
            .map(Value::Sequence),
        Kind::Mapping(pairs) => {
            let mut map = Mapping::new();
            for (key, value) in pairs {
                let key = ordered(key)?;
                if map.contains_key(&key) {
                    return Err(Error::new(ErrorKind::DuplicateKey).at(node.mark));
                }
                map.insert(key, ordered(value)?);
            }
            if let Some(merge) = map.shift_remove(&Value::String("<<".into())) {
                let inherited = match merge {
                    Value::Sequence(items) => items,
                    other => vec![other],
                };
                for entry in inherited {
                    let Value::Mapping(entries) = entry else {
                        return Err(Error::new(ErrorKind::Merge).at(node.mark));
                    };
                    // Move expanded inheritance, retaining explicit order and the
                    // first inherited value; even discarded values were charged.
                    for (key, value) in entries {
                        map.entry(key).or_insert(value);
                    }
                }
            }
            Ok(Value::Mapping(map))
        }
    }
}

pub(crate) fn json(node: &Node) -> Result<serde_json::Value, Error> {
    match &node.kind {
        Kind::Scalar { value, .. } => Ok(match value.as_ref().map_err(Clone::clone)? {
            Value::Null => serde_json::Value::Null,
            Value::Bool(b) => (*b).into(),
            Value::Number(n) => {
                if let Some(n) = n.as_u64() {
                    n.into()
                } else if let Some(n) = n.as_i64() {
                    n.into()
                } else {
                    n.as_f64()
                        .and_then(serde_json::Number::from_f64)
                        .map(serde_json::Value::Number)
                        .unwrap_or_default()
                }
            }
            Value::String(s) => s.clone().into(),
            Value::Sequence(_) | Value::Mapping(_) => return Err(Error::new(ErrorKind::Type)),
        }),
        Kind::Sequence(children) => children
            .iter()
            .map(|n| json(n))
            .collect::<Result<_, _>>()
            .map(serde_json::Value::Array),
        Kind::Mapping(pairs) => {
            let mut map = serde_json::Map::new();
            for (key, value) in pairs {
                let Kind::Scalar { lexical, .. } = &key.kind else {
                    return Err(Error::new(ErrorKind::Type).at(key.mark));
                };
                map.insert(lexical.clone(), json(value)?);
            }
            Ok(serde_json::Value::Object(map))
        }
    }
}
