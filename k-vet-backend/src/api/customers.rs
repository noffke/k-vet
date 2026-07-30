//! Customers: structured names, addresses, typed email addresses, phone (T043).

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::AppState;
use crate::api::common::{ListQuery, double_option};
use crate::api::settings::validate_country;
use crate::domain::contact::{display_phone, validate_email, validate_phone};
use crate::domain::draft::recompute_draft;
use crate::domain::enums::{EmailType, Salutation};
use crate::error::{AppError, AppResult};

#[derive(Debug, Serialize, ToSchema)]
pub struct Customer {
    pub id: i64,
    pub salutation: Option<Salutation>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    /// Optional second person of the household, addressed on the invoice as well.
    pub second_salutation: Option<Salutation>,
    pub second_first_name: Option<String>,
    pub second_last_name: Option<String>,
    pub home_addon: Option<String>,
    pub home_street: Option<String>,
    pub home_zip: Option<String>,
    pub home_city: Option<String>,
    /// ISO 3166-1 alpha-2; null falls back to `[invoice] default_country`.
    pub home_country: Option<String>,
    pub invoice_salutation: Option<Salutation>,
    pub invoice_first_name: Option<String>,
    pub invoice_last_name: Option<String>,
    pub invoice_addon: Option<String>,
    pub invoice_street: Option<String>,
    pub invoice_zip: Option<String>,
    pub invoice_city: Option<String>,
    pub invoice_country: Option<String>,
    /// Stored E.164 form, e.g. `+493012345678`.
    pub phone: Option<String>,
    /// The same number in the familiar national format, e.g. `030 12345678`.
    pub phone_display: Option<String>,
    pub warning_remark: Option<String>,
    pub archived: bool,
    pub draft: bool,
    pub missing_fields: Vec<String>,
    /// `true` when the invoice address group is complete and replaces the home address.
    pub has_invoice_address: bool,
    pub has_second_name: bool,
    pub emails: Vec<CustomerEmail>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CustomerEmail {
    pub id: i64,
    pub email: String,
    pub email_type: EmailType,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct PatchCustomer {
    #[serde(default, deserialize_with = "double_option")]
    pub salutation: Option<Option<Salutation>>,
    #[serde(default, deserialize_with = "double_option")]
    pub first_name: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub last_name: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub second_salutation: Option<Option<Salutation>>,
    #[serde(default, deserialize_with = "double_option")]
    pub second_first_name: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub second_last_name: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub home_addon: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub home_street: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub home_zip: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub home_city: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<String>)]
    pub home_country: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub invoice_salutation: Option<Option<Salutation>>,
    #[serde(default, deserialize_with = "double_option")]
    pub invoice_first_name: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub invoice_last_name: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub invoice_addon: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub invoice_street: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub invoice_zip: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub invoice_city: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<String>)]
    pub invoice_country: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub phone: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub warning_remark: Option<Option<String>>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateCustomerEmail {
    pub email: String,
    #[serde(default = "default_email_type")]
    pub email_type: EmailType,
}

fn default_email_type() -> EmailType {
    EmailType::Private
}

#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct PatchCustomerEmail {
    pub email: Option<String>,
    pub email_type: Option<EmailType>,
}

#[utoipa::path(
    get,
    path = "/api/customers",
    operation_id = "listCustomers",
    tag = "customers",
    params(ListQuery),
    responses((status = 200, body = Vec<Customer>))
)]
pub async fn list(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> AppResult<Json<Vec<Customer>>> {
    // Search matches both names of a household (data-model.md).
    let ids: Vec<i64> = sqlx::query_scalar!(
        r#"SELECT id FROM customer
           WHERE ($2 OR NOT archived)
             AND ($1::text IS NULL
                  OR last_name ILIKE '%' || $1 || '%'
                  OR first_name ILIKE '%' || $1 || '%'
                  OR second_last_name ILIKE '%' || $1 || '%'
                  OR second_first_name ILIKE '%' || $1 || '%'
                  OR home_city ILIKE '%' || $1 || '%')
           ORDER BY last_name NULLS LAST, first_name NULLS LAST, id
           LIMIT $3 OFFSET $4"#,
        query.search(),
        query.include_archived(),
        query.limit(),
        query.offset(),
    )
    .fetch_all(&state.pool)
    .await?;

    let mut customers = Vec::with_capacity(ids.len());
    for id in ids {
        customers.push(load(&state.pool, id).await?);
    }
    Ok(Json(customers))
}

#[utoipa::path(
    post,
    path = "/api/customers",
    operation_id = "createCustomer",
    tag = "customers",
    responses((status = 200, description = "An empty draft, ready for auto-save", body = Customer))
)]
pub async fn create(State(state): State<AppState>) -> AppResult<Json<Customer>> {
    let id: i64 = sqlx::query_scalar!("INSERT INTO customer DEFAULT VALUES RETURNING id")
        .fetch_one(&state.pool)
        .await?;
    Ok(Json(load(&state.pool, id).await?))
}

