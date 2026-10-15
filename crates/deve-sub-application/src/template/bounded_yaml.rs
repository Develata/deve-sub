//! Apply the shared template YAML limits before application schema checks.

use deve_sub_yaml::Value;

pub(super) fn parse(input: &str) -> Result<Value, deve_sub_yaml::Error> {
    deve_sub_yaml::parse_template(input)
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
        let rendered = deve_sub_yaml::to_string(&value).expect("render");
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
