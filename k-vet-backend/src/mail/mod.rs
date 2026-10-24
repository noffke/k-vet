//! Outbound invoice email: minijinja template, lettre transport (research R6, R17).
//!
//! The templates are plain-text UTF-8 files in the templates folder, listed with a name in the
//! configuration (`[[invoice.email_templates]]`); the vet picks one when releasing or sending.
//! The first line of a file is the subject template, then a blank line, then the body. A file
//! is read when a mail is rendered, so an edit takes effect with the next mail and a missing one
//! is a state the app shows rather than a reason not to start. Invoice emails are always German
//! (FR-030).

use lettre::message::header::ContentType;
use lettre::message::{Attachment, Mailbox, MessageBuilder, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use std::path::PathBuf;

use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use serde::Serialize;

use crate::config::{Config, EmailTemplateConfig, SmtpTls};
use crate::domain::enums::Salutation;
use crate::error::{AppError, AppResult};

/// Template variables of the invoice email.
#[derive(Debug, Clone, Serialize)]
pub struct InvoiceMail {
    /// Ready-made salutation line ("Sehr geehrte Frau Mustermann").
    pub greeting: String,
    pub salutation: String,
    pub first_name: String,
    pub last_name: String,
    pub second_salutation: String,
    pub second_first_name: String,
    pub second_last_name: String,
    pub invoice_number: String,
    /// German date, e.g. `04.05.2026`.
    pub invoice_date: String,
    /// German amount, e.g. `38,62 €`.
    pub invoice_total: String,
    pub practice_name: String,
    pub patients: Vec<String>,
}

/// Builds the German salutation line, including the two-name household case (R17).
pub fn greeting(
    salutation: Option<Salutation>,
    last_name: &str,
    second_salutation: Option<Salutation>,
    second_last_name: &str,
) -> String {
    let one = |salutation: Option<Salutation>, name: &str| match salutation {
        Some(Salutation::Frau) => format!("Sehr geehrte Frau {name}"),
        Some(Salutation::Herr) => format!("Sehr geehrter Herr {name}"),
        Some(Salutation::Familie) => format!("Sehr geehrte Familie {name}"),
        None => "Sehr geehrte Damen und Herren".to_owned(),
    };

    let first = one(salutation, last_name);
    match (second_salutation, second_last_name) {
        (Some(second), name) if !name.is_empty() => {
            // "Sehr geehrte Frau X, sehr geehrter Herr Y"
            // The second salutation continues the sentence, so it starts lower case.
            let second = one(Some(second), name);
            let second = match second.split_once(' ') {
                Some((head, tail)) => format!("{} {tail}", head.to_lowercase()),
                None => second,
            };
            format!("{first}, {second}")
        }
        _ => first,
    }
}

/// One outgoing invoice email.
pub struct OutgoingInvoice<'a> {
    pub recipients: &'a [String],
    /// Global CC addresses from the practice settings (e.g. the bookkeeper).
    pub cc: &'a [String],
    pub bcc: &'a [String],
    pub subject: &'a str,
    pub body: &'a str,
    pub pdf_name: &'a str,
    pub pdf: Vec<u8>,
}

/// Why a mail did not go out, as the queue keeps it on the row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailFailure {
    /// Final: retrying cannot help — the mail server refused for good, or k-vet could not build
    /// the mail (an address it cannot parse, no PDF). The queue stops at once.
    pub refused: bool,
    pub detail: String,
}

impl MailFailure {
    /// A failure on k-vet's side, before any mail server was asked.
    pub fn refused(detail: impl Into<String>) -> Self {
        Self {
            refused: true,
            detail: detail.into(),
        }
    }
}

/// A configured template, and whether its file is there to be read right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateState {
    pub name: String,
    pub file: String,
    pub available: bool,
}

/// Why a template cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TemplateProblem {
    /// No template of that name is configured — or no longer, since a mail was queued with it.
    #[error("the email template \"{0}\" is not configured")]
    Unknown(String),
    /// Configured, but its file is not in the templates folder (or cannot be read).
    #[error("the file of the email template \"{name}\" is missing: {}", path.display())]
    Missing { name: String, path: PathBuf },
}

/// A mail rendered for sending, with the name of the template it was written with — which the
/// invoice records once the mail server took it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedMail {
    pub template: String,
    pub subject: String,
    pub body: String,
}

/// Sends invoice emails through the configured SMTP server.
pub struct Mailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
    templates: Vec<EmailTemplateConfig>,
    templates_dir: PathBuf,
}

