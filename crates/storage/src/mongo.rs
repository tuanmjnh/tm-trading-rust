use mongodb::bson::doc;
use mongodb::options::{ConnectionString, HostInfo, Tls};
use mongodb::{Client, Database};

use crate::RepositoryError;

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
