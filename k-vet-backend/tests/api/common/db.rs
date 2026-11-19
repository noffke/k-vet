//! A database per test, cloned from a template the migrations built once.
//!
//! `#[sqlx::test]` created an empty database for every test and ran all migrations into it —
//! the GOT catalogue import among them — which was most of a test's setup (about 1.5 s against
//! 0.2 s for a clone). Here the migrations run once, into a template named after their
//! checksums, and each test gets `CREATE DATABASE … TEMPLATE …`: the same pristine, migrated
//! and seeded state, its own, so tests still run in parallel. A new migration builds a new
//! template by itself; there is no schema file to keep in step.
//!
//! The database of a test that passed is dropped when its `TestDb` goes; one that failed is
//! kept for inspection, and the next run's sweep removes it.

use std::sync::atomic::{AtomicU32, Ordering};

use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{ConnectOptions, Connection, PgConnection, PgPool};
use tokio::sync::OnceCell;

const TEMPLATE_PREFIX: &str = "kvet_template_";
const TEST_PREFIX: &str = "kvet_test_";
/// Serialises building the template — across the tests of one process, and across processes.
const LOCK_KEY: i64 = 0x6b76_6574_7465_7374;

static TEMPLATE: OnceCell<String> = OnceCell::const_new();
static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A test's own database, cloned from the template. Dropped with it, unless the test failed.
pub struct TestDb {
    name: String,
    pool: PgPool,
}

impl TestDb {
    /// The pool on this test's database.
    pub fn pool(&self) -> PgPool {
        self.pool.clone()
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("test database kept for inspection: {}", self.name);
            return;
        }
        // Drop cannot await, and a runtime cannot be blocked on from inside another: a short-
        // lived one on its own thread drops the database, which also ends the pool's sessions.
        let name = self.name.clone();
        let dropped = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("a runtime to drop the test database");
            runtime.block_on(async move {
                let mut admin = admin().await;
                execute(
                    &mut admin,
                    &format!(r#"DROP DATABASE IF EXISTS "{name}" WITH (FORCE)"#),
                )
                .await;
            });
        });
        dropped.join().expect("the test database is dropped");
    }
}

/// A fresh database for one test, migrated and seeded as the application's own.
pub async fn database() -> TestDb {
    let template = template().await;
    let name = format!(
        "{TEST_PREFIX}{}_{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let mut admin = admin().await;
    execute(
        &mut admin,
        &format!(r#"CREATE DATABASE "{name}" TEMPLATE "{template}""#),
    )
    .await;
    let _ = admin.close().await;
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect_with(server().database(&name))
        .await
        .expect("connect to the test database");
    TestDb { name, pool }
}

/// The template for the migrations as they are now, built once if it is not there yet.
async fn template() -> &'static str {
    TEMPLATE
        .get_or_init(|| async {
            let name = template_name();
            let mut admin = admin().await;
            sqlx::query("SELECT pg_advisory_lock($1)")
                .bind(LOCK_KEY)
                .execute(&mut admin)
                .await
                .expect("lock the template");

            let exists: bool =
                sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_database WHERE datname = $1)")
                    .bind(&name)
                    .fetch_one(&mut admin)
                    .await
                    .expect("look for the template");
            if !exists {
                // Built under another name and renamed when done: a half-built template is
                // never cloned, and an interrupted build is swept away below.
                let building = format!("{name}_build");
                execute(
                    &mut admin,
                    &format!(r#"DROP DATABASE IF EXISTS "{building}" WITH (FORCE)"#),
                )
                .await;
                execute(&mut admin, &format!(r#"CREATE DATABASE "{building}""#)).await;
                let pool = PgPoolOptions::new()
                    .max_connections(1)
                    .connect_with(server().database(&building))
                    .await
                    .expect("connect to the template being built");
                k_vet_backend::run_migrations(&pool)
                    .await
                    .expect("the migrations build the template");
                pool.close().await;
                execute(
                    &mut admin,
                    &format!(r#"ALTER DATABASE "{building}" RENAME TO "{name}""#),
                )
                .await;
            }

            // Templates of other migration sets, and test databases nobody is connected to —
            // a failed test's, kept for inspection, or a run that was cut short.
            let stale: Vec<String> = sqlx::query_scalar(
                r"SELECT datname FROM pg_database
                  WHERE (datname LIKE 'kvet\_template\_%' AND datname <> $1)
                     OR (datname LIKE 'kvet\_test\_%'
                         AND NOT EXISTS (SELECT 1 FROM pg_stat_activity activity
                                          WHERE activity.datname = pg_database.datname))",
            )
            .bind(&name)
            .fetch_all(&mut admin)
            .await
            .expect("find stale databases");
            for database in stale {
                execute(
                    &mut admin,
                    &format!(r#"DROP DATABASE IF EXISTS "{database}" WITH (FORCE)"#),
                )
                .await;
            }

            sqlx::query("SELECT pg_advisory_unlock($1)")
                .bind(LOCK_KEY)
                .execute(&mut admin)
                .await
                .expect("unlock the template");
            let _ = admin.close().await;
            name
        })
        .await
}

/// `kvet_template_<12 hex>`: a hash over every migration's version and checksum, so a new or
/// changed migration names — and builds — a new template.
fn template_name() -> String {
    let migrator = sqlx::migrate!("./migrations");
    let mut hash = Sha256::new();
    for migration in migrator.iter() {
        hash.update(migration.version.to_le_bytes());
        hash.update(&*migration.checksum);
    }
    let digest: String = hash
        .finalize()
        .iter()
        .take(6)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("{TEMPLATE_PREFIX}{digest}")
}

/// The test server, as `DATABASE_URL` names it — the same one `#[sqlx::test]` used.
fn server() -> PgConnectOptions {
    std::env::var("DATABASE_URL")
        .expect("DATABASE_URL points at the test server")
        .parse()
        .expect("DATABASE_URL parses")
}

async fn admin() -> PgConnection {
    server()
        .connect()
        .await
        .expect("connect to the test server")
}

/// A statement that cannot be prepared or bound — `CREATE`/`DROP DATABASE` with a name in it.
async fn execute(connection: &mut PgConnection, statement: &str) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(statement.to_owned()))
        .execute(connection)
        .await
        .unwrap_or_else(|error| panic!("{statement}: {error}"));
}