#[utoipa::path(
    get,
    path = "/api/customers/{id}",
    operation_id = "getCustomer",
    tag = "customers",
    params(("id" = i64, Path,)),
    responses((status = 200, body = Customer), (status = 404))
)]
pub async fn detail(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Customer>> {
    Ok(Json(load(&state.pool, id).await?))
}

#[utoipa::path(
    patch,
    path = "/api/customers/{id}",
    operation_id = "patchCustomer",
    tag = "customers",
    params(("id" = i64, Path,)),
    request_body = PatchCustomer,
    responses((status = 200, body = Customer), (status = 404), (status = 422))
)]
pub async fn patch(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<PatchCustomer>,
) -> AppResult<Json<Customer>> {
    // Validated against the real ISO 3166-1 list — a shape check would accept `XX`.
    let home_country = match &body.home_country {
        Some(Some(code)) => Some(Some(validate_country("home_country", code)?)),
        other => other.clone(),
    };
    let invoice_country = match &body.invoice_country {
        Some(Some(code)) => Some(Some(validate_country("invoice_country", code)?)),
        other => other.clone(),
    };

    let mut transaction = state.pool.begin().await?;

    let current = sqlx::query!(
        r#"SELECT salutation AS "salutation?: Salutation", last_name, home_street, home_zip,
                  home_city, draft
           FROM customer WHERE id = $1 FOR UPDATE"#,
        id,
    )
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound)?;

    // Invalid phone numbers are never stored; the last valid value stays (R14).
    let phone = match &body.phone {
        Some(Some(raw)) if !raw.trim().is_empty() => Some(Some(validate_phone("phone", raw)?.e164)),
        other => other.clone(),
    };

    // Merge the patch over the stored row, then recompute the draft flag.
    let merged = |patched: &Option<Option<String>>, stored: &Option<String>| -> bool {
        match patched {
            Some(value) => value.as_ref().is_some_and(|text| !text.trim().is_empty()),
            None => stored.is_some(),
        }
    };
    let salutation_present = match &body.salutation {
        Some(value) => value.is_some(),
        None => current.salutation.is_some(),
    };
    let draft = recompute_draft(
        current.draft,
        &[
            ("salutation", salutation_present),
            ("last_name", merged(&body.last_name, &current.last_name)),
            (
                "home_street",
                merged(&body.home_street, &current.home_street),
            ),
            ("home_zip", merged(&body.home_zip, &current.home_zip)),
            ("home_city", merged(&body.home_city, &current.home_city)),
        ],
    )?;

    sqlx::query!(
        r#"UPDATE customer SET
               salutation         = CASE WHEN $2  THEN $3  ELSE salutation END,
               first_name         = CASE WHEN $4  THEN $5  ELSE first_name END,
               last_name          = CASE WHEN $6  THEN $7  ELSE last_name END,
               second_salutation  = CASE WHEN $8  THEN $9  ELSE second_salutation END,
               second_first_name  = CASE WHEN $10 THEN $11 ELSE second_first_name END,
               second_last_name   = CASE WHEN $12 THEN $13 ELSE second_last_name END,
               home_addon         = CASE WHEN $14 THEN $15 ELSE home_addon END,
               home_street        = CASE WHEN $16 THEN $17 ELSE home_street END,
               home_zip           = CASE WHEN $18 THEN $19 ELSE home_zip END,
               home_city          = CASE WHEN $20 THEN $21 ELSE home_city END,
               invoice_salutation = CASE WHEN $22 THEN $23 ELSE invoice_salutation END,
               invoice_first_name = CASE WHEN $24 THEN $25 ELSE invoice_first_name END,
               invoice_last_name  = CASE WHEN $26 THEN $27 ELSE invoice_last_name END,
               invoice_addon      = CASE WHEN $28 THEN $29 ELSE invoice_addon END,
               invoice_street     = CASE WHEN $30 THEN $31 ELSE invoice_street END,
               invoice_zip        = CASE WHEN $32 THEN $33 ELSE invoice_zip END,
               invoice_city       = CASE WHEN $34 THEN $35 ELSE invoice_city END,
               home_country       = CASE WHEN $36 THEN $37 ELSE home_country END,
               invoice_country    = CASE WHEN $38 THEN $39 ELSE invoice_country END,
               phone              = CASE WHEN $40 THEN $41 ELSE phone END,
               warning_remark     = CASE WHEN $42 THEN $43 ELSE warning_remark END,
               draft              = $44
           WHERE id = $1"#,
        id,
        body.salutation.is_some(),
        body.salutation.flatten() as Option<Salutation>,
        body.first_name.is_some(),
        blank_to_null(&body.first_name),
        body.last_name.is_some(),
        blank_to_null(&body.last_name),
        body.second_salutation.is_some(),
        body.second_salutation.flatten() as Option<Salutation>,
        body.second_first_name.is_some(),
        blank_to_null(&body.second_first_name),
        body.second_last_name.is_some(),
        blank_to_null(&body.second_last_name),
        body.home_addon.is_some(),
        blank_to_null(&body.home_addon),
        body.home_street.is_some(),
        blank_to_null(&body.home_street),
        body.home_zip.is_some(),
        blank_to_null(&body.home_zip),
        body.home_city.is_some(),
        blank_to_null(&body.home_city),
        body.invoice_salutation.is_some(),
        body.invoice_salutation.flatten() as Option<Salutation>,
        body.invoice_first_name.is_some(),
        blank_to_null(&body.invoice_first_name),
        body.invoice_last_name.is_some(),
        blank_to_null(&body.invoice_last_name),
        body.invoice_addon.is_some(),
        blank_to_null(&body.invoice_addon),
        body.invoice_street.is_some(),
        blank_to_null(&body.invoice_street),
        body.invoice_zip.is_some(),
        blank_to_null(&body.invoice_zip),
        body.invoice_city.is_some(),
        blank_to_null(&body.invoice_city),
        home_country.is_some(),
        home_country.flatten(),
        invoice_country.is_some(),
        invoice_country.flatten(),
        phone.is_some(),
        blank_to_null(&phone),
        body.warning_remark.is_some(),
        blank_to_null(&body.warning_remark),
        draft,
    )
    .execute(&mut *transaction)
    .await?;

    transaction.commit().await?;
    Ok(Json(load(&state.pool, id).await?))
}

