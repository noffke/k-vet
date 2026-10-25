//! HTTP layer: one module per resource, utoipa-annotated.

use axum::Router;

use crate::AppState;

pub mod address_check;
pub mod appointments;
pub mod attachments;
pub mod common;
pub mod config;
pub mod customers;
pub mod dashboard;
pub mod drugs;
pub mod email_templates;
pub mod invoices;
pub mod lots;
pub mod mail_queue;
pub mod manufacturers;
pub mod patient_treatments;
pub mod patients;
pub mod picker;
pub mod price_quotes;
pub mod pricing;
pub mod services;
pub mod settings;
pub mod suppliers;
pub mod templates;
pub mod text_blocks;
pub mod travel_distance;
pub mod treatment_items;
pub mod treatments;

/// Every `/api` route that requires a session.
pub fn protected_routes() -> Router<AppState> {
    Router::new()
        .merge(address_check::routes())
        .merge(appointments::routes())
        .merge(attachments::routes())
        .merge(config::routes())
        .merge(customers::routes())
        .merge(dashboard::routes())
        .merge(drugs::routes())
        .merge(email_templates::routes())
        .merge(lots::routes())
        .merge(mail_queue::routes())
        .merge(manufacturers::routes())
        .merge(invoices::routes())
        .merge(patient_treatments::routes())
        .merge(patients::routes())
        .merge(picker::routes())
        .merge(price_quotes::routes())
        .merge(pricing::routes())
        .merge(services::routes())
        .merge(settings::routes())
        .merge(suppliers::routes())
        .merge(templates::routes())
        .merge(text_blocks::routes())
        .merge(treatment_items::routes())
        .merge(travel_distance::routes())
        .merge(treatments::routes())
}
