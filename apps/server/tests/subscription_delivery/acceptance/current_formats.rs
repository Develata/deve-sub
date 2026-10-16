//! Additional client-schema edges pass through the same import and HTTP surface.
use super::*;

pub(super) fn variants() -> Vec<Case> {
    let fixtures = cases();
    let mut tls = fixtures[9].clone();
    tls.input = tls.input.replace("obfs=http", "obfs=tls");
    tls.fields
        .insert("/config/Snell/obfs/mode".into(), json!("Tls"));
    tls.fields.insert("/tls".into(), json!({"enabled":true,"server_name":"cover.example","skip_cert_verify":null,"alpn":[],"client_fingerprint":null,"certificate_pins":[],"reality":null}));
    tls.profiles = vec!["mihomo".into(), "sing-box".into()];

    let mut snell6 = fixtures[9].clone();
    snell6.source_type = "singbox_json".into();
    snell6.input = json!({"outbounds":[{"type":"snell","tag":"acceptance-snell","server":"snell.example","server_port":443,"version":6,"psk":"TEST_SNELL","mode":"unsafe-raw"}]}).to_string();
    snell6.fields.remove("/config/Snell/obfs/mode");
    snell6.fields.remove("/config/Snell/obfs/host");
    snell6
        .fields
        .insert("/config/Snell/obfs".into(), Value::Null);
    snell6
        .fields
        .insert("/config/Snell/version".into(), json!("V6"));
    snell6
        .fields
        .insert("/config/Snell/v6_mode".into(), json!("UnsafeRaw"));
    snell6.profiles = vec!["sing-box".into()];

    let key = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";
    let mut wg = fixtures[7].clone();
    wg.source_type = "auto".into();
    wg.input = json!({"endpoints":[{"type":"wireguard","tag":"acceptance-wg","private_key":key,"address":["192.0.2.2/32","2001:db8::2/128"],"mtu":1408,"workers":2,"peers":[{"address":"2001:db8::1","port":443,"public_key":key,"pre_shared_key":key,"allowed_ips":["0.0.0.0/0","::/0"],"reserved":[1,2,3],"persistent_keepalive_interval":25}]}]}).to_string();
    wg.fields.insert(
        "/endpoint/host".into(),
        json!({"type":"ipv6","value":"2001:db8::1"}),
    );
    wg.fields.insert(
        "/config/WireGuard/peers/0/pre_shared_key".into(),
        json!(key),
    );
    wg.fields
        .insert("/config/WireGuard/workers".into(), json!(2));
    wg.profiles = vec!["sing-box".into()];

    let mut reality = fixtures[0].clone();
    reality.source_type = "singbox_json".into();
    reality.input = json!({"outbounds":[{"type":"vless","tag":"acceptance-vless","server":"vless.example","server_port":443,"uuid":"00000000-0000-4000-8000-000000000001","flow":"xtls-rprx-vision","tls":{"enabled":true,"server_name":"vless.example","utls":{"enabled":true,"fingerprint":"chrome"},"reality":{"enabled":true,"public_key":"AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8","short_id":"01020304"}}}]}).to_string();
    reality.profiles = vec!["sing-box".into(), "xray".into()];
    vec![tls, snell6, wg, reality]
}

pub(super) async fn delivered(case: &Case, profile: &str) -> String {
    let app = TestApp::new().await;
    let router = app.router();
    let cookie = setup_and_login(&router).await;
    import_cases(&app, &router, &cookie, std::slice::from_ref(case)).await;
    let id = template(&router, &cookie, &v3(None)).await;
    let token = subscription(&router, &cookie, &id, profile, None).await;
    let response = router
        .oneshot(get(&format!("/sub/{token}/{profile}")))
        .await
        .expect("delivery");
    assert_eq!(response.status(), StatusCode::OK);
    let content = body_to_string(response).await;
    assert_output(&content, profile, &[case]);
    content
}

#[tokio::test]
async fn acceptance_current_endpoint_snell_and_implicit_tcp_security_delivery() {
    for case in variants() {
        for profile in &case.profiles {
            delivered(&case, profile).await;
        }
    }
}
