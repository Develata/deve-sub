#![allow(clippy::expect_used)]

use std::{
    collections::{BTreeMap, HashMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
};

use deve_sub_yaml::{Error, ErrorKind, Mapping, Number, Value, from_json, from_value, to_string};
use serde::{Deserialize, Deserializer};

fn text(value: &str) -> Value {
    Value::String(value.to_owned())
}

fn integer(value: i64) -> Value {
    Value::Number(value.into())
}

fn mapping(entries: impl IntoIterator<Item = (Value, Value)>) -> Value {
    Value::Mapping(entries.into_iter().collect())
}

fn hash(value: &impl Hash) -> u64 {
    let mut state = DefaultHasher::new();
    value.hash(&mut state);
    state.finish()
}

#[test]
fn numeric_identity_and_hash_are_canonical() {
    for value in [0, 1, i64::MAX] {
        let signed = Number::from(value);
        let unsigned = Number::from(value as u64);
        assert_eq!(signed, unsigned);
        assert_eq!(hash(&signed), hash(&unsigned));
    }

    let nan = Number::from(f64::NAN);
    for bits in [0x7ff0_0000_0000_0001, 0xfff8_0000_0000_0005] {
        let other = Number::from(f64::from_bits(bits));
        assert_eq!(nan, other);
        assert_eq!(hash(&nan), hash(&other));
    }
    assert_eq!(Number::from(0.0), Number::from(-0.0));
    assert_eq!(hash(&Number::from(0.0)), hash(&Number::from(-0.0)));
    assert_ne!(Number::from(1_i64), Number::from(1.0));
    assert_ne!(Number::from(0_u64), Number::from(0.0));

    let distinct: std::collections::HashSet<_> = [0.0, 1.0, -1.0, 1.5, f64::INFINITY]
        .map(|value| hash(&Number::from(value)))
        .into_iter()
        .collect();
    assert_eq!(
        distinct.len(),
        5,
        "floating-point hashes must carry value data"
    );
}

#[test]
fn numeric_accessors_preserve_integer_bounds_and_scalar_types() {
    assert_eq!(Number::from(i64::MIN).as_i64(), Some(i64::MIN));
    assert_eq!(Number::from(-1_i64).as_u64(), None);
    assert_eq!(Number::from(u64::MAX).as_i64(), None);
    assert_eq!(Number::from(u64::MAX).as_u64(), Some(u64::MAX));
    assert_eq!(Number::from(1.0).as_i64(), None);
    assert_eq!(Number::from(1.0).as_u64(), None);
    assert_eq!(Number::from(12_i64).as_f64(), Some(12.0));
    assert_eq!(integer(12).as_u64(), Some(12));
    assert_eq!(integer(-12).as_i64(), Some(-12));
    assert_eq!(integer(12).as_f64(), Some(12.0));
    assert_eq!(text("12").as_u64(), None);
    assert_eq!(Value::Bool(true).as_bool(), Some(true));
    assert_eq!(integer(1).as_bool(), None);
}

#[test]
fn mapping_equality_and_hash_ignore_order_but_iteration_keeps_it() {
    let left = mapping([(text("z"), integer(1)), (text("a"), integer(2))]);
    let right = mapping([(text("a"), integer(2)), (text("z"), integer(1))]);
    assert_eq!(left, right);
    assert_eq!(hash(&left), hash(&right));
    let keys = |value: &Value| {
        value
            .as_mapping()
            .expect("mapping")
            .keys()
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(keys(&left), [text("z"), text("a")]);
    assert_eq!(keys(&right), [text("a"), text("z")]);
    assert_ne!(
        left,
        mapping([(text("z"), integer(2)), (text("a"), integer(1))])
    );
    assert_ne!(
        Value::Sequence(vec![integer(1), integer(2)]),
        Value::Sequence(vec![integer(2), integer(1)])
    );
}

#[test]
fn complex_keys_lookup_by_content_and_survive_generic_deserialization() {
    let first = mapping([(text("b"), integer(2)), (text("a"), integer(1))]);
    let reordered = mapping([(text("a"), integer(1)), (text("b"), integer(2))]);
    let sequence_key = Value::Sequence(vec![first.clone(), Value::Null]);
    let mut values = Mapping::new();
    values.insert(first, text("old"));
    values.insert(sequence_key.clone(), text("sequence"));
    assert_eq!(
        values.insert(reordered.clone(), text("updated")),
        Some(text("old"))
    );
    assert_eq!(values.len(), 2);
    assert_eq!(values.get(&reordered), Some(&text("updated")));
    assert_eq!(values.get(&sequence_key), Some(&text("sequence")));

    let original = Value::Mapping(values);
    let decoded: Value = from_value(original.clone()).expect("complex key value");
    assert_eq!(decoded, original);
    assert_eq!(hash(&decoded), hash(&original));
    assert!(to_string(&original).is_ok(), "YAML supports complex keys");
}

#[test]
fn floating_mapping_keys_follow_the_same_identity_rules() {
    let mut values = Mapping::new();
    values.insert(Value::Number(f64::NAN.into()), text("nan"));
    values.insert(Value::Number((-0.0).into()), text("zero"));
    values.insert(integer(1), text("integer"));
    values.insert(Value::Number(1.0.into()), text("float"));
    assert_eq!(values.len(), 4);
    assert_eq!(
        values.get(&Value::Number(f64::from_bits(0xfff8_0000_0000_0001).into())),
        Some(&text("nan"))
    );
    assert_eq!(values.get(&Value::Number(0.0.into())), Some(&text("zero")));
    assert_eq!(values.get(&integer(1)), Some(&text("integer")));
    assert_eq!(values.get(&Value::Number(1.0.into())), Some(&text("float")));
}

#[test]
fn string_and_sequence_lookups_return_null_for_missing_values() {
    let mut value = mapping([(text("items"), Value::Sequence(vec![text("first")]))]);
    assert_eq!(value["items"][0], "first");
    assert!(value["items"].is_sequence());
    assert_eq!(value.get("absent"), None);
    assert!(value["absent"][0]["nested"].is_null());
    assert!(value["items"][1].is_null());
    assert!(value[0].is_null());
    value
        .as_mapping_mut()
        .expect("mapping")
        .insert(text("flag"), Value::Bool(true));
    assert_eq!(value["flag"].as_bool(), Some(true));
}

#[derive(Debug, Deserialize, PartialEq)]
struct Defaults {
    #[serde(default)]
    names: Vec<String>,
    #[serde(default)]
    enabled: bool,
}

#[derive(Debug, Deserialize, PartialEq)]
struct Required {
    name: String,
}

#[test]
fn null_remains_empty_for_typed_collections_and_defaultable_structs() {
    assert!(
        from_value::<Vec<String>>(Value::Null)
            .expect("empty sequence")
            .is_empty()
    );
    assert!(
        from_value::<BTreeMap<String, u64>>(Value::Null)
            .expect("empty mapping")
            .is_empty()
    );
    assert!(
        from_value::<HashMap<String, u64>>(Value::Null)
            .expect("empty mapping")
            .is_empty()
    );
    assert_eq!(
        from_value::<Defaults>(Value::Null).expect("defaulted fields"),
        Defaults {
            names: vec![],
            enabled: false
        }
    );
    assert!(from_value::<Required>(Value::Null).is_err());
    assert_eq!(
        from_value::<[u8; 0]>(Value::Null).expect("empty tuple"),
        [0_u8; 0]
    );
    assert!(from_value::<(u8,)>(Value::Null).is_err());
    assert!(from_value::<(u8,)>(Value::Sequence(vec![integer(1), integer(2)])).is_err());
    assert!(from_value::<Defaults>(Value::Sequence(vec![])).is_err());
    assert!(from_value::<BTreeMap<String, u64>>(Value::Sequence(vec![])).is_err());
    assert!(from_value::<Vec<u8>>(mapping([])).is_err());
    for scalar in [text("example"), Value::Bool(false), integer(0)] {
        assert!(from_value::<Vec<u8>>(scalar.clone()).is_err());
        assert!(from_value::<BTreeMap<String, u64>>(scalar.clone()).is_err());
        assert!(from_value::<Defaults>(scalar).is_err());
    }
}

#[derive(Debug, Deserialize, PartialEq)]
struct Wrapped(Vec<String>);

#[derive(Debug, Deserialize, PartialEq)]
struct Unit;

#[test]
fn options_newtypes_and_units_keep_serde_semantics() {
    assert_eq!(from_value::<Option<u8>>(Value::Null).expect("none"), None);
    assert_eq!(from_value::<Option<u8>>(integer(7)).expect("some"), Some(7));
    assert_eq!(
        from_value::<Wrapped>(Value::Null).expect("newtype"),
        Wrapped(vec![])
    );
    assert_eq!(
        from_value::<Wrapped>(Value::Sequence(vec![text("one")])).expect("newtype payload"),
        Wrapped(vec!["one".to_owned()])
    );
    assert_eq!(from_value::<Unit>(Value::Null).expect("unit struct"), Unit);
    assert!(from_value::<Unit>(integer(1)).is_err());
    assert!(from_value::<Option<String>>(integer(1)).is_err());
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Mode {
    Select,
    Count(u8),
    Pair(u8, bool),
    Named { name: String },
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Member {
    Group { name: String },
}

#[test]
fn string_enums_and_internal_tags_decode_but_external_tag_maps_are_rejected() {
    assert_eq!(
        from_value::<Mode>(text("select")).expect("unit enum"),
        Mode::Select
    );
    let cases = [
        ("select", Value::Null),
        ("count", integer(7)),
        ("pair", Value::Sequence(vec![integer(7), Value::Bool(true)])),
        ("named", mapping([(text("name"), text("example"))])),
    ];
    for (variant, payload) in cases {
        // WHY: serde_yaml::from_value rejected mapping-encoded external
        // variants, including {select: null}. Keep the migration equally strict.
        assert_eq!(
            from_value::<Mode>(mapping([(text(variant), payload)]))
                .expect_err("external-tag mapping")
                .kind(),
            ErrorKind::Type
        );
    }
    assert!(from_value::<Mode>(text("count")).is_err());
    assert!(from_value::<Mode>(mapping([])).is_err());
    assert!(
        from_value::<Mode>(mapping([
            (text("select"), Value::Null),
            (text("count"), integer(1))
        ]))
        .is_err()
    );
    assert!(from_value::<Mode>(mapping([(text("count"), integer(256))])).is_err());

    let member = mapping([(text("kind"), text("group")), (text("name"), text("PROXY"))]);
    assert_eq!(
        from_value::<Member>(member).expect("internally tagged enum"),
        Member::Group {
            name: "PROXY".to_owned()
        }
    );
}

#[test]
fn numeric_visitors_check_ranges_and_never_coerce_strings() {
    assert_eq!(from_value::<u8>(integer(255)).expect("upper bound"), 255);
    assert_eq!(from_value::<i8>(integer(-128)).expect("lower bound"), -128);
    assert_eq!(
        from_value::<i128>(integer(i64::MIN)).expect("widen signed"),
        i128::from(i64::MIN)
    );
    assert_eq!(
        from_value::<u128>(Value::Number(u64::MAX.into())).expect("widen unsigned"),
        u128::from(u64::MAX)
    );
    assert!(from_value::<u8>(integer(256)).is_err());
    assert!(from_value::<u8>(integer(-1)).is_err());
    assert!(from_value::<i8>(integer(-129)).is_err());
    assert!(from_value::<i8>(integer(128)).is_err());
    assert!(from_value::<i64>(Value::Number(u64::MAX.into())).is_err());
    assert!(from_value::<u128>(integer(-1)).is_err());
    assert!(from_value::<i64>(Value::Number(1.0.into())).is_err());
    assert_eq!(
        from_value::<f64>(integer(3)).expect("numeric widening"),
        3.0
    );
    assert!(from_value::<u64>(text("3")).is_err());
    assert!(from_value::<f64>(text("3.0")).is_err());
    assert!(from_value::<String>(integer(3)).is_err());
    assert!(from_value::<bool>(text("true")).is_err());
    for non_scalar in [Value::Null, Value::Sequence(vec![]), mapping([])] {
        assert!(from_value::<bool>(non_scalar.clone()).is_err());
        assert!(from_value::<u64>(non_scalar.clone()).is_err());
        assert!(from_value::<f64>(non_scalar.clone()).is_err());
        assert!(from_value::<String>(non_scalar).is_err());
    }
    assert!(from_value::<bool>(integer(1)).is_err());
    assert!(from_value::<bool>(Value::Number(1.0.into())).is_err());
    assert!(from_value::<u64>(Value::Bool(true)).is_err());
    assert!(from_value::<f64>(Value::Bool(true)).is_err());
    assert!(from_value::<String>(Value::Bool(true)).is_err());
    assert!(from_value::<String>(Value::Number(1.0.into())).is_err());
}

#[test]
fn unknown_fields_are_ignored_without_restricting_their_value_shape() {
    let complex = mapping([(Value::Sequence(vec![Value::Null]), integer(7))]);
    let value = mapping([
        (text("enabled"), Value::Bool(true)),
        (text("unknown"), complex),
    ]);
    assert_eq!(
        from_value::<Defaults>(value).expect("unknown field"),
        Defaults {
            names: vec![],
            enabled: true
        }
    );
}

#[derive(Debug)]
struct Reject;

impl<'de> Deserialize<'de> for Reject {
    fn deserialize<D: Deserializer<'de>>(_: D) -> Result<Self, D::Error> {
        Err(serde::de::Error::custom("example-sensitive-custom-error"))
    }
}

#[test]
fn errors_contain_only_static_type_diagnostics() {
    let errors: [Error; 5] = [
        from_value::<u64>(text("example-sensitive-value")).expect_err("invalid type"),
        from_value::<Mode>(text("example-sensitive-variant")).expect_err("invalid enum"),
        from_value::<Reject>(Value::Null).expect_err("custom error"),
        from_value::<u8>(integer(256)).expect_err("invalid range"),
        from_value::<Required>(Value::Null).expect_err("missing field"),
    ];
    for error in errors {
        assert_eq!(error.kind(), ErrorKind::Type);
        assert_eq!(error.location(), None);
        assert_eq!(error.to_string(), "YAML value has an invalid type or range");
        assert!(!format!("{error:?}").contains("example-sensitive"));
    }
}

#[test]
fn generic_json_deserialization_and_conversion_preserve_all_json_types() {
    let source = serde_json::json!({
        "null": null, "bool": true, "signed": i64::MIN, "unsigned": u64::MAX,
        "float": 1.25, "string": "example", "sequence": [null, false, 7], "map": {}
    });
    let converted = from_json(source.clone());
    let decoded: Value = serde_json::from_value(source.clone()).expect("generic Deserialize");
    assert_eq!(converted, decoded);
    assert_eq!(
        serde_json::to_value(&converted).expect("generic Serialize"),
        source
    );
    assert_eq!(converted["unsigned"].as_u64(), Some(u64::MAX));
    assert_eq!(converted["float"].as_f64(), Some(1.25));
}

#[test]
fn yaml_serialization_keeps_mapping_iteration_order() {
    let value = mapping([
        (text("last-alphabetically"), integer(1)),
        (
            text("first-alphabetically"),
            Value::Sequence(vec![Value::Null, Value::Bool(true), text("example")]),
        ),
        (text("empty-mapping"), mapping([])),
        (text("negative"), integer(-5)),
        (text("float"), Value::Number(1.25.into())),
    ]);
    let output = to_string(&value).expect("serialize ordered mapping");
    assert!(
        output.find("last-alphabetically:").expect("first key")
            < output.find("first-alphabetically:").expect("second key")
    );
    assert!(output.contains("1.25"));
    assert!(output.contains("-5"));
    assert!(output.contains("example"));
}
