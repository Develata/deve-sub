//! Forward proxy URI decoding. DNS intent follows curl's scheme distinction.

use deve_sub_domain::{
    Authentication, Endpoint, HttpProxyConfig, Node, ProtocolConfig, ProtocolKind, Socks5Config,
    SocksDnsResolution,
};

use crate::ParseError;
use crate::uri::{
    build_common_tls, collect_query, decode_fragment, decode_userinfo, node_shell, parse_bool,
    parse_host,
};

pub(crate) fn parse(url: &url::Url, raw: &str) -> Result<Node, ParseError> {
    let socks = matches!(url.scheme(), "socks" | "socks5" | "socks5h");
    let query = collect_query(url);
    let allowed = [
        "tls",
        "sni",
        "alpn",
        "fp",
        "skip-cert-verify",
        "insecure",
        "pinSHA256",
        "spkiSHA256",
        "udp",
    ];
    if query.keys().any(|key| !allowed.contains(&key.as_str()))
        || url.query_pairs().count() != query.len()
        || !matches!(url.path(), "" | "/")
        // WHY: aliases must not silently override contradictory verification intent.
        || query.contains_key("skip-cert-verify") && query.contains_key("insecure")
    {
        return Err(invalid("unrepresented or repeated proxy URI option"));
    }
    let mut node = node_shell(Some(raw));
    node.display_name = decode_fragment(url);
    node.endpoint = Endpoint {
        host: parse_host(url.host_str().ok_or(ParseError::MissingField("host"))?)?,
        port: url.port_or_known_default().unwrap_or(1080),
    };
    if node.endpoint.port == 0 {
        return Err(ParseError::InvalidPort("zero".into()));
    }
    node.authentication = match (url.username(), url.password()) {
        ("", None) => Authentication::None,
        (username, Some(password)) => authentication(
            Some(decode_userinfo(username)),
            Some(decode_userinfo(password)),
            socks,
        )?,
        _ => return Err(ParseError::MissingField("password")),
    };
    let tls_enabled = query
        .get("tls")
        .map(|v| parse_bool(v))
        .transpose()?
        .unwrap_or(url.scheme() == "https");
    if url.scheme() == "https" && !tls_enabled || url.scheme() == "http" && tls_enabled {
        return Err(invalid("TLS option conflicts with HTTP URI scheme"));
    }
    let tls = build_common_tls(&query, &["skip-cert-verify", "insecure"], Some("pinSHA256"))?;
    if !tls_enabled
        && (tls.is_some() || query.contains_key("fp") || query.contains_key("spkiSHA256"))
    {
        return Err(invalid("TLS options require an enabled TLS connection"));
    }
    if tls_enabled {
        let mut tls = tls.unwrap_or_else(super::container::default_tls_enabled);
        tls.client_fingerprint = query.get("fp").cloned();
        if let Some(pins) = query.get("spkiSHA256") {
            tls.certificate_pins.extend(
                pins.split(',')
                    .map(|pin| deve_sub_domain::CertificatePin::new(format!("spkiSHA256:{pin}"))),
            );
        }
        node.tls = Some(tls);
    } else if query.contains_key("tls") {
        let mut tls = super::container::default_tls_enabled();
        tls.enabled = false;
        node.tls = Some(tls);
    }
    node.udp.supported = query.get("udp").map(|v| parse_bool(v)).transpose()?;
    if socks {
        node.protocol = ProtocolKind::Socks5;
        node.config = ProtocolConfig::Socks5(Socks5Config {
            dns_resolution: match url.scheme() {
                "socks5h" => SocksDnsResolution::Remote,
                "socks5" => SocksDnsResolution::Local,
                _ => SocksDnsResolution::Unspecified,
            },
        });
    } else {
        if node.udp.supported == Some(true) {
            return Err(invalid("HTTP CONNECT does not carry UDP"));
        }
        node.protocol = ProtocolKind::Http;
        node.config = ProtocolConfig::Http(HttpProxyConfig::default());
    }
    Ok(node)
}

pub(crate) fn authentication(
    username: Option<String>,
    password: Option<String>,
    socks: bool,
) -> Result<Authentication, ParseError> {
    match (username, password) {
        (None, None) => Ok(Authentication::None),
        (Some(username), Some(password)) => {
            // RFC 1929 lengths count octets; HTTP Basic forbids ':' in usernames.
            if socks
                && (!(1..=255).contains(&username.len()) || !(1..=255).contains(&password.len()))
                || !socks && username.contains(':')
            {
                return Err(invalid("invalid proxy authentication fields"));
            }
            Ok(Authentication::UserPassword { username, password })
        }
        _ => Err(invalid("username and password must both be supplied")),
    }
}

pub(crate) fn invalid(reason: &str) -> ParseError {
    ParseError::InvalidField {
        field: "forward proxy",
        value: reason.to_owned(),
    }
}
