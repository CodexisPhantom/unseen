//! Radarr v3 compatibility movie read endpoints (`GET /movie`,
//! `GET /movie?tmdbId=`, `GET /movie/{id}`, `GET /movie/lookup?term=tmdb:{id}`)
//! (docs/14-compatibility.md, section D).

use std::collections::{BTreeSet, HashMap};

use arr_compat::radarr::dto::{
    LookupMovieResource, MediaInfoResource, MovieCollectionResource, MovieResource,
};
use arr_compat::radarr::map::{FileView, MovieView, lookup_movie_resource, movie_resource};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::Method;
use db::entities::movie_files;
use db::movies::Movie as StoredMovie;
use unseen_core::metadata::{LookupTerm, MetadataError, MovieDetails, parse_term};
use unseen_core::permission::Permission;

use crate::AppState;
use crate::compat::{ApiKeyCaller, CompatError};
use crate::tmdb;

fn db_failed(error: impl std::fmt::Display) -> CompatError {
    tracing::error!(%error, "radarr compat movies");
    CompatError::internal()
}

fn file_view(file: &movie_files::Model) -> FileView {
    let languages = file
        .languages
        .as_ref()
        .and_then(|value| serde_json::from_value::<Vec<String>>(value.clone()).ok())
        .unwrap_or_default();
    let media_info = file
        .media_info
        .as_ref()
        .and_then(|value| serde_json::from_value::<MediaInfoResource>(value.clone()).ok());
    FileView {
        id: file.id,
        relative_path: file.relative_path.clone(),
        size: file.size,
        quality_id: file.quality_id,
        release_group: file.release_group.clone(),
        edition: file.edition.clone(),
        scene_name: file.scene_name.clone(),
        languages,
        media_info,
        added_at: file.added_at.clone(),
    }
}

fn build_movie_view(
    stored: &StoredMovie,
    root_folders: &HashMap<i32, String>,
    collections: &HashMap<i32, MovieCollectionResource>,
    files: &HashMap<i32, movie_files::Model>,
) -> MovieView {
    let model = &stored.model;
    let primary = stored
        .profiles
        .iter()
        .find(|profile| profile.is_primary)
        .or_else(|| stored.profiles.first());
    let quality_profile_id = primary.map_or(0, |profile| profile.quality_profile_id);
    let file = primary
        .and_then(|profile| profile.file_id)
        .and_then(|file_id| files.get(&file_id))
        .map(file_view);
    let root_folder_path = root_folders
        .get(&model.root_folder_id)
        .cloned()
        .unwrap_or_default();
    let collection = model
        .collection_id
        .and_then(|collection_id| collections.get(&collection_id).cloned());

    MovieView {
        id: model.id,
        tmdb_id: model.tmdb_id,
        imdb_id: model.imdb_id.clone(),
        title: model.title.clone(),
        original_title: model.original_title.clone(),
        sort_title: model.sort_title.clone(),
        year: model.year,
        overview: model.overview.clone(),
        runtime: model.runtime,
        status: model.status.clone(),
        in_cinemas: model.in_cinemas.clone(),
        digital_release: model.digital_release.clone(),
        physical_release: model.physical_release.clone(),
        poster_path: model.poster_path.clone(),
        root_folder_path,
        folder_name: model.folder_name.clone(),
        monitored: model.monitored,
        minimum_availability: model.minimum_availability.clone(),
        added_at: model.added_at.clone(),
        tags: stored.tags.clone(),
        quality_profile_id,
        file,
        collection,
        genres: Vec::new(),
    }
}

