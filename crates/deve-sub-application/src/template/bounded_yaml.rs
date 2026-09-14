//! Budget decoded values while deserializing aliases, before allocating the full tree.

use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_yaml::Value;

/// Includes decoded strings and a per-value allowance for container overhead.
const EXPANDED_BUDGET: usize = 8 * deve_sub_domain::MAX_SPEC_BYTES;

pub(super) fn parse(input: &str) -> Result<Value, serde_yaml::Error> {
    let mut budget = EXPANDED_BUDGET;
    let mut documents = serde_yaml::Deserializer::from_str(input);
    let doc = documents
        .next()
        .ok_or_else(|| <serde_yaml::Error as de::Error>::custom("empty YAML document"))?;
    let value = BoundedValue {
        budget: &mut budget,
        depth: 0,
    }
    .deserialize(doc)?;
    if documents.next().is_some() {
        return Err(de::Error::custom("only one YAML document is allowed"));
    }
    Ok(value)
}

struct BoundedValue<'a> {
    budget: &'a mut usize,
    depth: u32,
}

impl BoundedValue<'_> {
    fn charge<E: de::Error>(&mut self, bytes: usize) -> Result<(), E> {
        *self.budget = self
            .budget
            .checked_sub(bytes)
            .ok_or_else(|| E::custom("expanded YAML exceeds the 8 MiB value budget"))?;
        Ok(())
    }
}

impl<'de> DeserializeSeed<'de> for BoundedValue<'_> {
    type Value = Value;
    fn deserialize<D: de::Deserializer<'de>>(mut self, deserializer: D) -> Result<Value, D::Error> {
        if self.depth > deve_sub_domain::MAX_ALIAS_DEPTH {
            return Err(de::Error::custom("YAML nesting exceeds 10"));
        }
        self.charge::<D::Error>(64)?;
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for BoundedValue<'_> {
    type Value = Value;
    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("bounded YAML without custom tags")
    }
    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_none<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Number(v.into()))
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Value, E> {
        Ok(Value::Number(v.into()))
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Value, E> {
        Ok(Value::Number(v.into()))
    }
    fn visit_str<E: de::Error>(mut self, value: &str) -> Result<Value, E> {
        self.charge::<E>(value.len())?;
        Ok(Value::String(value.to_owned()))
    }
    fn visit_string<E: de::Error>(mut self, value: String) -> Result<Value, E> {
        self.charge::<E>(value.len())?;
        Ok(Value::String(value))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(BoundedValue {
            budget: self.budget,
            depth: self.depth + 1,
        })? {
            values.push(value);
        }
        Ok(Value::Sequence(values))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut mapping: A) -> Result<Value, A::Error> {
        let mut values = serde_yaml::Mapping::new();
        while let Some(key) = mapping.next_key_seed(BoundedValue {
            budget: self.budget,
            depth: self.depth + 1,
        })? {
            if values.contains_key(&key) {
                return Err(de::Error::custom("duplicate YAML mapping key"));
            }
            let value = mapping.next_value_seed(BoundedValue {
                budget: self.budget,
                depth: self.depth + 1,
            })?;
            values.insert(key, value);
        }
        // Aliases have already been decoded and charged. Move inherited
        // values (never clone them) and retain explicit mapping key order;
        // serde_yaml::apply_merge uses swap_remove and would reorder DNS policy.
        if let Some(merge) = values.shift_remove("<<") {
            let merges = match merge {
                Value::Sequence(items) => items,
                other => vec![other],
            };
            for merge in merges {
                let Value::Mapping(inherited) = merge else {
                    return Err(de::Error::custom(
                        "YAML merge requires a mapping or list of mappings",
                    ));
                };
                for (key, value) in inherited {
                    if !values.contains_key(&key) {
                        values.insert(key, value);
                    }
                }
            }
        }
        Ok(Value::Mapping(values))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_alias_expansion_is_bounded_during_parse() {
        let input = format!(
            "rules: [&r 'DOMAIN-KEYWORD,{},DIRECT', {}]",
            "a".repeat(8192),
            vec!["*r"; 1100].join(",")
        );
        assert!(input.len() < 20000);
        let error = parse(&input).expect_err("must stop before expanding all aliases");
        assert!(error.to_string().contains("budget"), "{error}");
    }

    #[test]
    fn small_aliases_keep_order_and_duplicate_keys_are_rejected() {
        let value = parse("dns: {z: &r [one], a: *r}").expect("bounded aliases");
        assert_eq!(value["dns"]["a"], value["dns"]["z"]);
        let rendered = serde_yaml::to_string(&value).expect("render");
        assert!(rendered.find("z:") < rendered.find("a:"));
        assert!(parse("rules: []\nrules: []").is_err());
        assert!(parse("rules: !script []").is_err());
        let merged = parse("groups: [&base {type: select, include-all-proxies: true}, {name: PROXY, <<: *base}]\ndns: {z: 1, <<: {b: 3}, a: 2}").expect("merge aliases");
        assert_eq!(merged["groups"][1]["type"], "select");
        let keys: Vec<_> = merged["dns"]
            .as_mapping()
            .expect("mapping")
            .keys()
            .filter_map(Value::as_str)
            .collect();
        assert_eq!(keys, ["z", "a", "b"]);
    }
}
