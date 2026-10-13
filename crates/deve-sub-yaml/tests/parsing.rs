#![allow(clippy::expect_used)]

use deve_sub_yaml::{ErrorKind, Value, from_str, parse_source_json, parse_template, to_string};

#[test]
fn legacy_plain_scalars_keep_their_types_and_ranges() {
    for (input, expected) in [
        ("012", "012"),
        ("12_000", "12_000"),
        ("1e999", "1e999"),
        ("yes", "yes"),
        ("No", "No"),
        ("2026-10-13", "2026-10-13"),
        ("!!binary aA==", "aA=="),
        ("!!timestamp 2026-10-13", "2026-10-13"),
        ("!!str 3", "3"),
        ("'null'", "null"),
        ("\"true\"", "true"),
    ] {
        assert_eq!(parse_template(input).expect(input).as_str(), Some(expected));
    }
    for input in ["true", "True", "TRUE", "!!bool 'true'"] {
        assert_eq!(parse_template(input).expect(input).as_bool(), Some(true));
    }
    for (input, number) in [
        ("0xFF", 255),
        ("0o17", 15),
        ("0b101", 5),
        ("+42", 42),
        ("-0", 0),
    ] {
        assert_eq!(parse_template(input).expect(input).as_u64(), Some(number));
    }
    assert_eq!(
        parse_template("18446744073709551615")
            .expect("u64 max")
            .as_u64(),
        Some(u64::MAX)
    );
    assert_eq!(
        parse_template("-9223372036854775808")
            .expect("i64 min")
            .as_i64(),
        Some(i64::MIN)
    );
    for input in [
        "18446744073709551616",
        "-9223372036854775809",
        "!!int 012",
        "!!bool yes",
        "!!int 12_000",
    ] {
        assert!(parse_template(input).is_err(), "{input}");
    }
    assert!(
        parse_template(".nan")
            .expect("NaN")
            .as_f64()
            .expect("float")
            .is_nan()
    );
    assert_eq!(
        parse_template(".inf").expect("infinity").as_f64(),
        Some(f64::INFINITY)
    );
    assert_eq!(parse_template("1e2").expect("float").as_f64(), Some(100.0));
    assert!(
        parse_template("340282366920938463463374607431768211456")
            .expect("beyond u128 finite float")
            .as_f64()
            .is_some()
    );
    assert!(parse_template("!!int 340282366920938463463374607431768211456").is_err());
}

#[test]
fn ordered_merges_preserve_explicit_order_and_first_inheritance() {
    let parsed = parse_template("dns: {z: 1, '<<': [{b: 3, x: 1}, {x: 2, c: 4}], a: 2, x: 9}")
        .expect("merge");
    let map = parsed["dns"].as_mapping().expect("mapping");
    assert_eq!(
        map.keys().filter_map(Value::as_str).collect::<Vec<_>>(),
        ["z", "a", "x", "b", "c"]
    );
    assert_eq!(parsed["dns"]["x"].as_u64(), Some(9));
    let first = parse_template("<<: [{x: 1}, {x: 2}]").expect("sequence merge");
    assert_eq!(first["x"].as_u64(), Some(1));
    for input in ["a: 1\na: 2", "<<: 1", "<<: [{a: 1}, 1]", "<<: {}\n<<: {}"] {
        assert!(parse_template(input).is_err(), "{input}");
    }
}

#[test]
fn duplicate_complex_keys_are_checked_after_recursive_merge() {
    for input in [
        "? {x: 1, y: 2}\n: first\n? {y: 2, x: 1}\n: second",
        "? {<<: {x: 1}}\n: first\n? {x: 1}\n: second",
        ".nan: first\n.NaN: second",
        "-0.0: first\n0.0: second",
    ] {
        assert_eq!(
            parse_template(input).expect_err(input).kind(),
            ErrorKind::DuplicateKey
        );
    }
    let distinct = parse_template("1: integer\n1.0: float").expect("distinct numeric types");
    assert_eq!(distinct.as_mapping().expect("map").len(), 2);
}

