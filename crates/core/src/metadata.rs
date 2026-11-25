//! The metadata provider trait (docs/08-integrations.md). The rest of the
//! code depends on this trait only; TMDB is the one implementation in V1.

use std::fmt;

use async_trait::async_trait;

/// A movie as a search or a collection lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovieSummary {
    pub tmdb_id: i32,
    pub title: String,
    pub original_title: String,
    pub overview: String,
    /// `YYYY-MM-DD`, absent when the movie has no date yet.
    pub release_date: Option<String>,
    /// Path on the provider's image host; the browser loads the image.
    pub poster_path: Option<String>,
}

impl MovieSummary {
    pub fn year(&self) -> Option<i32> {
        self.release_date.as_deref()?.get(..4)?.parse().ok()
    }
}

/// The kind of a release date, with TMDB's numbering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseKind {
    Premiere,
    TheatricalLimited,
    Theatrical,
    Digital,
    Physical,
    Tv,
}

impl ReleaseKind {
    pub fn from_code(code: i64) -> Option<Self> {
        Some(match code {
            1 => Self::Premiere,
            2 => Self::TheatricalLimited,
            3 => Self::Theatrical,
            4 => Self::Digital,
            5 => Self::Physical,
            6 => Self::Tv,
            _ => return None,
        })
    }
}

/// One release date in one country.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseDate {
    /// ISO 3166-1 code.
    pub country: String,
    pub kind: ReleaseKind,
    /// ISO 8601 time stamp, as the provider sends it.
    pub date: String,
    pub certification: Option<String>,
}

/// The collection a movie belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionRef {
    pub tmdb_id: i32,
    pub name: String,
}

/// One movie with its external ids and release dates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovieDetails {
    pub summary: MovieSummary,
    pub imdb_id: Option<String>,
    pub runtime_minutes: Option<u32>,
    pub status: String,
    pub genres: Vec<String>,
    pub collection: Option<CollectionRef>,
    pub release_dates: Vec<ReleaseDate>,
}

/// A collection with its movies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collection {
    pub tmdb_id: i32,
    pub name: String,
    pub overview: String,
    pub poster_path: Option<String>,
    pub movies: Vec<MovieSummary>,
}

/// Why a provider call failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetadataError {
    NotFound,
    /// The provider refused the API key.
    Unauthorized,
    /// Still rate limited after the retries.
    RateLimited,
    /// Network failure, server error or an answer that cannot be read.
    Unavailable(String),
}

impl fmt::Display for MetadataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => f.write_str("not found"),
            Self::Unauthorized => f.write_str("the metadata provider refused the API key"),
            Self::RateLimited => f.write_str("the metadata provider is rate limiting requests"),
            Self::Unavailable(why) => write!(f, "the metadata provider failed: {why}"),
        }
    }
}

impl std::error::Error for MetadataError {}

#[async_trait]
pub trait MetadataProvider: Send + Sync {
    async fn search(&self, term: &str) -> Result<Vec<MovieSummary>, MetadataError>;
    /// Movies that carry this IMDb id; empty when none does.
    async fn find_by_imdb_id(&self, imdb_id: &str) -> Result<Vec<MovieSummary>, MetadataError>;
    async fn details(&self, tmdb_id: i32) -> Result<MovieDetails, MetadataError>;
    async fn collection(&self, tmdb_id: i32) -> Result<Collection, MetadataError>;
}

/// What the user typed into a lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LookupTerm {
    Text(String),
    Tmdb(i32),
    /// Lower case, `tt` and digits.
    Imdb(String),
}

/// A lookup term that is empty or has a malformed id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermError {
    Empty,
    TmdbId,
    ImdbId,
}

impl fmt::Display for TermError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "must not be empty",
            Self::TmdbId => "needs a number after `tmdb:`, for example `tmdb:603`",
            Self::ImdbId => "needs an id like `tt0133093` after `imdb:`",
        })
    }
}

impl std::error::Error for TermError {}

