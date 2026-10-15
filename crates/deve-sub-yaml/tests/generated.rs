#![allow(clippy::expect_used)]

use deve_sub_yaml::{ErrorKind, SequenceState, inspect_generated_sequence};

#[test]
fn generated_sequence_states_follow_the_full_document() {
    for (input, expected) in [
        ("rules: []", SequenceState::Missing),
        ("proxies:", SequenceState::NotSequence),
        ("proxies: []", SequenceState::Empty),
        ("proxies: [1]\ndns: {}", SequenceState::NonEmpty),
    ] {
        assert_eq!(
            inspect_generated_sequence(input, "proxies").expect(input),
            expected
        );
    }
}

#[test]
fn generated_validation_catches_tail_errors_tags_aliases_and_nested_duplicates() {
    for input in [
        "proxies: [1]\ndns: [",
        "proxies: [1]\n---\nx: 1",
        "proxies: [1]\n...\n[invalid",
        "proxies: [1]\nproxies: [2]",
        "proxies: [{plugin-opts: {mode: tls, mode: http}}]",
        "proxies: [!script value]",
        "proxies: [&x value, *x]",
        "proxies: [1]\n? {x: 1, y: 2}\n: a\n? {y: 2, x: 1}\n: b",
    ] {
        assert!(
            inspect_generated_sequence(input, "proxies").is_err(),
            "{input}"
        );
    }
    let error = inspect_generated_sequence("proxies: [{mode: tls, mode: http}]", "proxies")
        .expect_err("nested duplicate");
    assert_eq!(error.kind(), ErrorKind::DuplicateKey);
}

#[test]
fn generated_output_does_not_inherit_incoming_source_byte_limits() {
    let input = format!(
        "proxies: [{{password: '{}'}}]",
        "x".repeat(10 * 1024 * 1024 + 1)
    );
    assert_eq!(
        inspect_generated_sequence(&input, "proxies").expect("trusted emitted output"),
        SequenceState::NonEmpty
    );
}
