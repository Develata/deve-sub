#![allow(clippy::expect_used)]

use deve_sub_yaml::{
    ErrorKind, Limits, parse_source_json, parse_template, parse_value_with_limits,
};

#[test]
fn aliases_charge_the_exact_logical_value_budget_before_expansion() {
    let input = "[&a hi, *a]";
    let limits = Limits {
        input_bytes: input.len(),
        expanded_bytes: 196,
        depth: 10,
    };
    assert!(parse_value_with_limits(input, limits).is_ok());
    assert_eq!(
        parse_value_with_limits(
            input,
            Limits {
                expanded_bytes: 195,
                ..limits
            }
        )
        .expect_err("one byte short")
        .kind(),
        ErrorKind::Budget
    );
    assert_eq!(
        parse_value_with_limits(
            input,
            Limits {
                input_bytes: input.len() - 1,
                ..limits
            }
        )
        .expect_err("input limit")
        .kind(),
        ErrorKind::Input
    );
}

#[test]
fn merge_overrides_cannot_hide_the_pre_merge_alias_cost() {
    let input = format!(
        "[&b {{x: '{}'}}, {}]",
        "x".repeat(8192),
        vec!["{x: 1, <<: *b}"; 300].join(",")
    );
    assert!(input.len() < 16000);
    let limits = Limits {
        expanded_bytes: 1024 * 1024,
        ..Limits::TEMPLATE
    };
    assert_eq!(
        parse_value_with_limits(&input, limits)
            .expect_err("discarded aliases still charged")
            .kind(),
        ErrorKind::Budget
    );
}

#[test]
fn scalar_alias_bombs_fail_under_the_existing_template_budget() {
    let input = format!(
        "[&a '{}', {}]",
        "x".repeat(8192),
        vec!["*a"; 1100].join(",")
    );
    assert!(input.len() < 20000);
    assert_eq!(
        parse_template(&input).expect_err("bounded").kind(),
        ErrorKind::Budget
    );
}

#[test]
fn alias_jumps_are_bounded_even_below_the_value_byte_budget() {
    let mut input = String::from("a: &a [x, x, x, x]\n");
    for index in 1..8 {
        let prev = (b'a' + index - 1) as char;
        let next = (b'a' + index) as char;
        input.push_str(&format!(
            "{next}: &{next} [*{prev}, *{prev}, *{prev}, *{prev}]\n"
        ));
    }
    assert_eq!(
        parse_template(&input)
            .expect_err("excessive alias jumps")
            .kind(),
        ErrorKind::Alias
    );
}

#[test]
fn value_depth_includes_aliased_subtrees_and_recursive_aliases_fail() {
    for levels in [9, 10] {
        let input = format!("{}x{}", "[".repeat(levels), "]".repeat(levels));
        assert!(parse_template(&input).is_ok(), "{levels}");
    }
    assert!(parse_template(&format!("{}x{}", "[".repeat(11), "]".repeat(11))).is_err());
    let input = format!(
        "[&a {}x{}, {}*a{}]",
        "[".repeat(7),
        "]".repeat(7),
        "[".repeat(3),
        "]".repeat(3)
    );
    assert_eq!(
        parse_template(&input)
            .expect_err("alias contributes depth")
            .kind(),
        ErrorKind::Depth
    );
    for input in ["&a [*a]", "&a {<<: *a}", "[*missing]"] {
        assert!(parse_template(input).is_err());
    }
}

#[test]
fn general_source_depth_keeps_the_128_container_boundary() {
    assert!(parse_source_json(&format!("{}0{}", "[".repeat(128), "]".repeat(128))).is_ok());
    assert!(parse_source_json(&format!("{}0{}", "[".repeat(129), "]".repeat(129))).is_err());
    assert!(parse_source_json(&format!("{}{}", "[".repeat(129), "]".repeat(129))).is_err());
}

#[test]
fn ordinary_ten_thousand_node_sources_fit_the_source_profile() {
    let nodes = (0..10000).map(|i| format!("- {{name: node-{i}, type: trojan, server: example.invalid, port: 443, password: fixture}}\n")).collect::<String>();
    let input = format!("proxies:\n{nodes}");
    let parsed = parse_source_json(&input).expect("source profile supports pool limit");
    assert_eq!(parsed["proxies"].as_array().expect("nodes").len(), 10000);
}
