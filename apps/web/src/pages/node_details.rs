//! Inspect server-projected node metadata without exposing credentials.
#![cfg(target_family = "wasm")]

use deve_sub_contract::NodeDto;
use dioxus::prelude::*;

use crate::i18n::{Language, t};
use crate::pages::modal_dialog::ModalDialog;

#[component]
pub fn NodeDetails(lang: Language, node: NodeDto, on_close: EventHandler<()>) -> Element {
    let details = &node.connection;
    let three_state = |value| match value {
        None => t(lang, "nodes.not_specified"),
        Some(true) => t(lang, "nodes.yes"),
        Some(false) => t(lang, "nodes.no"),
    };
    let fields = [
        (t(lang, "nodes.protocol"), node.protocol.clone()),
        (t(lang, "nodes.endpoint"), format!("{}:{}", node.host, node.port)),
        (t(lang, "nodes.authentication"), t(lang, &format!("nodes.auth_{}", details.authentication)).to_owned()),
        ("TLS", three_state(details.tls_enabled).to_owned()),
        (t(lang, "nodes.skip_verification"), three_state(details.skip_cert_verify).to_owned()),
        ("SNI", details.server_name.clone().unwrap_or_else(|| "—".into())),
        ("ALPN", details.alpn.join(", ")),
        (t(lang, "nodes.certificate_pins"), details.certificate_pin_count.to_string()),
        (t(lang, "nodes.destination_dns"), details.destination_dns.as_ref().map(|dns| t(lang, &format!("nodes.dns_{dns}")).to_owned()).unwrap_or_else(|| "—".into())),
    ];
    rsx! {
        ModalDialog { title: t(lang, "nodes.details"), busy: false, on_close,
            p { class: "mt-3 break-all font-medium", "{node.display_name}" }
            dl { class: "mt-4 grid grid-cols-2 gap-3 text-sm",
                for (label, value) in fields {
                    dt { class: "text-muted", "{label}" }
                    dd { class: "break-all", "{value}" }
                }
            }
            if node.security_quarantined {
                p { class: "ui-error mt-4 text-sm", {t(lang, "nodes.security_quarantine_hint")} }
            }
            if let Some(reason) = node.unsupported_reason {
                p { class: "ui-error mt-4 text-sm", "{reason}" }
            }
            div { class: "mt-6 flex justify-end",
                button { class: "ui-button", autofocus: true, onclick: move |_| on_close.call(()), {t(lang, "common.close")} }
            }
        }
    }
}