/// An emptied text field means "no value", not an empty string.
fn blank_to_null(value: &Option<Option<String>>) -> Option<String> {
    value
        .clone()
        .flatten()
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}

#[utoipa::path(
    post,
    path = "/api/customers/{id}/archive",
    operation_id = "archiveCustomer",
    tag = "customers",
    params(("id" = i64, Path,)),
    responses((status = 200, body = Customer))
)]
pub async fn archive(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Customer>> {
    set_archived(&state, id, true).await
}

#[utoipa::path(
    post,
    path = "/api/customers/{id}/unarchive",
    operation_id = "unarchiveCustomer",
    tag = "customers",
    params(("id" = i64, Path,)),
    responses((status = 200, body = Customer))
)]
pub async fn unarchive(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Customer>> {
    set_archived(&state, id, false).await
}

async fn set_archived(state: &AppState, id: i64, archived: bool) -> AppResult<Json<Customer>> {
    let updated = sqlx::query!(
        "UPDATE customer SET archived = $2 WHERE id = $1",
        id,
        archived
    )
    .execute(&state.pool)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(Json(load(&state.pool, id).await?))
}

#[utoipa::path(
    post,
    path = "/api/customers/{id}/emails",
    operation_id = "addCustomerEmail",
    tag = "customers",
    params(("id" = i64, Path,)),
    request_body = CreateCustomerEmail,
    responses((status = 200, body = Customer), (status = 422))
)]
pub async fn add_email(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<CreateCustomerEmail>,
) -> AppResult<Json<Customer>> {
    let email = validate_email("email", &body.email)?;
    sqlx::query!(
        "INSERT INTO customer_email (customer_id, email, email_type) VALUES ($1, $2, $3)",
        id,
        email,
        body.email_type as EmailType,
    )
    .execute(&state.pool)
    .await?;
    Ok(Json(load(&state.pool, id).await?))
}

