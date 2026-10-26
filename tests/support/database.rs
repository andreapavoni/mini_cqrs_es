//! Shared backend configuration. PostgreSQL test URLs must name disposable databases.
#![allow(dead_code)]
use mini_cqrs_es::toasty;
use std::path::Path;

pub async fn fresh_url(_path: &Path, read_models: bool) -> String {
    let variable = if read_models {
        "READ_MODELS_DATABASE_URL"
    } else {
        "EVENTS_DATABASE_URL"
    };
    if let Ok(base) = std::env::var(variable) {
        assert!(
            base.starts_with("postgresql:") || base.starts_with("postgres:"),
            "test ENV URLs must select PostgreSQL"
        );
        let schema = format!("cqrs_test_{}", uuid::Uuid::new_v4().simple());
        let mut admin = toasty::Db::builder()
            .connect(&base)
            .await
            .expect("connect PostgreSQL test database");
        toasty::sql::statement(format!("CREATE SCHEMA {schema}"))
            .exec(&mut admin)
            .await
            .expect("create isolated PostgreSQL test schema");
        let separator = if base.contains('?') { '&' } else { '?' };
        format!("{base}{separator}options=-c%20search_path%3D{schema}")
    } else {
        assert!(
            std::env::var("EVENTS_DATABASE_URL").is_err()
                && std::env::var("READ_MODELS_DATABASE_URL").is_err(),
            "set BOTH PostgreSQL test URLs"
        );
        #[cfg(feature = "turso")]
        {
            format!("turso:{}", _path.display())
        }
        #[cfg(not(feature = "turso"))]
        panic!("PostgreSQL tests require EVENTS_DATABASE_URL and READ_MODELS_DATABASE_URL")
    }
}

pub fn postgres() -> bool {
    std::env::var("EVENTS_DATABASE_URL").is_ok()
}