async fn load_movie_views(
    state: &AppState,
    movies: &[StoredMovie],
) -> Result<Vec<MovieView>, CompatError> {
    if movies.is_empty() {
        return Ok(Vec::new());
    }

    let primary_file_ids: Vec<i32> = movies
        .iter()
        .filter_map(|movie| {
            movie
                .profiles
                .iter()
                .find(|profile| profile.is_primary)
                .or_else(|| movie.profiles.first())
                .and_then(|profile| profile.file_id)
        })
        .collect();

    let (root_folders, collections, files) = tokio::try_join!(
        db::root_folders::list(&state.db),
        db::collections::list(&state.db),
        db::movies::files_by_ids(&state.db, &primary_file_ids),
    )
    .map_err(db_failed)?;

    let root_folder_map: HashMap<i32, String> = root_folders
        .into_iter()
        .map(|folder| (folder.id, folder.path))
        .collect();
    let collection_map: HashMap<i32, MovieCollectionResource> = collections
        .into_iter()
        .map(|collection| {
            (
                collection.id,
                MovieCollectionResource {
                    title: collection.title,
                    tmdb_id: collection.tmdb_id,
                },
            )
        })
        .collect();

    Ok(movies
        .iter()
        .map(|movie| build_movie_view(movie, &root_folder_map, &collection_map, &files))
        .collect())
}

fn details_to_view(details: &MovieDetails) -> MovieView {
    let summary = &details.summary;
    let dates = details.key_dates();
    let non_empty = |text: &str| (!text.is_empty()).then(|| text.to_owned());
    MovieView {
        id: 0,
        tmdb_id: summary.tmdb_id,
        imdb_id: details.imdb_id.clone(),
        title: summary.title.clone(),
        original_title: non_empty(&summary.original_title),
        sort_title: unseen_core::naming::sort_title(&summary.title),
        year: summary.year().unwrap_or(0),
        overview: non_empty(&summary.overview),
        runtime: details
            .runtime_minutes
            .and_then(|minutes| i32::try_from(minutes).ok()),
        status: details.radarr_status().to_owned(),
        in_cinemas: dates.in_cinemas,
        digital_release: dates.digital,
        physical_release: dates.physical,
        poster_path: summary.poster_path.clone(),
        root_folder_path: String::new(),
        folder_name: String::new(),
        monitored: false,
        minimum_availability: "released".to_owned(),
        added_at: String::new(),
        tags: Vec::new(),
        quality_profile_id: 0,
        file: None,
        collection: details
            .collection
            .as_ref()
            .map(|col| MovieCollectionResource {
                title: col.name.clone(),
                tmdb_id: col.tmdb_id,
            }),
        genres: details.genres.clone(),
    }
}

pub(crate) fn query_param<'a>(params: &'a HashMap<String, String>, name: &str) -> Option<&'a str> {
    params
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

pub(crate) async fn allowed_folders_for_read(
    state: &AppState,
    caller: &ApiKeyCaller,
) -> Result<Option<BTreeSet<i32>>, CompatError> {
    if caller.has_all_libraries(Permission::LibraryRead) {
        return Ok(None);
    }
    let map = db::libraries::root_folder_library_map(&state.db)
        .await
        .map_err(db_failed)?;
    Ok(Some(
        map.into_iter()
            .filter(|&(_, library_id)| {
                caller.allows_in_library(Permission::LibraryRead, library_id)
            })
            .map(|(folder_id, _)| folder_id)
            .collect(),
    ))
}