#[test]
fn source_json_projection_keeps_lexical_keys_duplicates_and_merge_keys() {
    let parsed = parse_source_json("0xB: hex\n11: decimal\n18446744073709551616: big\n~: null-key\ntrue: bool-key\n<<: {one: 1}\none: 1\none: 2\nfloat: .nan").expect("JSON projection");
    assert_eq!(parsed["0xB"], "hex");
    assert_eq!(parsed["11"], "decimal");
    assert_eq!(parsed["18446744073709551616"], "big");
    assert_eq!(parsed["~"], "null-key");
    assert_eq!(parsed["true"], "bool-key");
    assert_eq!(parsed["<<"]["one"], 1);
    assert_eq!(parsed["one"], 2);
    assert!(parsed["float"].is_null());
    assert!(parse_source_json("value: 18446744073709551616").is_err());
    assert!(parse_source_json("? [a, b]\n: value").is_err());
}

#[test]
fn custom_tags_and_all_extra_document_content_are_rejected() {
    for input in [
        "!str secret",
        "!int 7",
        "!bool true",
        "!script []",
        "!<tag:example.invalid,2026:script> secret",
        "%TAG !x! tag:example.invalid,2026:\n---\n!x!script secret",
        "x: 1\n---\nx: 2",
        "x: 1\n...\n---\n",
        "x: 1\n...\n[unterminated",
        "x: 1\n...\ntrailing",
    ] {
        assert!(parse_template(input).is_err(), "{input}");
        assert!(parse_source_json(input).is_err(), "{input}");
    }
    assert_eq!(
        parse_template("x: 1\n...\n# final comment\n").expect("valid end")["x"].as_u64(),
        Some(1)
    );
}

#[test]
fn standard_tags_keep_legacy_scalar_and_collection_projections() {
    for tag in [
        "merge",
        "binary",
        "timestamp",
        "value",
        "omap",
        "pairs",
        "set",
        "unknown",
    ] {
        assert_eq!(
            parse_template(&format!("!!{tag} 012")).expect("standard scalar"),
            "012"
        );
        assert!(
            parse_template(&format!("!!{tag} [one, two]"))
                .expect("standard collection")
                .is_sequence()
        );
    }
    let merged =
        parse_template("dns: {z: 1, !!merge <<: {b: 3}, a: 2}").expect("standard merge tag");
    assert_eq!(merged["dns"]["b"].as_u64(), Some(3));
    assert_eq!(
        parse_source_json("!!null 012: value").expect("JSON keys retain spelling")["012"],
        "value"
    );
    assert_eq!(
        parse_template("%TAG !m! tag:yaml.org,2002:i\n---\n!m!nt 12")
            .expect("resolved standard namespace")
            .as_u64(),
        Some(12)
    );
}

#[test]
fn empty_documents_and_missing_source_keys_preserve_empty_spelling() {
    for input in ["", " ", "# comment\n", "---\n", "null"] {
        assert!(parse_template(input).expect("null document").is_null());
        assert!(parse_source_json(input).expect("null source").is_null());
    }
    for input in ["?\n: value", "{? : value}", "a: &a\n? *a\n: value"] {
        assert_eq!(parse_source_json(input).expect("empty key")[""], "value");
    }
    let both = parse_source_json("{? : 1, ~: 2}").expect("two different lexical keys");
    assert_eq!(both[""], 1);
    assert_eq!(both["~"], 2);
    assert_eq!(parse_template("!!str").expect("tagged empty scalar"), "");
    assert_eq!(
        parse_template("value: !!str").expect("tagged empty field")["value"],
        ""
    );
    assert_eq!(
        parse_template("value: !!str ~").expect("literal tilde")["value"],
        "~"
    );
}

#[test]
fn ambiguous_compact_flow_indicators_fail_without_reinterpreting_keys_or_values() {
    for input in [
        "{?}",
        "{?: value}",
        "{?foo: value}",
        "{??}",
        "[?foo]",
        "[?foo: value]",
        "{a: [?foo]}",
    ] {
        assert_eq!(
            parse_template(input).expect_err(input).kind(),
            ErrorKind::AmbiguousFlowIndicator
        );
        assert_eq!(
            parse_source_json(input).expect_err(input).kind(),
            ErrorKind::AmbiguousFlowIndicator
        );
    }
    let parsed = parse_template("{? foo: value}").expect("space separates explicit indicator");
    assert_eq!(parsed["foo"], "value");
    assert_eq!(
        parse_template("{'?foo': value}").expect("quoted literal")["?foo"],
        "value"
    );
    assert_eq!(
        parse_template("?foo: value").expect("block literal unaffected")["?foo"],
        "value"
    );
    assert_eq!(
        parse_template("['?foo']").expect("quoted flow value")[0],
        "?foo"
    );
    assert!(
        parse_template("{?\t}").is_err(),
        "known scanner limitation; use a space"
    );
    assert!(parse_template("{? }").is_ok());
    assert_eq!(
        parse_source_json("{ : value }").expect("standard omitted key")[""],
        "value"
    );
}

