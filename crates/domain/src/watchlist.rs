use chrono::{DateTime, Utc};
use thiserror::Error;

/// Longest accepted watchlist name, in characters.
pub const MAX_WATCHLIST_NAME_CHARS: usize = 80;
/// Upper bound on the symbol set, so one list cannot become an unbounded scan.
pub const MAX_WATCHLIST_SYMBOLS: usize = 200;

/// A named symbol list owned by one subject, matching the legacy `watchlists` shape.
///
/// `symbols` is ordered and de-duplicated; the order is the display order. Ownership
/// is explicit: every read and write is scoped by owner, never by list name alone.
#[derive(Debug, Clone, PartialEq)]
pub struct Watchlist {
    id: String,
    owner_id: String,
    name: String,
    symbols: Vec<String>,
    is_default: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl Watchlist {
    /// Builds a list, rejecting a blank name or a symbol that is not a plain instrument id.
    pub fn new(
        id: String,
        owner_id: String,
        name: String,
        symbols: Vec<String>,
        is_default: bool,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Result<Self, WatchlistError> {
        Ok(Self {
            id: required("list", id)?,
            owner_id: required("owner", owner_id)?,
            name: normalize_name(&name)?,
            symbols: normalize_symbols(&symbols)?,
            is_default,
            created_at,
            updated_at,
        })
    }

    /// Returns a copy with a replaced symbol set and a new update time.
    pub fn with_symbols(
        self,
        symbols: Vec<String>,
        updated_at: DateTime<Utc>,
    ) -> Result<Self, WatchlistError> {
        Ok(Self {
            symbols: normalize_symbols(&symbols)?,
            updated_at,
            ..self
        })
    }

    #[must_use]
    pub fn as_default(self) -> Self {
        Self {
            is_default: true,
            ..self
        }
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn owner_id(&self) -> &str {
        &self.owner_id
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn symbols(&self) -> &[String] {
        &self.symbols
    }

    #[must_use]
    pub const fn is_default(&self) -> bool {
        self.is_default
    }

    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    #[must_use]
    pub const fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }
}

/// Trims a name and enforces the length limit.
pub fn normalize_name(name: &str) -> Result<String, WatchlistError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(WatchlistError::EmptyName);
    }
    if name.chars().count() > MAX_WATCHLIST_NAME_CHARS {
        return Err(WatchlistError::NameTooLong);
    }
    Ok(name.to_owned())
}

/// Trims, uppercases, and de-duplicates symbols while keeping first-appearance order.
pub fn normalize_symbols(symbols: &[String]) -> Result<Vec<String>, WatchlistError> {
    if symbols.len() > MAX_WATCHLIST_SYMBOLS {
        return Err(WatchlistError::TooManySymbols);
    }
    let mut normalized: Vec<String> = Vec::with_capacity(symbols.len());
    for symbol in symbols {
        let symbol = symbol.trim().to_ascii_uppercase();
        if symbol.is_empty() {
            return Err(WatchlistError::EmptySymbol);
        }
        if !symbol.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        }) {
            return Err(WatchlistError::InvalidSymbol(symbol));
        }
        if !normalized.contains(&symbol) {
            normalized.push(symbol);
        }
    }
    Ok(normalized)
}

fn required(field: &'static str, value: String) -> Result<String, WatchlistError> {
    if value.trim().is_empty() {
        return Err(WatchlistError::EmptyField(field));
    }
    Ok(value)
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum WatchlistError {
    #[error("watchlist {0} must not be empty")]
    EmptyField(&'static str),
    #[error("watchlist name must not be empty")]
    EmptyName,
    #[error("watchlist name is too long")]
    NameTooLong,
    #[error("watchlist symbol must not be empty")]
    EmptySymbol,
    #[error("watchlist symbol '{0}' is not a plain instrument id")]
    InvalidSymbol(String),
    #[error("a watchlist cannot hold more than {MAX_WATCHLIST_SYMBOLS} symbols")]
    TooManySymbols,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 19, 0, 0, 0).unwrap()
    }

    fn list(symbols: &[&str]) -> Result<Watchlist, WatchlistError> {
        Watchlist::new(
            "6512a1f4c8e3b2a91f0d4e77".to_owned(),
            "local-operator".to_owned(),
            "My List".to_owned(),
            symbols.iter().map(|symbol| (*symbol).to_owned()).collect(),
            true,
            at(),
            at(),
        )
    }

    #[test]
    fn normalizes_symbols_in_first_appearance_order() {
        let watchlist = list(&["btcusdt", " ETHUSDT ", "BTCUSDT", "btc-usdt-swap"]).unwrap();

        assert_eq!(watchlist.symbols(), ["BTCUSDT", "ETHUSDT", "BTC-USDT-SWAP"]);
    }

    #[test]
    fn rejects_an_empty_name() {
        assert_eq!(
            Watchlist::new(
                "id".to_owned(),
                "owner".to_owned(),
                "   ".to_owned(),
                Vec::new(),
                false,
                at(),
                at()
            ),
            Err(WatchlistError::EmptyName)
        );
    }

    #[test]
    fn rejects_a_symbol_with_unallowed_characters() {
        assert_eq!(
            list(&["BTC USDT"]),
            Err(WatchlistError::InvalidSymbol("BTC USDT".to_owned()))
        );
    }

    #[test]
    fn rejects_an_empty_symbol() {
        assert_eq!(list(&[""]), Err(WatchlistError::EmptySymbol));
    }

    #[test]
    fn replacing_symbols_keeps_identity_and_bumps_the_update_time() {
        let watchlist = list(&["BTCUSDT"]).unwrap();
        let later = at() + chrono::Duration::seconds(30);

        let updated = watchlist
            .clone()
            .with_symbols(vec!["ethusdt".to_owned()], later)
            .unwrap();

        assert_eq!(updated.id(), watchlist.id());
        assert_eq!(updated.symbols(), ["ETHUSDT"]);
        assert_eq!(updated.updated_at(), later);
    }
}
