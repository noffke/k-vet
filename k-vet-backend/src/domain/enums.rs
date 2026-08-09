//! The database enums, mapped once for both sqlx and the OpenAPI contract.
//!
//! Wire values equal the stored values (snake_case); user-facing labels are translated
//! in the frontend.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

macro_rules! db_enum {
    ($(#[$meta:meta])* $name:ident => $type_name:literal { $($variant:ident),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type, ToSchema,
        )]
        #[sqlx(type_name = $type_name, rename_all = "snake_case")]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $($variant),+
        }
    };
}

db_enum! {
    /// Form of address; `familie` addresses a household.
    Salutation => "salutation" { Frau, Herr, Familie }
}

db_enum! {
    /// Original packaging (bought, stocked) versus a subset dispensed from it.
    PackagingKind => "packaging_kind" { Original, Subset }
}

db_enum! {
    /// A stock movement is either a dispense tied to a treatment line or a correction.
    MovementKind => "movement_kind" { Dispense, Correction }
}

db_enum! {
    /// Official fee schedule position versus a self-defined service.
    ServiceType => "service_type" { Got, SelfDefined }
}

db_enum! {
    /// What a template line refers to.
    TemplateItemKind => "template_item_kind" { DrugPackaging, Service }
}

db_enum! {
    /// What a billing line refers to.
    TreatmentItemKind => "treatment_item_kind" { DrugPackaging, Service }
}

db_enum! {
    /// Who owns an attachment: a patient, a treatment, or a referencing record.
    AttachmentKind => "attachment_kind" { PatientFile, TreatmentFile, Referenced }
}

db_enum! {
    /// Invoice lifecycle: written, released, dispatched to the customer, handed to bookkeeping.
    /// Dispatch is a precondition of the hand-off, which is what keeps one column sufficient.
    InvoiceStatus => "invoice_status" { Created, Accepted, Sent, Submitted, Cancelled }
}

db_enum! {
    /// Type of a customer email address.
    EmailType => "email_type" { Private, Work, Other }
}

impl std::fmt::Display for Salutation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::Frau => "Frau",
            Self::Herr => "Herr",
            Self::Familie => "Familie",
        };
        formatter.write_str(text)
    }
}
