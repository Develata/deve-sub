#![allow(clippy::expect_used)]

use deve_sub_yaml::{
    ErrorKind, SequenceState, inspect_generated_sequence, parse_generated_value, parse_source_json,
    parse_template, to_string,
};

#[test]
fn literal_non_ascii_breaks_fail_before_changing_values_or_comment_boundaries() {
    for line_break in ['\u{85}', '\u{2028}', '\u{2029}'] {
        for input in [
            format!("value: 1{line_break}"),
            format!("value: 'before{line_break}after'"),
            format!("value: \"before{line_break}after\""),
            format!("{{value: before{line_break}after}}"),
            format!("value: |\n  before{line_break}  after\n"),
            // A scalar-only check misses this: libyaml reads `other`, while
            // granit considers the rest of the line part of the comment.
            format!("value: 1\n# comment{line_break}other: 2\n"),
            format!("# comment{line_break}proxies: []\n"),
        ] {
            for error in [
                parse_template(&input).expect_err("template ambiguity"),
                parse_source_json(&input).expect_err("source ambiguity"),
                parse_generated_value(&input).expect_err("stored YAML ambiguity"),
                inspect_generated_sequence(&input, "proxies").expect_err("output ambiguity"),
            ] {
                assert_eq!(error.kind(), ErrorKind::AmbiguousLineBreak);
                assert!(!error.to_string().contains("before"));
                assert!(!error.to_string().contains("comment"));
            }
        }
    }
}

#[test]
fn escaped_non_ascii_breaks_preserve_exact_values_through_every_boundary() {
    for (escaped, character) in [
        ("\\u0085", '\u{85}'),
        ("\\N", '\u{85}'),
        ("\\u2028", '\u{2028}'),
        ("\\L", '\u{2028}'),
        ("\\u2029", '\u{2029}'),
        ("\\P", '\u{2029}'),
    ] {
        let input = format!("value: \"before{escaped}after\"\nproxies: [1]\n");
        let expected = format!("before{character}after");
        let value = parse_template(&input).expect("escaped character");
        assert_eq!(value["value"].as_str(), Some(expected.as_str()));
        assert_eq!(
            parse_source_json(&input).expect("source")["value"],
            expected
        );
        assert_eq!(parse_generated_value(&input).expect("generated"), value);
        let rendered = to_string(&value).expect("serialize");
        assert!(
            !rendered.contains(character),
            "serializer must escape breaks"
        );
        assert_eq!(parse_template(&rendered).expect("round trip"), value);
        assert_eq!(
            inspect_generated_sequence(&rendered, "proxies").expect("output check"),
            SequenceState::NonEmpty
        );
    }
}

#[test]
fn ordinary_line_breaks_and_indented_quoted_continuations_keep_working() {
    for line_break in ["\n", "\r", "\r\n"] {
        let input = format!("value: \"before{line_break}  after\"{line_break}proxies: [1]");
        assert_eq!(
            parse_template(&input).expect("line break")["value"],
            "before after"
        );
        assert_eq!(
            parse_source_json(&input).expect("source")["value"],
            "before after"
        );
    }
    assert!(parse_template("value: \"before\nafter\"").is_err());
}
