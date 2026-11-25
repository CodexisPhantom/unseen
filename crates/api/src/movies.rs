//! The movies resource (docs/05-api.md, docs/09-filesystem.md): add by TMDB
//! id, edit, tags, quality profiles and files.

use std::path::{Component, Path as FsPath, PathBuf};

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use db::entities::{movie_files, movies};
use db::movies::{Metadata, Movie as Stored, MovieError, NewMovie as StoredNew, Patch};
use serde::{Deserialize, Serialize};
use serde_json::json;
use unseen_core::metadata::MetadataError;
use unseen_core::naming;
use utoipa::{IntoParams, ToSchema};

use crate::AppState;
use crate::access::{LibraryAdd, LibraryDelete, LibraryEdit, LibraryRead, Require};
use crate::audit::{Actor, Changes, ClientIp, Event, record};
use crate::error::Problem;
use crate::tmdb::{client, provider_error};

pub(crate) const AVAILABILITIES: [&str; 3] = ["announced", "inCinemas", "released"];

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MovieProfile {
    /// The id of this attachment, as `DELETE /movies/{id}/profiles/{pid}`
    /// and `primaryProfileId` use it. Not the quality profile id.
    id: i32,
    quality_profile_id: i32,
    is_primary: bool,
    /// The managed file of this profile, if it has one.
    file_id: Option<i32>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Movie {
    id: i32,
    tmdb_id: i32,
    imdb_id: Option<String>,
    title: String,
    original_title: Option<String>,
    sort_title: String,
    year: i32,
    overview: Option<String>,
    /// Minutes.
    runtime: Option<i32>,
    /// `tba`, `announced`, `released` or `deleted`.
    status: String,
    in_cinemas: Option<String>,
    digital_release: Option<String>,
    physical_release: Option<String>,
    /// Path on TMDB's image host; the browser loads the image.
    poster_path: Option<String>,
    /// The collection the movie belongs to.
    collection_id: Option<i32>,
    root_folder_id: i32,
    /// The movie's folder inside the root folder.
    folder_name: String,
    monitored: bool,
    /// `announced`, `inCinemas` or `released`.
    minimum_availability: String,
    tags: Vec<i32>,
    /// The primary profile first.
    profiles: Vec<MovieProfile>,
    added_at: String,
    updated_at: String,
}

impl From<Stored> for Movie {
    fn from(stored: Stored) -> Self {
        let m = stored.model;
        Self {
            id: m.id,
            tmdb_id: m.tmdb_id,
            imdb_id: m.imdb_id,
            title: m.title,
            original_title: m.original_title,
            sort_title: m.sort_title,
            year: m.year,
            overview: m.overview,
            runtime: m.runtime,
            status: m.status,
            in_cinemas: m.in_cinemas,
            digital_release: m.digital_release,
            physical_release: m.physical_release,
            poster_path: m.poster_path,
            collection_id: m.collection_id,
            root_folder_id: m.root_folder_id,
            folder_name: m.folder_name,
            monitored: m.monitored,
            minimum_availability: m.minimum_availability,
            tags: stored.tags,
            profiles: stored
                .profiles
                .into_iter()
                .map(|p| MovieProfile {
                    id: p.id,
                    quality_profile_id: p.quality_profile_id,
                    is_primary: p.is_primary,
                    file_id: p.file_id,
                })
                .collect(),
            added_at: m.added_at,
            updated_at: m.updated_at,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_availability() -> String {
    "released".to_owned()
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewMovie {
    tmdb_id: i32,
    root_folder_id: i32,
    /// One or more quality profiles. The first is the primary profile.
    quality_profile_ids: Vec<i32>,
    #[serde(default = "default_true")]
    monitored: bool,
    #[serde(default = "default_availability")]
    minimum_availability: String,
    #[serde(default)]
    tags: Vec<i32>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MoviePatch {
    monitored: Option<bool>,
    minimum_availability: Option<String>,
    /// Replaces the whole tag set.
    tags: Option<Vec<i32>>,
    /// Makes this profile attachment (`profiles[].id`) the primary one.
    primary_profile_id: Option<i32>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewMovieProfile {
    quality_profile_id: i32,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(rename_all = "camelCase")]
pub struct DeleteParams {
    /// Also delete the movie's folder on disk. Default `false`.
    delete_files: Option<bool>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MovieFile {
    id: i32,
    /// Relative to the movie's folder.
    relative_path: String,
    /// Bytes.
    size: i64,
    quality_id: i32,
    release_group: Option<String>,
    edition: Option<String>,
    scene_name: Option<String>,
    /// The profile this file belongs to. Absent for an unmanaged file.
    movie_profile_id: Option<i32>,
    managed: bool,
    added_at: String,
}

impl From<movie_files::Model> for MovieFile {
    fn from(file: movie_files::Model) -> Self {
        Self {
            id: file.id,
            relative_path: file.relative_path,
            size: file.size,
            quality_id: file.quality_id,
            release_group: file.release_group,
            edition: file.edition,
            scene_name: file.scene_name,
            managed: file.movie_profile_id.is_some(),
            movie_profile_id: file.movie_profile_id,
            added_at: file.added_at,
        }
    }
}

fn failed(error: impl std::fmt::Display) -> Problem {
    tracing::error!(%error, "movies");
    Problem::internal()
}

fn movie_error(error: MovieError) -> Problem {
    match error {
        MovieError::NotFound => Problem::not_found(),
        MovieError::TmdbTaken => Problem::conflict("movie_exists", "The movie is already added"),
        MovieError::FolderTaken => Problem::conflict(
            "folder_taken",
            "Another movie in this root folder has the same folder name",
        ),
        MovieError::UnknownReference => {
            Problem::validation("primaryProfileId", "must be a profile of this movie")
        }
        MovieError::ProfileTaken => Problem::conflict(
            "profile_taken",
            "The movie has this quality profile already",
        ),
        MovieError::PrimaryProfile => Problem::conflict(
            "primary_profile",
            "Make another profile primary before removing the primary profile",
        ),
        MovieError::LastProfile => {
            Problem::conflict("last_profile", "A movie keeps at least one profile")
        }
        MovieError::Db(error) => failed(error),
    }
}

async fn load(state: &AppState, id: i32) -> Result<Stored, Problem> {
    db::movies::find(&state.db, id)
        .await
        .map_err(failed)?
        .ok_or_else(Problem::not_found)
}

pub(crate) fn check_availability(value: &str) -> Result<(), Problem> {
    if AVAILABILITIES.contains(&value) {
        Ok(())
    } else {
        Err(Problem::validation(
            "minimumAvailability",
            "must be `announced`, `inCinemas` or `released`",
        ))
    }
}

pub(crate) async fn check_tags(state: &AppState, ids: &[i32]) -> Result<(), Problem> {
    for id in ids {
        if db::tags::find(&state.db, *id)
            .await
            .map_err(failed)?
            .is_none()
        {
            return Err(Problem::validation("tags", "must be ids of existing tags"));
        }
    }
    Ok(())
}

pub(crate) async fn check_profile(state: &AppState, id: i32, field: &str) -> Result<(), Problem> {
    if db::profiles::find(&state.db, id)
        .await
        .map_err(failed)?
        .is_none()
    {
        return Err(Problem::validation(
            field,
            "must be ids of existing quality profiles",
        ));
    }
    Ok(())
}

/// The folder of a movie on disk, or `None` when the stored name would lead
/// out of the root folder.
fn movie_dir(root: &str, folder_name: &str) -> Option<PathBuf> {
    let mut parts = FsPath::new(folder_name).components();
    match (parts.next(), parts.next()) {
        (Some(Component::Normal(_)), None) => Some(FsPath::new(root).join(folder_name)),
        _ => None,
    }
}

/// A file below the movie's folder, or `None` for a path that is absolute or
/// climbs out.
fn movie_file_path(dir: &FsPath, relative: &str) -> Option<PathBuf> {
    let parts = FsPath::new(relative).components();
    let inside = parts.clone().next().is_some()
        && parts
            .into_iter()
            .all(|part| matches!(part, Component::Normal(_)));
    inside.then(|| dir.join(relative))
}

/// Every movie, by sort title.
#[utoipa::path(
    get,
    path = "/api/v1/movies",
    tag = "movies",
    responses(
        (status = 200, description = "The library", body = Vec<Movie>),
        (status = 403, description = "Permission denied", body = Problem),
    )
)]
pub async fn list_movies(
    _: Require<LibraryRead>,
    State(state): State<AppState>,
) -> Result<Json<Vec<Movie>>, Problem> {
    let movies = db::movies::list(&state.db).await.map_err(failed)?;
    Ok(Json(movies.into_iter().map(Movie::from).collect()))
}

/// One movie.
#[utoipa::path(
    get,
    path = "/api/v1/movies/{id}",
    tag = "movies",
    params(("id" = i32, Path, description = "Movie id")),
    responses(
        (status = 200, description = "The movie", body = Movie),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such movie", body = Problem),
    )
)]
pub async fn get_movie(
    _: Require<LibraryRead>,
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<Movie>, Problem> {
    Ok(Json(load(&state, id).await?.into()))
}

/// Adds a movie by TMDB id. Unseen fetches the metadata and names the movie
/// folder with the default template `{Movie Title} ({Release Year})`.
#[utoipa::path(
    post,
    path = "/api/v1/movies",
    tag = "movies",
    request_body = NewMovie,
    responses(
        (status = 201, description = "The new movie", body = Movie),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 409, description = "The movie is already added (`movie_exists`), its folder name is taken (`folder_taken`) or the TMDB key is not set (`tmdb_not_configured`)", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
        (status = 502, description = "TMDB failed or refused the key", body = Problem),
    )
)]
pub async fn create_movie(
    caller: Require<LibraryAdd>,
    State(state): State<AppState>,
    ip: ClientIp,
    Json(body): Json<NewMovie>,
) -> Result<(StatusCode, Json<Movie>), Problem> {
    if body.quality_profile_ids.is_empty() {
        return Err(Problem::validation(
            "qualityProfileIds",
            "must hold at least one quality profile",
        ));
    }
    check_availability(&body.minimum_availability)?;
    let root = db::root_folders::find(&state.db, body.root_folder_id)
        .await
        .map_err(failed)?
        .ok_or_else(|| Problem::validation("rootFolderId", "must be the id of a root folder"))?;
    for id in &body.quality_profile_ids {
        check_profile(&state, *id, "qualityProfileIds").await?;
    }
    check_tags(&state, &body.tags).await?;
    if db::movies::find_by_tmdb_id(&state.db, body.tmdb_id)
        .await
        .map_err(failed)?
        .is_some()
    {
        return Err(movie_error(MovieError::TmdbTaken));
    }

    let provider = client(&state).await?;
    let details = match provider.details(body.tmdb_id).await {
        Ok(details) => details,
        Err(MetadataError::NotFound) => {
            return Err(Problem::validation("tmdbId", "is not a movie on TMDB"));
        }
        Err(error) => return Err(provider_error(error)),
    };
    let metadata = Metadata::from_details(&details).ok_or_else(|| {
        Problem::validation("tmdbId", "has no release date on TMDB yet, so no year")
    })?;
    let folder_name = naming::movie_folder_name(&metadata.title, metadata.year);
    let collection_id = match &details.collection {
        Some(reference) => Some(
            db::collections::ensure(&state.db, provider.as_ref(), reference)
                .await
                .map_err(failed)?,
        ),
        None => None,
    };
    let created = db::movies::create(
        &state.db,
        StoredNew {
            metadata,
            root_folder_id: root.id,
            folder_name,
            monitored: body.monitored,
            minimum_availability: body.minimum_availability,
            collection_id,
        },
        &body.quality_profile_ids,
        &body.tags,
    )
    .await
    .map_err(movie_error)?;
    let event = Event::new("movie.created")
        .target("movie", created.model.id)
        .data(json!({ "title": created.model.title, "tmdbId": created.model.tmdb_id }));
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok((StatusCode::CREATED, Json(created.into())))
}

/// Changes the monitored state, the minimum availability, the tags or the
/// primary profile of a movie.
#[utoipa::path(
    patch,
    path = "/api/v1/movies/{id}",
    tag = "movies",
    params(("id" = i32, Path, description = "Movie id")),
    request_body = MoviePatch,
    responses(
        (status = 200, description = "The changed movie", body = Movie),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such movie", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
    )
)]
pub async fn update_movie(
    caller: Require<LibraryEdit>,
    State(state): State<AppState>,
    ip: ClientIp,
    Path(id): Path<i32>,
    Json(body): Json<MoviePatch>,
) -> Result<Json<Movie>, Problem> {
    if let Some(value) = &body.minimum_availability {
        check_availability(value)?;
    }
    if let Some(tags) = &body.tags {
        check_tags(&state, tags).await?;
    }
    let before = Movie::from(load(&state, id).await?);
    let updated = Movie::from(
        db::movies::update(
            &state.db,
            id,
            Patch {
                monitored: body.monitored,
                minimum_availability: body.minimum_availability,
                tags: body.tags,
                primary_profile_id: body.primary_profile_id,
            },
        )
        .await
        .map_err(movie_error)?,
    );
    let primary = |movie: &Movie| movie.profiles.first().map(|p| p.id);
    let changes = Changes::new()
        .field("monitored", before.monitored, updated.monitored)
        .field(
            "minimumAvailability",
            &before.minimum_availability,
            &updated.minimum_availability,
        )
        .field("tags", &before.tags, &updated.tags)
        .field("primaryProfileId", primary(&before), primary(&updated));
    let event = Event::new("movie.updated")
        .target("movie", id)
        .data(changes.into_data());
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok(Json(updated))
}

async fn root_path(state: &AppState, movie: &movies::Model) -> Result<String, Problem> {
    Ok(db::root_folders::find(&state.db, movie.root_folder_id)
        .await
        .map_err(failed)?
        .ok_or_else(|| failed("movie without root folder"))?
        .path)
}

/// Runs a disk removal off the async workers. A path that is already gone
/// counts as removed.
async fn remove_on_disk(
    job: impl FnOnce() -> std::io::Result<()> + Send + 'static,
) -> Result<(), Problem> {
    let result = tokio::task::spawn_blocking(move || match job() {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    })
    .await
    .map_err(failed)?;
    result.map_err(failed)
}

/// Removes a movie from the library. With `deleteFiles=true` the movie's
/// folder is deleted from disk too; by default the folder stays untouched.
#[utoipa::path(
    delete,
    path = "/api/v1/movies/{id}",
    tag = "movies",
    params(("id" = i32, Path, description = "Movie id"), DeleteParams),
    responses(
        (status = 204, description = "Deleted"),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such movie", body = Problem),
    )
)]
pub async fn delete_movie(
    caller: Require<LibraryDelete>,
    State(state): State<AppState>,
    ip: ClientIp,
    Path(id): Path<i32>,
    Query(params): Query<DeleteParams>,
) -> Result<StatusCode, Problem> {
    let movie = load(&state, id).await?.model;
    let delete_files = params.delete_files.unwrap_or(false);
    if delete_files {
        let root = root_path(&state, &movie).await?;
        let dir = movie_dir(&root, &movie.folder_name)
            .ok_or_else(|| failed(format!("unsafe folder name {:?}", movie.folder_name)))?;
        remove_on_disk(move || std::fs::remove_dir_all(dir)).await?;
    }
    db::movies::delete(&state.db, id)
        .await
        .map_err(movie_error)?;
    let event = Event::new("movie.deleted")
        .target("movie", id)
        .data(json!({ "title": movie.title, "deleteFiles": delete_files }));
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok(StatusCode::NO_CONTENT)
}

