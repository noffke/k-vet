//! Entry point: reads the configuration, prepares the database and serves the app.
//!
//! Usage:
//!   k-vet-backend [CONFIG_PATH]      serve (config from arg, `KVET_CONFIG`, or /etc/k-vet)
//!   k-vet-backend --hash-password    print an argon2id hash for `[auth] password_hash`
//!   k-vet-backend --reset-database   drop the schema and re-migrate (needs KVET_ALLOW_DB_RESET=1)

use std::io::{IsTerminal, Write};
use std::process::ExitCode;
use std::sync::Arc;

use k_vet_backend::config::Config;
use k_vet_backend::{AppState, auth, build_app, jobs, run_migrations};
use sqlx::postgres::PgPoolOptions;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::time::ChronoLocal;

/// Human readable local timestamps — what the vet correlates with real-world events (R16).
const LOG_TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S%.3f";

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            // Errors before the subscriber is up would otherwise vanish.
            let _ = writeln!(std::io::stderr(), "k-vet: {message}");
            tracing::error!("{message}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let mut config_path_argument = None;
    let mut hash_password = false;
    let mut reset_database = false;

    for argument in std::env::args().skip(1) {
        match argument.as_str() {
            "--hash-password" => hash_password = true,
            "--reset-database" => reset_database = true,
            "--help" | "-h" => {
                print_usage();
                return Ok(());
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown argument `{other}` (try --help)"));
            }
            other => config_path_argument = Some(other.to_owned()),
        }
    }

    if hash_password {
        return print_password_hash();
    }

    let path = Config::resolve_path(config_path_argument);
    let config = Config::load(&path).map_err(|error| error.to_string())?;
    let config = Arc::new(config);

    init_tracing(&config.server.log_level)?;

    let pool = PgPoolOptions::new()
        .max_connections(config.database.max_connections)
        .connect(&config.database.url)
        .await
        .map_err(|error| format!("cannot connect to the database: {error}"))?;

    if reset_database {
        return reset(&pool).await;
    }

    tokio::fs::create_dir_all(&config.storage.attachments_dir)
        .await
        .map_err(|error| {
            format!(
                "cannot create attachments directory {}: {error}",
                config.storage.attachments_dir.display()
            )
        })?;

    run_migrations(&pool)
        .await
        .map_err(|error| error.to_string())?;

    let socket = config.listen_socket().map_err(|error| error.to_string())?;
    let state = AppState::new(pool, Arc::clone(&config)).map_err(|error| error.to_string())?;
    // Nightly maintenance runs inside the same process; there is no cron on the appliance.
    let maintenance = jobs::spawn(state.clone());
    let app = build_app(state).await.map_err(|error| error.to_string())?;

    let listener = tokio::net::TcpListener::bind(socket)
        .await
        .map_err(|error| format!("cannot bind {socket}: {error}"))?;

    tracing::info!(%socket, config = %path.display(), "k-vet backend started");

    let served = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|error| format!("server error: {error}"));

    maintenance.abort();
    served
}

fn print_usage() {
    let usage = "\
k-vet backend

USAGE:
    k-vet-backend [CONFIG_PATH]      serve the application
    k-vet-backend --hash-password    print an argon2id hash for the config file
    k-vet-backend --reset-database   drop the schema and re-migrate (KVET_ALLOW_DB_RESET=1)

The configuration path is taken from the argument, then KVET_CONFIG, then /etc/k-vet/config.toml.
";
    let _ = write!(std::io::stdout(), "{usage}");
}

fn print_password_hash() -> Result<(), String> {
    if std::io::stdin().is_terminal() {
        let _ = write!(std::io::stderr(), "password: ");
        let _ = std::io::stderr().flush();
    }
    let mut password = String::new();
    std::io::stdin()
        .read_line(&mut password)
        .map_err(|error| format!("cannot read the password: {error}"))?;
    let password = password.trim_end_matches(['\n', '\r']);
    if password.is_empty() {
        return Err("the password must not be empty".to_owned());
    }
    let hash = auth::hash_password(password).map_err(|error| error.to_string())?;
    let _ = writeln!(std::io::stdout(), "{hash}");
    Ok(())
}

/// Development/test helper: wipes the schema, then re-applies the migrations.
async fn reset(pool: &sqlx::PgPool) -> Result<(), String> {
    if std::env::var("KVET_ALLOW_DB_RESET").unwrap_or_default() != "1" {
        return Err("--reset-database requires KVET_ALLOW_DB_RESET=1".to_owned());
    }
    for statement in [
        "DROP SCHEMA IF EXISTS tower_sessions CASCADE",
        "DROP SCHEMA public CASCADE",
        "CREATE SCHEMA public",
    ] {
        sqlx::query(statement)
            .execute(pool)
            .await
            .map_err(|error| format!("cannot reset the database ({statement}): {error}"))?;
    }
    run_migrations(pool)
        .await
        .map_err(|error| error.to_string())?;
    tracing::info!("database reset and migrated");
    Ok(())
}

fn init_tracing(log_level: &str) -> Result<(), String> {
    let filter = EnvFilter::try_from_default_env().or_else(|_| {
        EnvFilter::try_new(format!("{log_level},sqlx::query=warn,tower_sessions=warn"))
    });
    let filter = filter.map_err(|error| format!("invalid log level `{log_level}`: {error}"))?;

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_timer(ChronoLocal::new(LOG_TIME_FORMAT.to_owned()))
        .compact()
        .try_init()
        .map_err(|error| format!("cannot install the log subscriber: {error}"))
}

async fn shutdown_signal() {
    let interrupt = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => tracing::error!(%error, "cannot listen for SIGTERM"),
        }
    };
    tokio::select! {
        () = interrupt => {},
        () = terminate => {},
    }
    tracing::info!("shutting down");
}
