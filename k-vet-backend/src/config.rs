//! TOML configuration (research R13).
//!
//! Everything the operator sets outside the UI lives here: credentials, database,
//! SMTP, invoice number pattern, VAT choices and travel expense rates. The shipped
//! `config.example.toml` documents every field in de-DE and en-US.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use rust_decimal::Decimal;
use serde::Deserialize;

/// Environment variable holding the path of the configuration file.
pub const CONFIG_PATH_ENV: &str = "KVET_CONFIG";
/// Fallback path when neither an argument nor the environment names a file.
pub const DEFAULT_CONFIG_PATH: &str = "/etc/k-vet/config.toml";

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read configuration file {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot parse configuration file {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("invalid configuration: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub server: ServerConfig,
    pub auth: AuthConfig,
    pub database: DatabaseConfig,
    pub storage: StorageConfig,
    pub mail: MailConfig,
    pub invoice: InvoiceConfig,
    pub travel_expenses: TravelExpenseConfig,
    /// Optional: a `config.toml` written before this section existed still loads.
    #[serde(default)]
    pub pharmacy: PharmacyConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    #[serde(default = "default_listen_address")]
    pub listen_address: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    /// Directory holding the built frontend (`k-vet-web/dist`), served with SPA fallback.
    /// In development the Vite dev server serves the UI instead and this path is unused.
    #[serde(default = "default_web_dir")]
    pub web_dir: PathBuf,
    /// Names this instance in the interface when it is not the practice's real one.
    ///
    /// Empty or absent on production, so nothing is drawn. Anywhere else — the staging
    /// instance on the same Pi, a laptop — it is what stops an invoice being written in the
    /// wrong window, which is a mistake with no trace afterwards because the record does not
    /// exist in the database anyone looks at.
    #[serde(default)]
    pub environment_label: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthConfig {
    pub username: String,
    pub password_hash: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatabaseConfig {
    pub url: String,
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageConfig {
    pub attachments_dir: PathBuf,
    #[serde(default = "default_max_upload_mb")]
    pub max_upload_mb: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SmtpTls {
    /// Upgrade a plain connection with STARTTLS (typically port 587).
    Starttls,
    /// Implicit TLS from the first byte (typically port 465).
    Tls,
    /// No transport encryption — only sensible for a local relay.
    None,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MailConfig {
    pub smtp_host: String,
    #[serde(default = "default_smtp_port")]
    pub smtp_port: u16,
    #[serde(default = "default_smtp_tls")]
    pub smtp_tls: SmtpTls,
    #[serde(default)]
    pub smtp_username: String,
    #[serde(default)]
    pub smtp_password: String,
    pub from_address: String,
    #[serde(default)]
    pub from_name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvoiceConfig {
    pub number_pattern: String,
    #[serde(default = "default_currency")]
    pub currency: String,
    #[serde(default = "default_vat_rates")]
    pub vat_rates: Vec<Decimal>,
    /// ISO 3166-1 alpha-2 used when a customer or the practice has no country of its own.
    /// A business preference rather than an invariant, so it lives here and not as a column
    /// default — the database stores NULL and this fills in.
    #[serde(default = "default_country")]
    pub default_country: String,
    /// Days between the invoice date and the due date. Pinned onto each invoice at creation,
    /// so changing it never moves the due date of an invoice already issued.
    #[serde(default = "default_payment_terms_days")]
    pub payment_terms_days: i64,
    #[serde(default, deserialize_with = "empty_path_as_none")]
    pub typst_template: Option<PathBuf>,
    #[serde(default, deserialize_with = "empty_path_as_none")]
    pub email_template: Option<PathBuf>,
}

/// Deviations from the statutory drug price, decided per practice.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PharmacyConfig {
    /// Never price a Teilmenge below its share of the whole pack.
    ///
    /// Off by default, and deliberately so: it can exceed the statutory maximum. § 10 Abs. 1
    /// AMPreisV permits *höchstens* the § 4 surcharge of 100 % on the pro-rata listed price, and
    /// the floor can go well past that — a 10 ml human preparation listed at 1,00 EUR gives a 5 ml
    /// Teilmenge of 1,00 EUR under § 4, which the floor lifts to 4,57 EUR, a 357 % surcharge. Only
    /// switch it on where the practice has decided to.
    ///
    /// It only ever bites on human preparations: the veterinary bands stay below 100 %, so § 4
    /// already exceeds the pro-rata share there.
    #[serde(default)]
    pub subset_never_below_proportional: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TravelExpenseConfig {
    pub rate_per_double_km: Decimal,
    pub minimum: Decimal,
}

impl Config {
    /// Reads and validates the configuration file at `path`.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let raw = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        Self::parse(&raw, path)
    }

    /// Parses and validates configuration text. `path` is only used for error messages.
    pub fn parse(raw: &str, path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let config: Config = toml::from_str(raw).map_err(|source| ConfigError::Parse {
            path: path.as_ref().to_path_buf(),
            source,
        })?;
        config.validate()?;
        Ok(config)
    }

    /// Resolves the configuration path: explicit argument, then `KVET_CONFIG`,
    /// then [`DEFAULT_CONFIG_PATH`].
    pub fn resolve_path(argument: Option<String>) -> PathBuf {
        if let Some(arg) = argument {
            return PathBuf::from(arg);
        }
        match std::env::var(CONFIG_PATH_ENV) {
            Ok(value) if !value.trim().is_empty() => PathBuf::from(value),
            _ => PathBuf::from(DEFAULT_CONFIG_PATH),
        }
    }

    /// Checks the invariants that are cheap to verify without touching the database.
    /// The invoice number pattern itself is validated by
    /// [`crate::domain::invoice_number::validate_pattern`] at startup.
    fn validate(&self) -> Result<(), ConfigError> {
        if self.auth.username.trim().is_empty() {
            return Err(ConfigError::Invalid(
                "auth.username must not be empty".into(),
            ));
        }
        if !self.auth.password_hash.starts_with("$argon2") {
            return Err(ConfigError::Invalid(
                "auth.password_hash must be an argon2 hash (starting with `$argon2`)".into(),
            ));
        }
        if self.database.url.trim().is_empty() {
            return Err(ConfigError::Invalid(
                "database.url must not be empty".into(),
            ));
        }
        if self.database.max_connections == 0 {
            return Err(ConfigError::Invalid(
                "database.max_connections must be greater than zero".into(),
            ));
        }
        if self.invoice.currency.trim().len() != 3 {
            return Err(ConfigError::Invalid(
                "invoice.currency must be a three-letter ISO 4217 code".into(),
            ));
        }
        if rust_iso3166::from_alpha2(&self.invoice.default_country.to_uppercase()).is_none() {
            return Err(ConfigError::Invalid(format!(
                "invoice.default_country must be an ISO 3166-1 alpha-2 country code, got `{}`",
                self.invoice.default_country,
            )));
        }
        if self.invoice.payment_terms_days < 0 {
            return Err(ConfigError::Invalid(
                "invoice.payment_terms_days must not be negative".into(),
            ));
        }
        if self.invoice.vat_rates.is_empty() {
            return Err(ConfigError::Invalid(
                "invoice.vat_rates must list at least one rate".into(),
            ));
        }
        if self
            .invoice
            .vat_rates
            .iter()
            .any(|rate| rate.is_sign_negative())
        {
            return Err(ConfigError::Invalid(
                "invoice.vat_rates must not contain negative rates".into(),
            ));
        }
        if self.travel_expenses.rate_per_double_km.is_sign_negative()
            || self.travel_expenses.minimum.is_sign_negative()
        {
            return Err(ConfigError::Invalid(
                "travel_expenses rates must not be negative".into(),
            ));
        }
        if self.storage.max_upload_mb == 0 {
            return Err(ConfigError::Invalid(
                "storage.max_upload_mb must be greater than zero".into(),
            ));
        }
        Ok(())
    }

    /// The socket the HTTP server binds to.
    pub fn listen_socket(&self) -> Result<SocketAddr, ConfigError> {
        let raw = format!("{}:{}", self.server.listen_address, self.server.port);
        raw.parse().map_err(|_| {
            ConfigError::Invalid(format!(
                "server.listen_address/port is not a valid socket: {raw}"
            ))
        })
    }

    /// Maximum accepted upload size in bytes.
    pub fn max_upload_bytes(&self) -> usize {
        usize::try_from(self.storage.max_upload_mb.saturating_mul(1024 * 1024))
            .unwrap_or(usize::MAX)
    }
}

fn default_listen_address() -> String {
    "127.0.0.1".to_owned()
}
fn default_port() -> u16 {
    8080
}
fn default_base_url() -> String {
    "http://localhost:8080".to_owned()
}
fn default_log_level() -> String {
    "info".to_owned()
}
fn default_web_dir() -> PathBuf {
    PathBuf::from("/usr/share/k-vet/web")
}
fn default_max_connections() -> u32 {
    5
}
fn default_max_upload_mb() -> u64 {
    25
}
fn default_smtp_port() -> u16 {
    587
}
fn default_smtp_tls() -> SmtpTls {
    SmtpTls::Starttls
}
fn default_country() -> String {
    "DE".to_owned()
}

fn default_payment_terms_days() -> i64 {
    14
}

fn default_currency() -> String {
    "EUR".to_owned()
}
fn default_vat_rates() -> Vec<Decimal> {
    vec![Decimal::from(19), Decimal::from(7)]
}

fn empty_path_as_none<'de, D>(deserializer: D) -> Result<Option<PathBuf>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(deserializer)?;
    Ok(raw
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"
        [auth]
        username = "tierarzt"
        password_hash = "$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA"
        [server]
        [database]
        url = "postgres://kvet:kvet@localhost/kvet"
        [storage]
        attachments_dir = "/var/lib/k-vet/attachments"
        [mail]
        smtp_host = "smtp.example.com"
        from_address = "praxis@example.com"
        [invoice]
        number_pattern = "{year}-{counter:4}"
        [travel_expenses]
        rate_per_double_km = "3.50"
        minimum = "13.00"
    "#;

    fn parse(raw: &str) -> Result<Config, ConfigError> {
        Config::parse(raw, "test.toml")
    }

    #[test]
    fn parses_minimal_config_and_applies_defaults() {
        let config = parse(MINIMAL).expect("minimal config parses");
        assert_eq!(config.server.listen_address, "127.0.0.1");
        assert_eq!(config.server.port, 8080);
        assert_eq!(config.server.log_level, "info");
        assert_eq!(config.server.web_dir, PathBuf::from("/usr/share/k-vet/web"));
        assert_eq!(config.database.max_connections, 5);
        assert_eq!(config.storage.max_upload_mb, 25);
        assert_eq!(config.mail.smtp_port, 587);
        assert_eq!(config.mail.smtp_tls, SmtpTls::Starttls);
        assert_eq!(config.invoice.currency, "EUR");
        assert_eq!(
            config.invoice.vat_rates,
            vec![Decimal::from(19), Decimal::from(7)]
        );
        assert_eq!(config.invoice.typst_template, None);
        assert_eq!(config.travel_expenses.minimum, Decimal::new(1300, 2));
    }

    #[test]
    fn shipped_example_config_is_valid() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../config.example.toml");
        let config = Config::load(path).expect("config.example.toml must stay loadable");
        assert_eq!(config.invoice.number_pattern, "{year}-{counter:4}");
        assert_eq!(
            config.travel_expenses.rate_per_double_km,
            Decimal::new(350, 2)
        );
        assert_eq!(config.invoice.vat_rates.len(), 2);
    }

    #[test]
    fn rejects_unknown_keys() {
        let raw = format!("{MINIMAL}\n[unknown_section]\nfoo = 1\n");
        let error = parse(&raw).expect_err("unknown sections are a typo, not a feature");
        assert!(matches!(error, ConfigError::Parse { .. }), "got {error:?}");
    }

    #[test]
    fn rejects_missing_required_field() {
        let raw = MINIMAL.replace("url = \"postgres://kvet:kvet@localhost/kvet\"", "");
        let error = parse(&raw).expect_err("database.url is required");
        assert!(matches!(error, ConfigError::Parse { .. }), "got {error:?}");
    }

    #[test]
    fn rejects_plaintext_password() {
        let raw = MINIMAL.replace(
            "password_hash = \"$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA\"",
            "password_hash = \"hunter2\"",
        );
        let error = parse(&raw).expect_err("plaintext passwords are rejected");
        assert!(matches!(error, ConfigError::Invalid(_)), "got {error:?}");
    }

    /// Adds extra keys to the `[invoice]` section of [`MINIMAL`].
    fn with_invoice_keys(extra: &str) -> String {
        const ANCHOR: &str = "number_pattern = \"{year}-{counter:4}\"";
        MINIMAL.replace(ANCHOR, &format!("{ANCHOR}\n{extra}"))
    }

    #[test]
    fn rejects_empty_vat_rates() {
        let raw = with_invoice_keys("vat_rates = []");
        let error = parse(&raw).expect_err("at least one VAT rate is required");
        assert!(matches!(error, ConfigError::Invalid(_)), "got {error:?}");
    }

    #[test]
    fn rejects_bad_currency() {
        let raw = with_invoice_keys("currency = \"Euro\"");
        let error = parse(&raw).expect_err("currency must be an ISO code");
        assert!(matches!(error, ConfigError::Invalid(_)), "got {error:?}");
    }

    #[test]
    fn empty_template_paths_become_none() {
        let raw = with_invoice_keys("typst_template = \"  \"\nemail_template = \"/tmp/mail.txt\"");
        let config = parse(&raw).expect("blank template path is allowed");
        assert_eq!(config.invoice.typst_template, None);
        assert_eq!(
            config.invoice.email_template,
            Some(PathBuf::from("/tmp/mail.txt"))
        );
    }

    #[test]
    fn the_frontend_directory_can_be_pointed_at_a_checkout() {
        let raw = MINIMAL.replace("[server]", "[server]\nweb_dir = \"/srv/k-vet/dist\"");
        let config = parse(&raw).expect("web_dir is a plain path");
        assert_eq!(config.server.web_dir, PathBuf::from("/srv/k-vet/dist"));
    }

    #[test]
    fn resolves_listen_socket() {
        let config = parse(MINIMAL).expect("config parses");
        let socket = config.listen_socket().expect("valid socket");
        assert_eq!(socket.port(), 8080);
    }

    #[test]
    fn resolve_path_prefers_argument_over_environment() {
        let path = Config::resolve_path(Some("/tmp/explicit.toml".to_owned()));
        assert_eq!(path, PathBuf::from("/tmp/explicit.toml"));
    }
}
