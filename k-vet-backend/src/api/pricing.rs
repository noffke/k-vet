//! Live AMPreisV price preview for the packaging editor (T051).

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::AppState;
use crate::domain::enums::PackagingKind;
use crate::domain::money;
use crate::error::AppResult;

#[derive(Debug, Deserialize, ToSchema)]
pub struct PricePreviewRequest {
    pub kind: PackagingKind,
    /// Purchase price without VAT — of the original packaging in both cases.
    pub list_price_net: Decimal,
    pub vat_percent: Decimal,
    /// Content of the original packaging (subset previews need it for the pro rata).
    pub original_quantity: Option<Decimal>,
    /// Content of the subset being priced.
    pub subset_quantity: Option<Decimal>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PricePreview {
    /// § 3(2) basis the surcharge is levied on.
    pub basis_net: Decimal,
    /// The statutory surcharge.
    pub surcharge: Decimal,
    pub net: Decimal,
    pub gross: Decimal,
    /// Which rule produced the price, for the hint next to the field.
    pub rule: String,
}

#[utoipa::path(
    post,
    path = "/api/pricing/preview",
    operation_id = "previewPrice",
    tag = "pharmacy",
    request_body = PricePreviewRequest,
    responses((status = 200, body = PricePreview))
)]
pub async fn preview(
    State(_state): State<AppState>,
    Json(body): Json<PricePreviewRequest>,
) -> AppResult<Json<PricePreview>> {
    let (price, rule) = match body.kind {
        PackagingKind::Original => (
            money::drug_price_original(body.list_price_net, body.vat_percent),
            "AMPreisV § 3 Abs. 3/4, § 10 Abs. 2",
        ),
        PackagingKind::Subset => (
            money::drug_price_subset(
                body.list_price_net,
                body.original_quantity.unwrap_or(Decimal::ZERO),
                body.subset_quantity.unwrap_or(Decimal::ZERO),
                body.vat_percent,
            ),
            "AMPreisV § 4 Abs. 1/2 (Teilmengenzuschlag)",
        ),
    };

    Ok(Json(PricePreview {
        basis_net: price.basis_net,
        surcharge: price.surcharge,
        net: price.net,
        gross: price.gross,
        rule: rule.to_owned(),
    }))
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/pricing/preview", post(preview))
}
