//! VEE Core Types
//!
//! Typed artifacts, capabilities, budgets, hypotheses, and execution events.
//! Adapted from the VEE Technical Directive v1.0 for the Tauri architecture.
//! This module is intentionally decomposed by domain to avoid a god-module.

pub mod api;
pub mod artifact;
pub mod budget;
pub mod capability;
pub mod events;
pub mod osmosis;
pub mod pattern;
pub mod provenance;
pub mod result;
pub mod schema;
pub mod task;

pub use api::*;
pub use artifact::*;
pub use budget::*;
pub use capability::*;
pub use events::*;
pub use osmosis::*;
pub use pattern::*;
pub use provenance::*;
pub use result::*;
pub use schema::*;
pub use task::*;
