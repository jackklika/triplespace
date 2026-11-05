//! Connection pools (0033 §4): `deadpool-postgres` over `tokio-postgres`.

use std::sync::Arc;

use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod};
use rustls_platform_verifier::BuilderVerifierExt;
use tokio_postgres::NoTls;
use tokio_postgres::config::SslMode;
use tokio_postgres_rustls::MakeRustlsConnect;

/// Why a pool could not be built.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PoolError {
    /// The URL does not parse as a connection string.
    #[error("database URL: {0}")]
    Url(#[from] tokio_postgres::Error),
    /// The URL asks for TLS and the TLS client could not be configured.
    #[error("TLS: {0}")]
    Tls(#[from] rustls::Error),
    /// The pool refused its configuration.
    #[error("pool: {0}")]
    Build(String),
}

/// Whether connections to `config` go over TLS (0033 A25): when its `sslmode` is `require`.
/// `disable` and `prefer`, `tokio-postgres`'s default, connect in plaintext as before, so a
/// local server, or one behind a TLS-terminating proxy, is unaffected.
#[must_use]
pub fn wants_tls(config: &tokio_postgres::Config) -> bool {
    matches!(config.get_ssl_mode(), SslMode::Require)
}

/// The TLS connector for a connection whose URL says `sslmode=require` (0033 A25): rustls on
/// the aws-lc-rs provider (0033 §6), the server's certificate checked against the platform's
/// roots by `rustls-platform-verifier`, as the web tier's HTTP client checks the API's.
/// A managed Postgres that requires TLS (Neon, RDS, Supabase) is reached directly.
pub fn tls() -> Result<MakeRustlsConnect, rustls::Error> {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_platform_verifier()?
        .with_no_client_auth();
    Ok(MakeRustlsConnect::new(config))
}

/// A pool of at most `size` connections to `url`, each verified with a fast check when
/// recycled; over TLS when the URL says `sslmode=require` ([`wants_tls`]).
pub fn pool(url: &str, size: usize) -> Result<Pool, PoolError> {
    let config: tokio_postgres::Config = url.parse()?;
    let manager_config = ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    };
    let manager = if wants_tls(&config) {
        Manager::from_config(config, tls()?, manager_config)
    } else {
        Manager::from_config(config, NoTls, manager_config)
    };
    Pool::builder(manager)
        .max_size(size)
        .build()
        .map_err(|e| PoolError::Build(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // 0033 A25: only `sslmode=require` selects TLS; the default stays plaintext.
    #[test]
    fn tls_only_when_required() {
        let parse = |url: &str| url.parse::<tokio_postgres::Config>().expect("url");
        assert!(!wants_tls(&parse("postgres://u@localhost/db")));
        assert!(!wants_tls(&parse(
            "postgres://u@localhost/db?sslmode=disable"
        )));
        assert!(!wants_tls(&parse(
            "postgres://u@localhost/db?sslmode=prefer"
        )));
        assert!(wants_tls(&parse(
            "postgres://u@db.example/db?sslmode=require"
        )));
    }

    // 0033 A25: the connector builds with the aws-lc-rs provider and the platform verifier.
    #[test]
    fn tls_connector_builds() {
        tls().expect("TLS client config");
    }

    // 0033 A25: a pool for a `require` URL builds without connecting.
    #[test]
    fn pool_builds_for_require() {
        pool("postgres://u@db.example/db?sslmode=require", 2).expect("pool");
    }
}