#[utoipa::path(
    patch,
    path = "/api/customer-emails/{id}",
    operation_id = "patchCustomerEmail",
    tag = "customers",
    params(("id" = i64, Path,)),
    request_body = PatchCustomerEmail,
    responses((status = 200, body = Customer), (status = 404), (status = 422))
)]
pub async fn patch_email(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<PatchCustomerEmail>,
) -> AppResult<Json<Customer>> {
    let email = match &body.email {
        Some(raw) => Some(validate_email("email", raw)?),
        None => None,
    };
    let customer_id: i64 = sqlx::query_scalar!(
        r#"UPDATE customer_email SET
               email      = COALESCE($2, email),
               email_type = COALESCE($3, email_type)
           WHERE id = $1
           RETURNING customer_id"#,
        id,
        email,
        body.email_type as Option<EmailType>,
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    Ok(Json(load(&state.pool, customer_id).await?))
}

#[utoipa::path(
    delete,
    path = "/api/customer-emails/{id}",
    operation_id = "deleteCustomerEmail",
    tag = "customers",
    params(("id" = i64, Path,)),
    responses((status = 204), (status = 404))
)]
pub async fn delete_email(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    let deleted = sqlx::query!("DELETE FROM customer_email WHERE id = $1", id)
        .execute(&state.pool)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Loads a customer with its email addresses and the derived display fields.
pub async fn load(pool: &sqlx::PgPool, id: i64) -> AppResult<Customer> {
    let row = sqlx::query!(
        r#"SELECT id, salutation AS "salutation?: Salutation", first_name, last_name,
                  second_salutation AS "second_salutation?: Salutation", second_first_name,
                  second_last_name, home_addon, home_street, home_zip, home_city, home_country,
                  invoice_salutation AS "invoice_salutation?: Salutation", invoice_first_name,
                  invoice_last_name, invoice_addon, invoice_street, invoice_zip, invoice_city,
                  invoice_country,
                  phone, warning_remark, archived, draft, has_invoice_address, has_second_name,
                  created_at, updated_at
           FROM customer WHERE id = $1"#,
        id,
    )
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;

    let emails = sqlx::query!(
        r#"SELECT id, email, email_type AS "email_type: EmailType"
           FROM customer_email WHERE customer_id = $1 ORDER BY id"#,
        id,
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|email| CustomerEmail {
        id: email.id,
        email: email.email,
        email_type: email.email_type,
    })
    .collect();

    let missing_fields = crate::domain::draft::missing_fields(&[
        ("salutation", row.salutation.is_some()),
        ("last_name", row.last_name.is_some()),
        ("home_street", row.home_street.is_some()),
        ("home_zip", row.home_zip.is_some()),
        ("home_city", row.home_city.is_some()),
    ])
    .into_iter()
    .map(str::to_owned)
    .collect();

    Ok(Customer {
        id: row.id,
        salutation: row.salutation,
        first_name: row.first_name,
        last_name: row.last_name,
        second_salutation: row.second_salutation,
        second_first_name: row.second_first_name,
        second_last_name: row.second_last_name,
        home_addon: row.home_addon,
        home_street: row.home_street,
        home_zip: row.home_zip,
        home_city: row.home_city,
        home_country: row.home_country,
        invoice_salutation: row.invoice_salutation,
        invoice_first_name: row.invoice_first_name,
        invoice_last_name: row.invoice_last_name,
        invoice_addon: row.invoice_addon,
        invoice_street: row.invoice_street,
        invoice_zip: row.invoice_zip,
        invoice_city: row.invoice_city,
        invoice_country: row.invoice_country,
        phone_display: row.phone.as_deref().map(display_phone),
        phone: row.phone,
        warning_remark: row.warning_remark,
        archived: row.archived,
        draft: row.draft,
        missing_fields,
        has_invoice_address: row.has_invoice_address,
        has_second_name: row.has_second_name,
        emails,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/customers", get(list).post(create))
        .route("/customers/{id}", get(detail).patch(patch))
        .route("/customers/{id}/archive", post(archive))
        .route("/customers/{id}/unarchive", post(unarchive))
        .route("/customers/{id}/emails", post(add_email))
        .route(
            "/customer-emails/{id}",
            axum::routing::patch(patch_email).delete(delete_email),
        )
}
