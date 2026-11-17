use std::fmt;
use std::net::SocketAddr;

use thiserror::Error;
use url::Url;

const DEFAULT_API_BIND: &str = "127.0.0.1:4101";
const DEFAULT_API_PREFIX: &str = "/api/v1";
const DEFAULT_DATABASE_NAME: &str = "tm-trading-rust";
const DEFAULT_DATABASE_URI: &str = "mongodb://127.0.0.1:27017/?tls=true";
const DEFAULT_HUB_URL: &str = "http://127.0.0.1:4000";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    Development,
    Test,
    Production,
}

impl Environment {
    fn parse(value: &str) -> Result<Self, ConfigError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "development" | "dev" => Ok(Self::Development),
            "test" => Ok(Self::Test),
            "production" | "prod" => Ok(Self::Production),
            _ => Err(ConfigError::InvalidEnvironment(value.to_owned())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeploymentMode {
    Local,
    Remote,
}

impl DeploymentMode {
    fn parse(value: &str) -> Result<Self, ConfigError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "local" => Ok(Self::Local),
            "remote" => Ok(Self::Remote),
            _ => Err(ConfigError::InvalidDeploymentMode(value.to_owned())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Compact,
    Json,
}

impl LogFormat {
    fn parse(value: &str) -> Result<Self, ConfigError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "compact" => Ok(Self::Compact),
            "json" => Ok(Self::Json),
            _ => Err(ConfigError::InvalidLogFormat(value.to_owned())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiConfig {
    pub bind: SocketAddr,
    pub prefix: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct DatabaseConfig {
    uri: String,
    pub name: String,
}

impl DatabaseConfig {
    #[must_use]
    pub fn uri(&self) -> &str {
        &self.uri
    }
}

impl fmt::Debug for DatabaseConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DatabaseConfig")
            .field("uri", &"[REDACTED]")
            .field("name", &self.name)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HubConfig {
    pub base_url: Url,
    pub app_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    pub environment: Environment,
    pub deployment: DeploymentMode,
    pub api: ApiConfig,
    pub database: DatabaseConfig,
    pub hub: HubConfig,
    pub log_format: LogFormat,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_source(&EnvironmentSource)
    }

    pub fn from_source(source: &dyn ConfigSource) -> Result<Self, ConfigError> {
        let environment = source
            .get("TM_ENVIRONMENT")
            .as_deref()
            .map(Environment::parse)
            .transpose()?
            .unwrap_or(Environment::Development);

        let deployment = source
            .get("TM_DEPLOYMENT_MODE")
            .as_deref()
            .map(DeploymentMode::parse)
            .transpose()?
            .unwrap_or(DeploymentMode::Local);

        let bind = source
            .get("TM_API_BIND")
            .unwrap_or_else(|| DEFAULT_API_BIND.to_owned());
        let bind = bind
            .trim()
            .parse::<SocketAddr>()
            .map_err(|_| ConfigError::InvalidApiBind(bind.clone()))?;
        if deployment == DeploymentMode::Local && !bind.ip().is_loopback() {
            return Err(ConfigError::NonLoopbackBind);
        }

        let prefix = source
            .get("TM_API_PREFIX")
            .unwrap_or_else(|| DEFAULT_API_PREFIX.to_owned());
        let prefix = prefix.trim().to_owned();
        if !prefix.starts_with('/') || (prefix.len() > 1 && prefix.ends_with('/')) {
            return Err(ConfigError::InvalidApiPrefix(prefix));
        }

        let explicit_database_uri = source.get("TM_DATABASE_URI");
        let database_uri = explicit_database_uri
            .clone()
            .unwrap_or_else(|| DEFAULT_DATABASE_URI.to_owned());
        if database_uri.trim().is_empty() {
            return Err(ConfigError::EmptyValue("TM_DATABASE_URI"));
        }

        let database_name = source
            .get("TM_DATABASE_NAME")
            .unwrap_or_else(|| DEFAULT_DATABASE_NAME.to_owned());
        let database_name = database_name.trim().to_owned();
        validate_database_name(&database_name)?;

        let explicit_hub_url = source.get("TM_HUB_URL");
        let hub_url = explicit_hub_url
            .clone()
            .unwrap_or_else(|| DEFAULT_HUB_URL.to_owned());
        let hub_url = Url::parse(hub_url.trim()).map_err(|_| ConfigError::InvalidHubUrl)?;
        if !matches!(hub_url.scheme(), "http" | "https") {
            return Err(ConfigError::InvalidHubUrl);
        }
        if !hub_url.username().is_empty() || hub_url.password().is_some() {
            return Err(ConfigError::InvalidHubUrl);
        }

        let hub_app_id = source
            .get("TM_HUB_APP_ID")
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());

        let log_format = source
            .get("TM_LOG_FORMAT")
            .as_deref()
            .map(LogFormat::parse)
            .transpose()?
            .unwrap_or(LogFormat::Compact);

        if environment == Environment::Production {
            if explicit_database_uri.is_none() {
                return Err(ConfigError::MissingProductionValue("TM_DATABASE_URI"));
            }
            if explicit_hub_url.is_none() {
                return Err(ConfigError::MissingProductionValue("TM_HUB_URL"));
            }
            if hub_url.scheme() != "https" {
                return Err(ConfigError::InsecureProductionHubUrl);
            }
            if hub_app_id.is_none() {
                return Err(ConfigError::MissingProductionValue("TM_HUB_APP_ID"));
            }
        }

        Ok(Self {
            environment,
            deployment,
            api: ApiConfig { bind, prefix },
            database: DatabaseConfig {
                uri: database_uri,
                name: database_name,
            },
            hub: HubConfig {
                base_url: hub_url,
                app_id: hub_app_id,
            },
            log_format,
        })
    }
}

fn validate_database_name(value: &str) -> Result<(), ConfigError> {
    if value.is_empty() {
        return Err(ConfigError::EmptyValue("TM_DATABASE_NAME"));
    }
    if value.contains(['/', '\\', '.', ' ', '"', '$', '\0']) {
        return Err(ConfigError::InvalidDatabaseName(value.to_owned()));
    }
    Ok(())
}

pub trait ConfigSource {
    fn get(&self, key: &str) -> Option<String>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct EnvironmentSource;

impl ConfigSource for EnvironmentSource {
    fn get(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ConfigError {
    #[error("invalid environment value: {0}")]
    InvalidEnvironment(String),
    #[error("invalid log format value: {0}")]
    InvalidLogFormat(String),
    #[error("invalid API bind address: {0}")]
    InvalidApiBind(String),
    #[error("invalid deployment mode: {0}")]
    InvalidDeploymentMode(String),
    #[error("local deployment mode requires a loopback API bind address")]
    NonLoopbackBind,
    #[error("invalid API prefix: {0}")]
    InvalidApiPrefix(String),
    #[error("invalid MongoDB database name: {0}")]
    InvalidDatabaseName(String),
    #[error("invalid TM Hub URL")]
    InvalidHubUrl,
    #[error("production TM Hub URL must use HTTPS")]
    InsecureProductionHubUrl,
    #[error("configuration value {0} must not be empty")]
    EmptyValue(&'static str),
    #[error("production configuration requires {0}")]
    MissingProductionValue(&'static str),
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[derive(Default)]
    struct MapSource(HashMap<String, String>);

    impl ConfigSource for MapSource {
        fn get(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
    }

    #[test]
    fn development_uses_safe_local_defaults() {
        let config = AppConfig::from_source(&MapSource::default()).unwrap();

        assert_eq!(config.environment, Environment::Development);
        assert_eq!(config.deployment, DeploymentMode::Local);
        assert_eq!(config.api.bind.to_string(), DEFAULT_API_BIND);
        assert_eq!(config.api.prefix, DEFAULT_API_PREFIX);
        assert_eq!(config.database.uri(), DEFAULT_DATABASE_URI);
        assert_eq!(config.database.name, DEFAULT_DATABASE_NAME);
        assert_eq!(config.hub.base_url.as_str(), "http://127.0.0.1:4000/");
        assert_eq!(config.hub.app_id, None);
    }

    #[test]
    fn local_mode_rejects_a_non_loopback_bind() {
        let mut source = MapSource::default();
        source
            .0
            .insert("TM_API_BIND".to_owned(), "0.0.0.0:4101".to_owned());

        let error = AppConfig::from_source(&source).unwrap_err();

        assert_eq!(error, ConfigError::NonLoopbackBind);
    }

    #[test]
    fn remote_mode_allows_an_explicit_non_loopback_bind() {
        let mut source = MapSource::default();
        source
            .0
            .insert("TM_DEPLOYMENT_MODE".to_owned(), "remote".to_owned());
        source
            .0
            .insert("TM_API_BIND".to_owned(), "0.0.0.0:4101".to_owned());

        let config = AppConfig::from_source(&source).unwrap();

        assert_eq!(config.deployment, DeploymentMode::Remote);
    }

    #[test]
    fn rejects_invalid_deployment_mode() {
        let mut source = MapSource::default();
        source
            .0
            .insert("TM_DEPLOYMENT_MODE".to_owned(), "cluster".to_owned());

        let error = AppConfig::from_source(&source).unwrap_err();

        assert_eq!(
            error,
            ConfigError::InvalidDeploymentMode("cluster".to_owned())
        );
    }

    #[test]
    fn rejects_invalid_api_bind() {
        let mut source = MapSource::default();
        source
            .0
            .insert("TM_API_BIND".to_owned(), "not-an-address".to_owned());

        let error = AppConfig::from_source(&source).unwrap_err();

        assert_eq!(
            error,
            ConfigError::InvalidApiBind("not-an-address".to_owned())
        );
    }

    #[test]
    fn rejects_invalid_api_prefix() {
        let mut source = MapSource::default();
        source
            .0
            .insert("TM_API_PREFIX".to_owned(), "api/v1".to_owned());

        let error = AppConfig::from_source(&source).unwrap_err();

        assert_eq!(error, ConfigError::InvalidApiPrefix("api/v1".to_owned()));
    }

    #[test]
    fn rejects_invalid_database_name() {
        let mut source = MapSource::default();
        source
            .0
            .insert("TM_DATABASE_NAME".to_owned(), "bad.name".to_owned());

        let error = AppConfig::from_source(&source).unwrap_err();

        assert_eq!(
            error,
            ConfigError::InvalidDatabaseName("bad.name".to_owned())
        );
    }

    #[test]
    fn rejects_invalid_hub_url() {
        let mut source = MapSource::default();
        source.0.insert(
            "TM_HUB_URL".to_owned(),
            "ftp://hub.example.invalid".to_owned(),
        );

        let error = AppConfig::from_source(&source).unwrap_err();

        assert_eq!(error, ConfigError::InvalidHubUrl);
    }

    #[test]
    fn production_fails_closed_without_explicit_secure_values() {
        let mut source = MapSource::default();
        source
            .0
            .insert("TM_ENVIRONMENT".to_owned(), "production".to_owned());

        let error = AppConfig::from_source(&source).unwrap_err();

        assert_eq!(
            error,
            ConfigError::MissingProductionValue("TM_DATABASE_URI")
        );
    }

    #[test]
    fn production_accepts_explicit_secure_values() {
        let mut source = MapSource::default();
        source
            .0
            .insert("TM_ENVIRONMENT".to_owned(), "production".to_owned());
        source.0.insert(
            "TM_DATABASE_URI".to_owned(),
            "mongodb://database.example.invalid:27017".to_owned(),
        );
        source.0.insert(
            "TM_HUB_URL".to_owned(),
            "https://hub.example.invalid".to_owned(),
        );
        source.0.insert(
            "TM_HUB_APP_ID".to_owned(),
            "tm-trading-rust_test".to_owned(),
        );

        let config = AppConfig::from_source(&source).unwrap();

        assert_eq!(config.environment, Environment::Production);
        assert_eq!(config.hub.base_url.scheme(), "https");
        assert_eq!(config.hub.app_id.as_deref(), Some("tm-trading-rust_test"));
    }

    #[test]
    fn database_debug_output_redacts_uri() {
        let config = DatabaseConfig {
            uri: "mongodb://user:password@database.example.invalid".to_owned(),
            name: "tm-trading-rust".to_owned(),
        };

        let debug = format!("{config:?}");

        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("password"));
    }
}
