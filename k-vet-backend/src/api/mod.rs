//! HTTP layer: one module per resource, utoipa-annotated.

use axum::Router;

use crate::AppState;

pub mod appointments;
pub mod attachments;
pub mod common;
pub mod config;
pub mod customers;
pub mod dashboard;
pub mod drugs;
pub mod invoices;
pub mod lots;
pub mod manufacturers;
pub mod patients;
pub mod picker;
pub mod pricing;
pub mod services;
pub mod settings;
pub mod suppliers;
pub mod templates;
pub mod treatment_items;
pub mod treatments;

/// Every `/api` route that requires a session.
pub fn protected_routes() -> Router<AppState> {
    Router::new()
        .merge(appointments::routes())
        .merge(attachments::routes())
        .merge(config::routes())
        .merge(customers::routes())
        .merge(dashboard::routes())
        .merge(drugs::routes())
        .merge(lots::routes())
        .merge(manufacturers::routes())
        .merge(invoices::routes())
        .merge(patients::routes())
        .merge(picker::routes())
        .merge(pricing::routes())
        .merge(services::routes())
        .merge(settings::routes())
        .merge(suppliers::routes())
        .merge(templates::routes())
        .merge(treatment_items::routes())
        .merge(treatments::routes())
}