impl Mailer {
    /// Builds the transport from the configuration. Fails fast on an unusable setup.
    pub fn from_config(config: &Config) -> AppResult<Self> {
        let builder = match config.mail.smtp_tls {
            SmtpTls::Starttls => {
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.mail.smtp_host)
                    .map_err(|error| AppError::internal("configuring the SMTP transport", error))?
            }
            SmtpTls::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(&config.mail.smtp_host)
                .map_err(|error| AppError::internal("configuring the SMTP transport", error))?,
            SmtpTls::None => {
                AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&config.mail.smtp_host)
            }
        };

        let builder = builder.port(config.mail.smtp_port);
        let builder = if config.mail.smtp_username.is_empty() {
            builder
        } else {
            builder.credentials(Credentials::new(
                config.mail.smtp_username.clone(),
                config.mail.smtp_password.clone(),
            ))
        };

        let from = format_mailbox(&config.mail.from_name, &config.mail.from_address)?;

        Ok(Self {
            transport: builder.build(),
            from,
            templates: config.invoice.email_templates.clone(),
            templates_dir: config.invoice.templates_dir.clone(),
        })
    }

    /// The configured templates in the order offered, each with whether its file is there.
    /// Looked at on every call: a file put in place shows up without a restart.
    pub fn templates(&self) -> Vec<TemplateState> {
        self.templates
            .iter()
            .map(|template| TemplateState {
                name: template.name.clone(),
                file: template.file.clone(),
                available: is_readable_file(&self.templates_dir.join(&template.file)),
            })
            .collect()
    }

    /// The template of that name — or, for a mail queued before templates could be chosen, the
    /// first one — as its configured name and its file, which must be in place.
    pub fn resolve(&self, name: Option<&str>) -> Result<(String, PathBuf), TemplateProblem> {
        let template = match name {
            Some(name) => self
                .templates
                .iter()
                .find(|template| template.name == name)
                .ok_or_else(|| TemplateProblem::Unknown(name.to_owned()))?,
            None => self
                .templates
                .first()
                .ok_or_else(|| TemplateProblem::Unknown(String::new()))?,
        };
        let path = self.templates_dir.join(&template.file);
        if is_readable_file(&path) {
            Ok((template.name.clone(), path))
        } else {
            Err(TemplateProblem::Missing {
                name: template.name.clone(),
                path,
            })
        }
    }

    /// Renders subject and body from the named template, read from its file now. Anything that
    /// keeps it from rendering — not configured, no file, a broken template — fails the mail
    /// for good: trying again later would not change it.
    pub fn render(
        &self,
        context: &InvoiceMail,
        name: Option<&str>,
    ) -> Result<RenderedMail, MailFailure> {
        let (template_name, path) = self
            .resolve(name)
            .map_err(|problem| MailFailure::refused(problem.to_string()))?;
        let template = std::fs::read_to_string(&path).map_err(|error| {
            MailFailure::refused(format!(
                "the email template {} cannot be read: {error}",
                path.display()
            ))
        })?;
        let (subject, body) = render_template(&template, context).map_err(|error| {
            MailFailure::refused(format!(
                "the email template {} does not render: {error}",
                path.display()
            ))
        })?;
        Ok(RenderedMail {
            template: template_name,
            subject,
            body,
        })
    }

    /// Builds the message: plain-text UTF-8 body plus the invoice PDF as attachment.
    pub fn build_message(&self, outgoing: OutgoingInvoice<'_>) -> AppResult<Message> {
        if outgoing.recipients.is_empty() {
            return Err(AppError::field("recipient_emails", "field.required"));
        }

        let mut builder: MessageBuilder = Message::builder().from(self.from.clone());
        for address in outgoing.recipients {
            builder = builder.to(parse_mailbox(address)?);
        }
        for address in outgoing.cc {
            builder = builder.cc(parse_mailbox(address)?);
        }
        for address in outgoing.bcc {
            builder = builder.bcc(parse_mailbox(address)?);
        }

        // lettre encodes the non-ASCII subject as an RFC 2047 encoded word and the body
        // with a content-transfer-encoding that survives every mail server (R17).
        builder
            .subject(outgoing.subject)
            .multipart(
                MultiPart::mixed()
                    .singlepart(
                        SinglePart::builder()
                            .header(ContentType::TEXT_PLAIN)
                            .body(outgoing.body.to_owned()),
                    )
                    .singlepart(Attachment::new(outgoing.pdf_name.to_owned()).body(
                        outgoing.pdf,
                        ContentType::parse("application/pdf").map_err(|error| {
                            AppError::internal("building the PDF attachment", error)
                        })?,
                    )),
            )
            .map_err(|error| AppError::internal("building the invoice email", error))
    }

    /// Hands a prepared message to the mail server. Only the mail queue calls this — no request
    /// waits on a mail server (`jobs::send_queued_mail`).
    pub async fn send(&self, message: Message) -> Result<(), MailFailure> {
        self.transport
            .send(message)
            .await
            .map(|_| ())
            .map_err(|error| MailFailure {
                // A 5xx reply is the mail server's final word; an unreachable server, a timeout
                // or a 4xx reply may pass, and the queue tries again.
                refused: error.is_permanent(),
                // The transport's own words: they name the host, or carry the server's reply
                // ("permanent error (550): no such user").
                detail: error.to_string(),
            })
    }
}

