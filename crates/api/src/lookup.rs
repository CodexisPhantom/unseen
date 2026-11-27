//! Movie lookup against the metadata provider (docs/05-api.md).

use axum::Json;
use axum::extract::{Query, State};
use serde::{Deserialize, Serialize};
use unseenarr_core::metadata::{LookupTerm, MetadataError, MovieSummary, parse_term};
use utoipa::{IntoParams, ToSchema};

use crate::AppState;
use crate::access::{LibraryAdd, Require};
use crate::error::Problem;
use crate::tmdb::{client, provider_error};

/// Query of `GET /api/v1/movies/lookup`.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LookupParams {
    /// Text to search for, `tmdb:123` or `imdb:tt0133093`.
    term: Option<String>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MovieLookup {
    tmdb_id: i32,
    /// Known for `tmdb:` and `imdb:` lookups, not for a text search.
    imdb_id: Option<String>,
    title: String,
    original_title: String,
    year: Option<i32>,
    /// `YYYY-MM-DD`.
    release_date: Option<String>,
    overview: String,
    /// Path on TMDB's image host; the browser loads the image.
    poster_path: Option<String>,
}

impl MovieLookup {
    fn new(movie: MovieSummary, imdb_id: Option<String>) -> Self {
        Self {
            tmdb_id: movie.tmdb_id,
            imdb_id,
            year: movie.year(),
            title: movie.title,
            original_title: movie.original_title,
            release_date: movie.release_date,
            overview: movie.overview,
            poster_path: movie.poster_path,
        }
    }
}

/// Finds movies on TMDB by title, TMDB id (`tmdb:603`) or IMDb id
/// (`imdb:tt0133093`). An id that matches nothing gives an empty list.
#[utoipa::path(
    get,
    path = "/api/v1/movies/lookup",
    tag = "movies",
    params(LookupParams),
    responses(
        (status = 200, description = "The matches", body = Vec<MovieLookup>),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 409, description = "The TMDB key is not set (`tmdb_not_configured`)", body = Problem),
        (status = 422, description = "The term is empty or malformed", body = Problem),
        (status = 502, description = "TMDB failed or refused the key", body = Problem),
    )
)]
pub async fn lookup(
    _: Require<LibraryAdd>,
    State(state): State<AppState>,
    Query(params): Query<LookupParams>,
) -> Result<Json<Vec<MovieLookup>>, Problem> {
    let term = parse_term(params.term.as_deref().unwrap_or_default())
        .map_err(|error| Problem::validation("term", &error.to_string()))?;
    let tmdb = client(&state).await?;
    let found = match term {
        LookupTerm::Text(text) => tmdb.search(&text).await.map(|movies| {
            movies
                .into_iter()
                .map(|m| MovieLookup::new(m, None))
                .collect()
        }),
        LookupTerm::Imdb(id) => tmdb.find_by_imdb_id(&id).await.map(|movies| {
            movies
                .into_iter()
                .map(|m| MovieLookup::new(m, Some(id.clone())))
                .collect()
        }),
        LookupTerm::Tmdb(id) => tmdb
            .details(id)
            .await
            .map(|d| vec![MovieLookup::new(d.summary, d.imdb_id)]),
    };
    match found {
        Ok(found) => Ok(Json(found)),
        Err(MetadataError::NotFound) => Ok(Json(Vec::new())),
        Err(error) => Err(provider_error(error)),
    }
}