/// Attaches one more quality profile to a movie. It is not primary.
#[utoipa::path(
    post,
    path = "/api/v1/movies/{id}/profiles",
    tag = "movies",
    params(("id" = i32, Path, description = "Movie id")),
    request_body = NewMovieProfile,
    responses(
        (status = 201, description = "The movie with the new profile", body = Movie),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such movie", body = Problem),
        (status = 409, description = "The movie has this profile already", body = Problem),
        (status = 422, description = "No such quality profile", body = Problem),
    )
)]
pub async fn add_profile(
    caller: Require<LibraryAdd>,
    State(state): State<AppState>,
    ip: ClientIp,
    Path(id): Path<i32>,
    Json(body): Json<NewMovieProfile>,
) -> Result<(StatusCode, Json<Movie>), Problem> {
    load(&state, id).await?;
    check_profile(&state, body.quality_profile_id, "qualityProfileId").await?;
    let updated = db::movies::add_profile(&state.db, id, body.quality_profile_id)
        .await
        .map_err(movie_error)?;
    let event = Event::new("movie.profile_added")
        .target("movie", id)
        .data(json!({ "qualityProfileId": body.quality_profile_id }));
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok((StatusCode::CREATED, Json(updated.into())))
}

/// Detaches a quality profile. Its file stays as an unmanaged file. The
/// primary profile cannot be removed while another exists.
#[utoipa::path(
    delete,
    path = "/api/v1/movies/{id}/profiles/{pid}",
    tag = "movies",
    params(
        ("id" = i32, Path, description = "Movie id"),
        ("pid" = i32, Path, description = "Profile attachment id (`profiles[].id`)"),
    ),
    responses(
        (status = 204, description = "Removed"),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such movie or profile", body = Problem),
        (status = 409, description = "The profile is primary (`primary_profile`) or the last one (`last_profile`)", body = Problem),
    )
)]
pub async fn remove_profile(
    caller: Require<LibraryDelete>,
    State(state): State<AppState>,
    ip: ClientIp,
    Path((id, pid)): Path<(i32, i32)>,
) -> Result<StatusCode, Problem> {
    load(&state, id).await?;
    db::movies::remove_profile(&state.db, id, pid)
        .await
        .map_err(movie_error)?;
    let event = Event::new("movie.profile_removed")
        .target("movie", id)
        .data(json!({ "movieProfileId": pid }));
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok(StatusCode::NO_CONTENT)
}

