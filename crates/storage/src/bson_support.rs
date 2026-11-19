use chrono::{DateTime, TimeZone, Utc};
use mongodb::bson::{Bson, DateTime as BsonDateTime};

use crate::RepositoryError;

/// Converts a UTC instant to the BSON `DateTime` used by every collection we write.
pub(crate) fn timestamp(value: DateTime<Utc>) -> BsonDateTime {
    BsonDateTime::from_millis(value.timestamp_millis())
}

pub(crate) fn from_timestamp(value: BsonDateTime) -> Result<DateTime<Utc>, RepositoryError> {
    Utc.timestamp_millis_opt(value.timestamp_millis())
        .single()
        .ok_or(RepositoryError::CorruptRecord("invalid timestamp"))
}

/// Reads a numeric field that may have been written as `Int32`, `Int64`, or `Double`.
///
/// Legacy documents were written by a JavaScript driver that stores whole numbers as
/// integers and fractional ones as doubles, so a strict reader would reject valid rows.
pub(crate) fn number_as_f64(field: &'static str, value: &Bson) -> Result<f64, RepositoryError> {
    match value {
        Bson::Double(value) => Ok(*value),
        Bson::Int32(value) => Ok(f64::from(*value)),
        Bson::Int64(value) => Ok(*value as f64),
        _ => Err(RepositoryError::CorruptRecord(field)),
    }
}

/// True when MongoDB rejected a write because a unique index was violated.
pub(crate) fn is_duplicate_key(error: &mongodb::error::Error) -> bool {
    matches!(
        *error.kind,
        mongodb::error::ErrorKind::Write(mongodb::error::WriteFailure::WriteError(
            mongodb::error::WriteError { code: 11000, .. }
        ))
    )
}

/// Reads an integral numeric field written as `Int32`, `Int64`, or an exact `Double`.
pub(crate) fn number_as_i64(field: &'static str, value: &Bson) -> Result<i64, RepositoryError> {
    match value {
        Bson::Int32(value) => Ok(i64::from(*value)),
        Bson::Int64(value) => Ok(*value),
        Bson::Double(value)
            if value.is_finite()
                && value.fract() == 0.0
                && *value >= i64::MIN as f64
                && *value <= i64::MAX as f64 =>
        {
            Ok(*value as i64)
        }
        _ => Err(RepositoryError::CorruptRecord(field)),
    }
}