/// A regular file this process can open.
fn is_readable_file(path: &std::path::Path) -> bool {
    std::fs::File::open(path)
        .and_then(|file| file.metadata())
        .is_ok_and(|metadata| metadata.is_file())
}

/// Splits the template into subject and body and renders both with minijinja.
fn render_template(template: &str, context: &InvoiceMail) -> AppResult<(String, String)> {
    let (subject_template, body_template) = template
        .split_once('\n')
        .ok_or_else(|| AppError::Internal("the email template has no body".to_owned()))?;

    let mut environment = minijinja::Environment::new();
    environment
        .add_template("subject", subject_template.trim())
        .map_err(|error| AppError::internal("parsing the email subject template", error))?;
    environment
        .add_template("body", body_template.trim_start_matches('\n'))
        .map_err(|error| AppError::internal("parsing the email body template", error))?;

    let subject = environment
        .get_template("subject")
        .and_then(|template| template.render(context))
        .map_err(|error| AppError::internal("rendering the email subject", error))?;
    let body = environment
        .get_template("body")
        .and_then(|template| template.render(context))
        .map_err(|error| AppError::internal("rendering the email body", error))?;

    let subject = subject.trim().to_owned();
    // A stray leading blank line in an operator's template would otherwise send a mail with no
    // subject at all, which most clients file as spam and nobody notices until a customer asks.
    if subject.is_empty() {
        return Err(AppError::Internal(
            "the email template's first line is the subject and must not be empty".to_owned(),
        ));
    }

    Ok((subject, body))
}

fn format_mailbox(name: &str, address: &str) -> AppResult<Mailbox> {
    let raw = if name.trim().is_empty() {
        address.to_owned()
    } else {
        format!("{name} <{address}>")
    };
    raw.parse()
        .map_err(|error| AppError::internal(format!("parsing the sender address {raw}"), error))
}