/// Reads plain text, `tmdb:123` or `imdb:tt123`. The prefixes ignore case.
pub fn parse_term(term: &str) -> Result<LookupTerm, TermError> {
    let term = term.trim();
    if term.is_empty() {
        return Err(TermError::Empty);
    }
    let prefixed = |prefix: &str| {
        term.get(..prefix.len())
            .filter(|head| head.eq_ignore_ascii_case(prefix))
            .map(|_| term[prefix.len()..].trim())
    };
    if let Some(id) = prefixed("tmdb:") {
        let id = id.parse::<i32>().ok().filter(|id| *id > 0);
        return id.map(LookupTerm::Tmdb).ok_or(TermError::TmdbId);
    }
    if let Some(id) = prefixed("imdb:") {
        let id = id.to_ascii_lowercase();
        let digits = id.strip_prefix("tt").unwrap_or_default();
        let valid = !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit());
        return if valid {
            Ok(LookupTerm::Imdb(id))
        } else {
            Err(TermError::ImdbId)
        };
    }
    Ok(LookupTerm::Text(term.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_three_forms() {
        for (input, expected) in [
            ("The Matrix", Ok(LookupTerm::Text("The Matrix".into()))),
            ("  matrix  ", Ok(LookupTerm::Text("matrix".into()))),
            ("tmdb:603", Ok(LookupTerm::Tmdb(603))),
            ("TMDB: 603", Ok(LookupTerm::Tmdb(603))),
            ("imdb:tt0133093", Ok(LookupTerm::Imdb("tt0133093".into()))),
            ("IMDB:TT0133093", Ok(LookupTerm::Imdb("tt0133093".into()))),
            // A title that merely starts like a prefix is text.
            ("tmdbx", Ok(LookupTerm::Text("tmdbx".into()))),
            ("", Err(TermError::Empty)),
            ("   ", Err(TermError::Empty)),
            ("tmdb:", Err(TermError::TmdbId)),
            ("tmdb:abc", Err(TermError::TmdbId)),
            ("tmdb:0", Err(TermError::TmdbId)),
            ("tmdb:-4", Err(TermError::TmdbId)),
            ("tmdb:99999999999", Err(TermError::TmdbId)),
            ("imdb:", Err(TermError::ImdbId)),
            ("imdb:123", Err(TermError::ImdbId)),
            ("imdb:tt", Err(TermError::ImdbId)),
            ("imdb:ttabc", Err(TermError::ImdbId)),
        ] {
            assert_eq!(parse_term(input), expected, "{input:?}");
        }
    }

    #[test]
    fn a_multibyte_title_is_text() {
        assert_eq!(parse_term("é"), Ok(LookupTerm::Text("é".into())));
        assert_eq!(
            parse_term("日本語の映画"),
            Ok(LookupTerm::Text("日本語の映画".into()))
        );
    }

    #[test]
    fn year_comes_from_the_release_date() {
        let mut movie = MovieSummary {
            tmdb_id: 1,
            title: String::new(),
            original_title: String::new(),
            overview: String::new(),
            release_date: Some("1999-03-30".into()),
            poster_path: None,
        };
        assert_eq!(movie.year(), Some(1999));
        movie.release_date = None;
        assert_eq!(movie.year(), None);
        movie.release_date = Some("x".into());
        assert_eq!(movie.year(), None);
    }

    #[test]
    fn release_kinds_follow_tmdb_codes() {
        assert_eq!(ReleaseKind::from_code(3), Some(ReleaseKind::Theatrical));
        assert_eq!(ReleaseKind::from_code(4), Some(ReleaseKind::Digital));
        assert_eq!(ReleaseKind::from_code(0), None);
        assert_eq!(ReleaseKind::from_code(7), None);
    }

    #[test]
    fn errors_have_text() {
        for error in [
            MetadataError::NotFound,
            MetadataError::Unauthorized,
            MetadataError::RateLimited,
            MetadataError::Unavailable("boom".into()),
        ] {
            assert!(!error.to_string().is_empty());
        }
        for error in [TermError::Empty, TermError::TmdbId, TermError::ImdbId] {
            assert!(!error.to_string().is_empty());
        }
    }
}
