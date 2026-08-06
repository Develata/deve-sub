//! Fuzz tests for illegal input (PARSE-018).
//!
//! Property: feeding arbitrary input to every parser entry point must not
//! crash, panic, abort, hang, or allocate without bound. Parsers must return
//! `Ok` or `Err` deterministically.
//!
//! This is a proptest-based fuzz substitute. Coverage-guided fuzzing
//! (`cargo-fuzz`) can be added later for deeper exploration; this suite
//! provides the acceptance gate required by M3.

#![allow(clippy::expect_used)]

use proptest::prelude::*;

use deve_sub_protocol::container::{
    parse_base64_subscription, parse_mihomo_yaml, parse_shadowrocket, parse_singbox_json,
    parse_uri_list, parse_v2ray_json, parse_xray_json,
};
use deve_sub_protocol::parse_uri;

/// Arbitrary strings up to 4 KiB. Large enough to exercise structural parsing
/// without making the test prohibitively slow.
fn arb_input() -> impl Strategy<Value = String> {
    "[\x00-\x7f]{0,4096}"
}

proptest! {
    /// `parse_uri` must not panic on arbitrary input.
    #[test]
    fn fuzz_parse_uri_no_panic(input in arb_input()) {
        let _ = parse_uri(&input);
    }

    /// `parse_uri_list` must not panic on arbitrary input.
    #[test]
    fn fuzz_parse_uri_list_no_panic(input in arb_input()) {
        let _ = parse_uri_list(&input);
    }

    /// `parse_base64_subscription` must not panic on arbitrary input.
    #[test]
    fn fuzz_parse_base64_subscription_no_panic(input in arb_input()) {
        let _ = parse_base64_subscription(&input);
    }

    /// `parse_shadowrocket` must not panic on arbitrary input.
    #[test]
    fn fuzz_parse_shadowrocket_no_panic(input in arb_input()) {
        let _ = parse_shadowrocket(&input);
    }

    /// `parse_mihomo_yaml` must not panic on arbitrary input.
    #[test]
    fn fuzz_parse_mihomo_yaml_no_panic(input in arb_input()) {
        let _ = parse_mihomo_yaml(&input);
    }

    /// `parse_singbox_json` must not panic on arbitrary input.
    #[test]
    fn fuzz_parse_singbox_json_no_panic(input in arb_input()) {
        let _ = parse_singbox_json(&input);
    }

    /// `parse_xray_json` must not panic on arbitrary input.
    #[test]
    fn fuzz_parse_xray_json_no_panic(input in arb_input()) {
        let _ = parse_xray_json(&input);
    }

    /// `parse_v2ray_json` must not panic on arbitrary input.
    #[test]
    fn fuzz_parse_v2ray_json_no_panic(input in arb_input()) {
        let _ = parse_v2ray_json(&input);
    }
}