fn parse_mailbox(address: &str) -> AppResult<Mailbox> {
    address
        .trim()
        .parse()
        .map_err(|_| AppError::field("recipient_emails", "value.invalidEmail"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> InvoiceMail {
        InvoiceMail {
            greeting: greeting(Some(Salutation::Frau), "Müller", None, ""),
            salutation: "Frau".to_owned(),
            first_name: "Erika".to_owned(),
            last_name: "Müller".to_owned(),
            second_salutation: String::new(),
            second_first_name: String::new(),
            second_last_name: String::new(),
            invoice_number: "2026-0042".to_owned(),
            invoice_date: "04.05.2026".to_owned(),
            invoice_total: "38,62 €".to_owned(),
            practice_name: "Tierärztin Dr. Musterfrau".to_owned(),
            patients: vec!["Bello".to_owned(), "Minka".to_owned()],
        }
    }

    #[test]
    fn greeting_covers_all_salutations() {
        assert_eq!(
            greeting(Some(Salutation::Frau), "Müller", None, ""),
            "Sehr geehrte Frau Müller"
        );
        assert_eq!(
            greeting(Some(Salutation::Herr), "Schmidt", None, ""),
            "Sehr geehrter Herr Schmidt"
        );
        assert_eq!(
            greeting(Some(Salutation::Familie), "Weber", None, ""),
            "Sehr geehrte Familie Weber"
        );
        assert_eq!(
            greeting(None, "", None, ""),
            "Sehr geehrte Damen und Herren"
        );
    }

    #[test]
    fn greeting_addresses_two_name_households() {
        let both = greeting(
            Some(Salutation::Frau),
            "Müller",
            Some(Salutation::Herr),
            "Schmidt",
        );
        assert_eq!(both, "Sehr geehrte Frau Müller, sehr geehrter Herr Schmidt");
    }

    /// The template the container seeds as "Standard".
    const SEEDED_TEMPLATE: &str = include_str!("../../templates/invoice-email.txt");

    #[test]
    fn template_first_line_is_the_subject() {
        let (subject, body) = render_template(SEEDED_TEMPLATE, &context()).expect("renders");
        assert_eq!(
            subject,
            "Ihre Rechnung 2026-0042 – Tierärztin Dr. Musterfrau"
        );
        assert!(
            body.starts_with("Sehr geehrte Frau Müller,"),
            "body was: {body}"
        );
        assert!(body.contains("38,62 €"));
        assert!(body.contains("Bello, Minka"), "patients are listed: {body}");
        assert!(body.contains("Tierärztin Dr. Musterfrau"));
    }

    // Building the transport spawns lettre's connection pool, so a runtime is required.
    #[tokio::test]
    async fn umlauts_survive_rendering_and_message_building() {
        let config = Config::parse(
            r#"
            [server]
            [auth]
            username = "tierarzt"
            password_hash = "$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA"
            [database]
            url = "postgres://unused/unused"
            [storage]
            attachments_dir = "/tmp"
            [mail]
            smtp_host = "127.0.0.1"
            smtp_tls = "none"
            from_address = "praxis@example.com"
            from_name = "Tierärztin Dr. Musterfrau"
            [invoice]
            number_pattern = "{year}-{counter:4}"
            [travel_expenses]
            rate_per_double_km = "3.50"
            minimum = "13.00"
            "#,
            "test.toml",
        )
        .expect("test config");
        let mut config = config;
        config.invoice.templates_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates");
        let mailer = Mailer::from_config(&config).expect("mailer builds");

        let RenderedMail {
            template,
            subject,
            body,
        } = mailer.render(&context(), None).expect("renders");
        assert_eq!(
            template, "Standard",
            "a mail without a choice uses the first"
        );

        let message = mailer
            .build_message(OutgoingInvoice {
                recipients: &["kundin@example.com".to_owned()],
                cc: &["buchhaltung@example.com".to_owned()],
                bcc: &[],
                // An operator-edited subject template may well contain umlauts — the
                // header is where naive implementations break (R17).
                subject: &format!("{subject} für Bello (Tierärztin Musterfrau)"),
                body: &body,
                pdf_name: "Rechnung-2026-0042.pdf",
                pdf: b"%PDF-1.7 fake".to_vec(),
            })
            .expect("message builds");

        let raw = String::from_utf8_lossy(&message.formatted()).to_string();
        // A non-ASCII subject travels as RFC 2047 encoded words, never as raw bytes.
        let subject_line = raw
            .lines()
            .find(|line| line.starts_with("Subject:"))
            .unwrap_or_default();
        assert!(
            subject_line.contains("=?utf-8?") || subject_line.contains("=?UTF-8?"),
            "subject header was not encoded: {subject_line}"
        );
        assert!(
            subject_line.is_ascii(),
            "the subject header must not carry raw non-ASCII bytes: {subject_line}"
        );
        assert!(raw.contains("charset=utf-8"), "body charset missing: {raw}");
        assert!(raw.contains("Cc: buchhaltung@example.com"));
        assert!(raw.contains("Rechnung-2026-0042.pdf"));

        // The body's umlauts survive as quoted-printable (or as literal UTF-8).
        let umlauts_intact = raw.contains("Sehr geehrte Frau M=C3=BCller")
            || raw.contains("Sehr geehrte Frau Müller");
        assert!(umlauts_intact, "body encoding lost the umlauts: {raw}");
        assert!(
            raw.contains("38,62 =E2=82=AC") || raw.contains("38,62 €"),
            "the euro sign was mangled: {raw}"
        );
    }

    #[tokio::test]
    async fn a_recipient_is_required() {
        let config = Config::parse(
            r#"
            [server]
            [auth]
            username = "t"
            password_hash = "$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA"
            [database]
            url = "postgres://unused/unused"
            [storage]
            attachments_dir = "/tmp"
            [mail]
            smtp_host = "127.0.0.1"
            smtp_tls = "none"
            from_address = "praxis@example.com"
            [invoice]
            number_pattern = "{counter}"
            [travel_expenses]
            rate_per_double_km = "3.50"
            minimum = "13.00"
            "#,
            "test.toml",
        )
        .expect("test config");
        let mailer = Mailer::from_config(&config).expect("mailer builds");

        let error = mailer
            .build_message(OutgoingInvoice {
                recipients: &[],
                cc: &[],
                bcc: &[],
                subject: "s",
                body: "b",
                pdf_name: "x.pdf",
                pdf: Vec::new(),
            })
            .expect_err("no recipient");
        assert!(matches!(error, AppError::Validation(_)));
    }
}