/// `GET /radarr/api/v3/movie` and `GET /radarr/api/v3/movie?tmdbId={tmdbId}`.
pub async fn list_movies(
    caller: ApiKeyCaller,
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Vec<MovieResource>>, CompatError> {
    let allowed = allowed_folders_for_read(&state, &caller).await?;
    let movies = match query_param(&params, "tmdbId").map(str::trim) {
        Some(raw) if !raw.is_empty() => {
            let tmdb_id = raw
                .parse::<i32>()
                .map_err(|_| CompatError::validation("tmdbId", "must be an integer"))?;
            if tmdb_id <= 0 {
                return Ok(Json(Vec::new()));
            }
            db::movies::find_movie_by_tmdb_id(&state.db, tmdb_id)
                .await
                .map_err(db_failed)?
                .into_iter()
                .collect()
        }
        _ => db::movies::list(&state.db).await.map_err(db_failed)?,
    };
    let movies: Vec<_> = match allowed {
        None => movies,
        Some(allowed) => movies
            .into_iter()
            .filter(|movie| allowed.contains(&movie.model.root_folder_id))
            .collect(),
    };

    let now = db::now();
    let views = load_movie_views(&state, &movies).await?;
    let resources = views
        .iter()
        .map(|view| movie_resource(view, &now))
        .collect();
    Ok(Json(resources))
}

/// `GET /radarr/api/v3/movie/{id}`.
pub async fn get_movie(
    caller: ApiKeyCaller,
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
) -> Result<Json<MovieResource>, CompatError> {
    let id = raw_id
        .parse::<i32>()
        .map_err(|_| CompatError::bad_request("invalid movie id"))?;
    if id <= 0 {
        return Err(CompatError::not_found());
    }
    let stored = db::movies::find(&state.db, id)
        .await
        .map_err(db_failed)?
        .ok_or_else(CompatError::not_found)?;
    if !caller.has_all_libraries(Permission::LibraryRead) {
        let library_id =
            db::libraries::library_of_root_folder(&state.db, stored.model.root_folder_id)
                .await
                .map_err(db_failed)?
                .ok_or_else(CompatError::not_found)?;
        if !caller.allows_in_library(Permission::LibraryRead, library_id) {
            return Err(CompatError::not_found());
        }
    }
    let now = db::now();
    let views = load_movie_views(&state, &[stored]).await?;
    let view = views.first().ok_or_else(CompatError::not_found)?;
    Ok(Json(movie_resource(view, &now)))
}

/// `GET /radarr/api/v3/movie/lookup?term=tmdb:{id}`.
pub async fn lookup_movie(
    caller: ApiKeyCaller,
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Vec<LookupMovieResource>>, CompatError> {
    let raw_term = query_param(&params, "term").unwrap_or_default();
    let term =
        parse_term(raw_term).map_err(|error| CompatError::validation("term", error.to_string()))?;

    let tmdb_id = match term {
        LookupTerm::Tmdb(tmdb_id) => tmdb_id,
        LookupTerm::Text(_) | LookupTerm::Imdb(_) => {
            tracing::warn!(
                method = "GET",
                path = "/radarr/api/v3/movie/lookup",
                "unsupported radarr compat lookup term"
            );
            return Err(CompatError::not_implemented(
                &Method::GET,
                "/radarr/api/v3/movie/lookup",
            ));
        }
    };

    let now = db::now();
    if let Some(stored) = db::movies::find_movie_by_tmdb_id(&state.db, tmdb_id)
        .await
        .map_err(db_failed)?
    {
        let visible = if caller.has_all_libraries(Permission::LibraryRead) {
            true
        } else {
            db::libraries::library_of_root_folder(&state.db, stored.model.root_folder_id)
                .await
                .map_err(db_failed)?
                .is_some_and(|library_id| {
                    caller.allows_in_library(Permission::LibraryRead, library_id)
                })
        };
        if visible {
            let views = load_movie_views(&state, &[stored]).await?;
            let resources = views
                .iter()
                .map(|view| lookup_movie_resource(view, &now))
                .collect();
            return Ok(Json(resources));
        }
    }

    let provider = tmdb::client(&state).await.map_err(|problem| {
        tracing::warn!(code = %problem.code, "radarr compat lookup: tmdb client unavailable");
        CompatError::internal()
    })?;
    match provider.details(tmdb_id).await {
        Ok(details) => {
            let view = details_to_view(&details);
            Ok(Json(vec![lookup_movie_resource(&view, &now)]))
        }
        Err(MetadataError::NotFound) => Ok(Json(Vec::new())),
        Err(error) => {
            tracing::warn!(%error, tmdb_id, "radarr compat lookup: tmdb failed");
            Err(CompatError::internal())
        }
    }
}
