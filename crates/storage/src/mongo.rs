use mongodb::bson::{Document, doc};
use mongodb::options::{ConnectionString, HostInfo, Tls};
use mongodb::{Client, Database};

use crate::RepositoryError;
use crate::bson_support::number_as_i64;

pub struct MongoConnection {
    database: Database,
}

impl MongoConnection {
    pub async fn connect(uri: &str, database_name: &str) -> Result<Self, RepositoryError> {
        if database_name.trim().is_empty()
            || database_name.contains(['/', '\\', '.', ' ', '"', '$', '\0'])
        {
            return Err(RepositoryError::InvalidDatabaseName);
        }

        let connection_string = ConnectionString::parse(uri)?;
        match connection_string.tls {
            Some(Tls::Disabled) => return Err(RepositoryError::TlsRequired),
            Some(Tls::Enabled(options)) if options.allow_invalid_certificates == Some(true) => {
                return Err(RepositoryError::InsecureTls);
            }
            None if !matches!(connection_string.host_info, HostInfo::DnsRecord(_)) => {
                return Err(RepositoryError::TlsRequired);
            }
            _ => {}
        }

        let client = Client::with_uri_str(uri).await?;
        let database = client.database(database_name);
        Ok(Self { database })
    }

    #[must_use]
    pub fn database(&self) -> &Database {
        &self.database
    }

    pub async fn ping(&self) -> Result<(), RepositoryError> {
        self.database.run_command(doc! { "ping": 1 }).await?;
        Ok(())
    }

    /// Reports whether the deployment can commit multi-document transactions.
    pub async fn transactions_supported(&self) -> Result<bool, RepositoryError> {
        let hello = self.database.run_command(doc! { "hello": 1 }).await?;
        Ok(supports_transactions(&hello))
    }
}

fn supports_transactions(hello: &Document) -> bool {
    let max_wire_version = hello
        .get("maxWireVersion")
        .and_then(|value| number_as_i64("maxWireVersion", value).ok())
        .unwrap_or_default();
    let replica_set = hello.get_str("setName").is_ok_and(|name| !name.is_empty());
    let sharded_cluster = hello
        .get_str("msg")
        .is_ok_and(|message| message == "isdbgrid");

    (replica_set && max_wire_version >= 7) || (sharded_cluster && max_wire_version >= 8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn connection_without_tls_is_rejected() {
        let result = MongoConnection::connect("mongodb://127.0.0.1:27017", "tm-trading-rust").await;

        assert!(matches!(result, Err(RepositoryError::TlsRequired)));
    }

    #[tokio::test]
    async fn connection_configuration_with_verified_tls_is_accepted() {
        let result =
            MongoConnection::connect("mongodb://127.0.0.1:27017/?tls=true", "tm-trading-rust")
                .await;

        assert!(result.is_ok());
    }

    #[test]
    fn transaction_topology_requires_a_replica_set_or_mongos() {
        assert!(supports_transactions(&doc! {
            "setName": "rs0",
            "maxWireVersion": 21,
        }));
        assert!(supports_transactions(&doc! {
            "msg": "isdbgrid",
            "maxWireVersion": 21,
        }));
        assert!(!supports_transactions(&doc! { "maxWireVersion": 21 }));
        assert!(!supports_transactions(&doc! {
            "setName": "rs0",
            "maxWireVersion": 6,
        }));
    }

    #[tokio::test]
    async fn connection_allowing_invalid_certificates_is_rejected() {
        let result = MongoConnection::connect(
            "mongodb://127.0.0.1:27017/?tls=true&tlsAllowInvalidCertificates=true",
            "tm-trading-rust",
        )
        .await;

        assert!(matches!(result, Err(RepositoryError::InsecureTls)));
    }
}
