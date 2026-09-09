use super::*;

#[test]
fn invalid_lines_also_consume_the_entry_budget() {
    let body = "not-a-node\n".repeat(100_000);
    assert!(matches!(
        parse_content(SourceType::UriList, None, body.as_bytes()),
        Err(ParseContentError::TooManyNodes(10_001))
    ));
}

#[test]
fn oversized_raw_input_is_rejected_before_decoding() {
    assert!(matches!(
        parse_content(SourceType::Base64, None, &vec![b'A'; MAX_BODY_BYTES + 1]),
        Err(ParseContentError::TooLarge)
    ));
}

#[test]
fn containers_check_count_before_constructing_nodes() {
    let body = format!(
        "{{\"outbounds\":[{}]}}",
        vec!["{}"; MAX_NODES + 1].join(",")
    );
    assert!(matches!(
        parse_content(SourceType::SingboxJson, None, body.as_bytes()),
        Err(ParseContentError::TooManyNodes(10_001))
    ));
}

#[test]
fn yaml_alias_bomb_and_recursive_containers_fail_without_panicking() {
    let yaml = "proxies: &a [*a]";
    assert!(parse_content(SourceType::MihomoYaml, None, yaml.as_bytes()).is_err());
    let json = format!("{{\"outbounds\":{}0{}}}", "[".repeat(200), "]".repeat(200));
    assert!(parse_content(SourceType::SingboxJson, None, json.as_bytes()).is_err());
    let mut bomb = String::from("a: &a [1,1,1,1,1,1,1,1,1,1]\n");
    for (current, previous) in [('b', 'a'), ('c', 'b'), ('d', 'c'), ('e', 'd'), ('f', 'e')] {
        bomb.push_str(&format!(
            "{current}: &{current} [{}]\n",
            vec![format!("*{previous}"); 10].join(",")
        ));
    }
    bomb.push_str("proxies: *f\n");
    assert!(parse_content(SourceType::MihomoYaml, None, bomb.as_bytes()).is_err());
}
