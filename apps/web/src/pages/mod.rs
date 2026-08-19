//! Page components for the Deve Sub web frontend.

#![cfg(target_family = "wasm")]

pub mod audit;
pub mod audit_types;
pub mod dashboard;
pub mod login;
pub mod node_chain_modal;
pub mod node_import_modal;
pub mod node_override_modal;
pub mod node_tag_modal;
pub mod node_types;
pub mod nodes;
pub mod settings;
pub mod setup;
pub mod source_types;
pub mod sources;
pub mod subscription_modals;
pub mod subscription_types;
pub mod subscriptions;
pub mod template_gen_modal;
pub mod template_modals;
pub mod template_types;
pub mod template_versions;
pub mod templates;
pub mod twofa_settings;
pub mod user_modals;
pub mod user_types;
pub mod users;
pub mod util;