#[test]
fn question_marks_and_tabs_in_strings_and_comments_are_not_syntax_guards() {
    let input = concat!(
        "# {?foo: ignored}\t[?ignored]\n",
        "quoted: ['?foo', \"?password\\tvalue\", 'a?b']\n",
        "url: https://example.invalid/path?q=fixture&next=?question\n",
        "password: fixture?token\twith-tab\n",
        "flow: {url: https://example.invalid/?q=fixture, password: fixture?token}\n",
        "block: ?literal\n",
        "anchor: &literal '?anchored\\ttext'\n",
        "aliases: [*literal]\n",
        "# comment with a literal\ttab and ? question\n",
    );
    let ordered = parse_template(input).expect("ordinary strings and comments");
    assert_eq!(ordered["quoted"][0], "?foo");
    assert_eq!(ordered["quoted"][1], "?password\tvalue");
    assert_eq!(
        ordered["url"],
        "https://example.invalid/path?q=fixture&next=?question"
    );
    assert_eq!(ordered["password"], "fixture?token\twith-tab");
    assert_eq!(ordered["flow"]["password"], "fixture?token");
    assert_eq!(ordered["block"], "?literal");
    assert_eq!(ordered["aliases"][0], ordered["anchor"]);
    let json = parse_source_json(input).expect("source strings and comments");
    assert_eq!(json["quoted"][1], "?password\tvalue");
    assert_eq!(json["password"], "fixture?token\twith-tab");
    let rendered = to_string(&ordered).expect("emit");
    assert_eq!(parse_template(&rendered).expect("round trip"), ordered);
    assert_eq!(
        deve_sub_yaml::parse_generated_value(&rendered).expect("generated value"),
        ordered
    );
    let generated = format!(
        "{rendered}proxies: [{{password: '?literal\\ttext', url: 'https://example.invalid/?q=fixture'}}]\n"
    );
    assert_eq!(
        deve_sub_yaml::inspect_generated_sequence(&generated, "proxies").expect("generated check"),
        deve_sub_yaml::SequenceState::NonEmpty
    );
}

#[test]
fn reused_anchor_names_resolve_to_the_latest_definition_of_that_name() {
    // The old loader reused numeric IDs after rebinding a name; defining b
    // could accidentally redirect a later *a to b. Preserve YAML identity.
    let parsed = parse_template("a: &a 1\nb: &a 2\nc: &b 3\nd: *a").expect("rebound anchor");
    assert_eq!(parsed["d"].as_u64(), Some(2));
}

#[test]
fn safe_diagnostics_never_include_values_or_source_snippets() {
    for input in [
        "secret: [fixture-password-NEVER-PRINT",
        "!!int fixture-password-NEVER-PRINT",
        "!fixture-password-NEVER-PRINT value",
        "{fixture-password-NEVER-PRINT: 1, fixture-password-NEVER-PRINT: 2}",
    ] {
        let error = parse_template(input).expect_err("invalid");
        assert!(!format!("{error:?} {error}").contains("fixture-password"));
    }
}

#[test]
fn serializer_keeps_ambiguous_strings_and_scalar_numbers_on_round_trip() {
    let original = Value::Sequence(
        [
            "012",
            "12_000",
            "1e999",
            ".nan",
            ".inf",
            "true",
            "on",
            "No",
            "null",
            "2026-10-13",
            "${ENV}",
            "line one\nline two\n",
            "a\0b\tc",
            "---",
            "!str hi",
        ]
        .into_iter()
        .map(|s| Value::String(s.into()))
        .chain([
            Value::Number(1.0.into()),
            Value::Number(f64::NAN.into()),
            Value::Number(f64::INFINITY.into()),
        ])
        .collect(),
    );
    let text = to_string(&original).expect("serialize");
    let parsed: Value = from_str(&text).expect("parse serialized YAML");
    assert_eq!(parsed, original, "{text}");
}