/// The files of a movie, managed and unmanaged.
#[utoipa::path(
    get,
    path = "/api/v1/movies/{id}/files",
    tag = "movies",
    params(("id" = i32, Path, description = "Movie id")),
    responses(
        (status = 200, description = "The files", body = Vec<MovieFile>),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such movie", body = Problem),
    )
)]
pub async fn list_files(
    _: Require<LibraryRead>,
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<Vec<MovieFile>>, Problem> {
    load(&state, id).await?;
    let files = db::movies::files_of(&state.db, id).await.map_err(failed)?;
    Ok(Json(files.into_iter().map(MovieFile::from).collect()))
}

/// Deletes a file of a movie from disk and from the library.
#[utoipa::path(
    delete,
    path = "/api/v1/movies/{id}/files/{fid}",
    tag = "movies",
    params(
        ("id" = i32, Path, description = "Movie id"),
        ("fid" = i32, Path, description = "File id"),
    ),
    responses(
        (status = 204, description = "Deleted"),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such movie or file", body = Problem),
    )
)]
pub async fn delete_file(
    caller: Require<LibraryDelete>,
    State(state): State<AppState>,
    ip: ClientIp,
    Path((id, fid)): Path<(i32, i32)>,
) -> Result<StatusCode, Problem> {
    let movie = load(&state, id).await?.model;
    let file = db::movies::find_file(&state.db, id, fid)
        .await
        .map_err(failed)?
        .ok_or_else(Problem::not_found)?;
    let root = root_path(&state, &movie).await?;
    let path = movie_dir(&root, &movie.folder_name)
        .and_then(|dir| movie_file_path(&dir, &file.relative_path))
        .ok_or_else(|| failed(format!("unsafe file path {:?}", file.relative_path)))?;
    remove_on_disk(move || std::fs::remove_file(path)).await?;
    db::movies::delete_file(&state.db, fid)
        .await
        .map_err(failed)?;
    let event = Event::new("movie.file_deleted")
        .target("movie", id)
        .data(json!({ "relativePath": file.relative_path }));
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_plain_folder_name_stays_inside_the_root() {
        assert!(movie_dir("/m", "Dune (2021)").is_some());
        for bad in ["", "..", "../x", "a/b", "/abs"] {
            assert!(movie_dir("/m", bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn only_a_relative_path_without_dots_stays_inside_the_folder() {
        let dir = FsPath::new("/m/Dune (2021)");
        assert!(movie_file_path(dir, "Dune.mkv").is_some());
        assert!(movie_file_path(dir, "extras/Dune.mkv").is_some());
        for bad in ["", "../x.mkv", "/etc/passwd", "a/../../b", ".."] {
            assert!(movie_file_path(dir, bad).is_none(), "{bad}");
        }
    }
}
