//! Contracts and integrity verification for system configuration backup & restore.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use utoipa::ToSchema;

use crate::{AiModelDto, AiProviderProfileDto, AiTaskRouteDto, ApiVersionInfo, WatchlistDto};

/// Payload containing the backed-up configurations and user data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct BackupPayloadDto {
    pub watchlists: Vec<WatchlistDto>,
    pub ai_provider_profiles: Vec<AiProviderProfileDto>,
    #[serde(default)]
    pub ai_models: Vec<AiModelDto>,
    pub ai_task_routes: Vec<AiTaskRouteDto>,
}

impl BackupPayloadDto {
    /// Computes the SHA-256 digest of the canonical JSON representation of this payload.
    pub fn compute_sha256(&self) -> Result<String, serde_json::Error> {
        let serialized = serde_json::to_vec(self)?;
        let mut hasher = Sha256::new();
        hasher.update(&serialized);
        let result = hasher.finalize();
        Ok(format!("{result:x}"))
    }
}

/// Standalone, portable backup archive with metadata and SHA-256 integrity checksum.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct BackupArchiveDto {
    pub version: ApiVersionInfo,
    pub exported_at: DateTime<Utc>,
    pub environment: String,
    pub sha256_checksum: String,
    pub payload: BackupPayloadDto,
}

impl BackupArchiveDto {
    /// Constructs a new verified backup archive from payload and environment name.
    pub fn new(
        version: ApiVersionInfo,
        environment: String,
        payload: BackupPayloadDto,
    ) -> Result<Self, serde_json::Error> {
        let sha256_checksum = payload.compute_sha256()?;
        Ok(Self {
            version,
            exported_at: Utc::now(),
            environment,
            sha256_checksum,
            payload,
        })
    }

    /// Verifies whether the archive's internal checksum matches its payload.
    #[must_use]
    pub fn verify_integrity(&self) -> bool {
        match self.payload.compute_sha256() {
            Ok(computed) => computed.eq_ignore_ascii_case(&self.sha256_checksum),
            Err(_) => false,
        }
    }
}

/// Request to restore configurations from a backup archive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RestoreBackupRequestDto {
    pub archive: BackupArchiveDto,
    /// Whether to replace existing configurations entirely or merge. Default false (merge/upsert).
    #[serde(default)]
    pub overwrite_existing: bool,
}

/// Result report after executing or simulating a restore operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RestoreBackupResultDto {
    pub restored_at: DateTime<Utc>,
    pub checksum_verified: bool,
    pub watchlists_restored: usize,
    pub ai_provider_profiles_restored: usize,
    #[serde(default)]
    pub ai_models_restored: usize,
    pub ai_task_routes_restored: usize,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_and_verifies_payload_checksum() {
        let payload = BackupPayloadDto {
            watchlists: Vec::new(),
            ai_provider_profiles: Vec::new(),
            ai_models: Vec::new(),
            ai_task_routes: Vec::new(),
        };

        let checksum = payload.compute_sha256().expect("checksum computation");
        assert!(!checksum.is_empty());

        let archive = BackupArchiveDto::new(
            ApiVersionInfo::current(),
            "local-test".to_owned(),
            payload.clone(),
        )
        .expect("archive creation");

        assert!(archive.verify_integrity());

        // Tampering with payload should fail verification
        let mut tampered = archive.clone();
        tampered.sha256_checksum =
            "0000000000000000000000000000000000000000000000000000000000000000".to_owned();
        assert!(!tampered.verify_integrity());
    }
}
