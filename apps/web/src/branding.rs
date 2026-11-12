//! One reactive display name for the shell, authentication pages and title.

use dioxus::prelude::*;

/// Browser customization overrides the server's configured display name only
/// in this browser; it does not modify server configuration or the 2FA issuer.
#[derive(Clone, PartialEq)]
pub struct Branding {
    pub server_name: String,
    pub custom_name: Option<String>,
}

impl Branding {
    /// Current browser override, falling back to the configured service name.
    pub fn display_name(&self) -> &str {
        self.custom_name.as_deref().unwrap_or(&self.server_name)
    }
}

/// Load public configuration without delaying authentication or navigation.
pub fn use_branding() -> Signal<Branding> {
    let mut branding = use_context_provider(|| Signal::new(Branding {
        server_name: "Deve Sub".to_string(),
        custom_name: crate::theme::read_custom_product_name(),
    }));
    use_future(move || async move {
        if let Ok(health) = crate::api::health().await {
            branding.write().server_name = health.product_name;
        }
    });
    branding
}
