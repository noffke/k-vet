//! The backend's integration tests, as one binary: a module per area, each driving the real
//! router on a database of its own (`common::database`). One binary rather than one per file —
//! twenty-nine of them each linked the whole application, which was much of the build time and
//! of the target directory's size.
//!
//! `cargo test --test api` runs them all; `cargo test --test api invoices::` one area;
//! `cargo test --test api invoices::creating_an_invoice_allocates` one test.

mod common;

mod appointments;
mod attachments;
mod auth;
mod config_endpoint;
mod customers;
mod dashboard;
mod dav;
mod deleting;
mod dispensing;
mod draft;
mod drug_codes;
mod email_templates;
mod got_catalogue;
mod invoice_admin;
mod invoices;
mod jobs;
mod mail_queue;
mod maps;
mod patients;
mod pharmacy;
mod picker;
mod price_quotes;
mod reminders;
mod rich_text;
mod services;
mod stock;
mod templates;
mod text_blocks;
mod treatments;
