//! Adopting an existing library in place (docs/09-filesystem.md). Scan lists
//! the sub-folders of a root folder that no movie uses and proposes a TMDB
//! match for each; adopt stores the movies the user confirmed with their
//! existing folder names and files. Nothing on disk is moved, renamed or
//! deleted.

use std::collections::HashSet;
use std::path::{Component, Path as FsPath, PathBuf};
use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use db::movies::{Metadata, MovieError, NewFile, NewMovie as StoredNew};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::task::JoinSet;
use unseen_core::backfill::file_quality;
use unseen_core::library::{
    FolderGuess, confident_match, guess_folder, is_extras_folder, is_movie_video,
};
use unseen_core::metadata::{MetadataError, MetadataProvider, MovieSummary};
use unseen_core::permission::Permission;
use unseen_core::root_folder;
use utoipa::ToSchema;

use crate::AppState;
use crate::access::{Caller, ImportRun, Require};
use crate::audit::{Actor, ClientIp, Event, record};
use crate::error::Problem;
use crate::tmdb::{client, provider_error};

/// TMDB calls in flight at once.
const PARALLEL: usize = 8;
/// How deep below the movie folder a video file is looked for.
const DEPTH: usize = 3;
const AVAILABILITIES: [&str; 3] = ["announced", "inCinemas", "released"];

fn failed(error: impl std::fmt::Display) -> Problem {
    tracing::error!(%error, "library");
    Problem::internal()
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScanRequest {
    root_folder_id: i32,
}

/// A video file of a folder.
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScanFile {
    /// Relative to the movie folder.
    relative_path: String,
    size: i64,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum ScanStatus {
    /// One confident TMDB match; ready to adopt.
    Matched,
    /// No confident match. Unseen does not guess.
    Unmatched,
    /// The match is a movie that the library has already, or that another
    /// folder of this scan matched first.
    Duplicate,
    /// The folder holds no movie video file.
    NoVideo,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScanFolder {
    folder_name: String,
    /// What the folder name says.
    title: String,
    year: Option<i32>,
    status: ScanStatus,
    /// The match, for `matched` and `duplicate`.
    tmdb_id: Option<i32>,
    match_title: Option<String>,
    match_year: Option<i32>,
    poster_path: Option<String>,
    /// Video files, the largest first. The largest is the main file.
    files: Vec<ScanFile>,
}

#[derive(Serialize, ToSchema)]
pub struct ScanResult {
    folders: Vec<ScanFolder>,
}

struct DiskFile {
    relative_path: String,
    size: u64,
}

/// The movie video files below `folder`, largest first. Skips extras
/// folders, samples and anything that is not a video. Does not follow
/// directory links.
fn video_files(folder: &FsPath) -> std::io::Result<Vec<DiskFile>> {
    fn walk(
        dir: &FsPath,
        prefix: &str,
        depth: usize,
        out: &mut Vec<DiskFile>,
    ) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            let relative = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            if entry.file_type()?.is_dir() {
                if depth < DEPTH && !is_extras_folder(&name) {
                    walk(&entry.path(), &relative, depth + 1, out)?;
                }
            } else if is_movie_video(&name)
                && let Ok(metadata) = entry.metadata()
                && metadata.is_file()
            {
                out.push(DiskFile {
                    relative_path: relative,
                    size: metadata.len(),
                });
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(folder, "", 1, &mut files)?;
    files.sort_by(|a, b| {
        b.size
            .cmp(&a.size)
            .then_with(|| a.relative_path.cmp(&b.relative_path))
    });
    Ok(files)
}

fn list_dir_names(path: &FsPath) -> std::io::Result<Vec<String>> {
    Ok(std::fs::read_dir(path)?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect())
}

/// Runs `job` for every input, at most [`PARALLEL`] at a time, and returns
/// the results in input order.
async fn in_parallel<I, T, F, Fut>(inputs: Vec<I>, job: F) -> Vec<T>
where
    I: Send + 'static,
    T: Send + 'static,
    F: Fn(I) -> Fut,
    Fut: Future<Output = T> + Send + 'static,
{
    let total = inputs.len();
    let mut results: Vec<Option<T>> = (0..total).map(|_| None).collect();
    let mut running = JoinSet::new();
    let mut collect = |done: Result<(usize, T), tokio::task::JoinError>| {
        if let Ok((index, value)) = done {
            results[index] = Some(value);
        }
    };
    for (index, input) in inputs.into_iter().enumerate() {
        if running.len() >= PARALLEL
            && let Some(done) = running.join_next().await
        {
            collect(done);
        }
        let future = job(input);
        running.spawn(async move { (index, future.await) });
    }
    while let Some(done) = running.join_next().await {
        collect(done);
    }
    results.into_iter().flatten().collect()
}

/// The TMDB movie for a folder: the id tag, else the IMDb tag, else a
/// confident text match.
async fn resolve(
    tmdb: &dyn MetadataProvider,
    guess: &FolderGuess,
) -> Result<Option<MovieSummary>, MetadataError> {
    if let Some(id) = guess.tmdb_id {
        match tmdb.details(id).await {
            Ok(details) => return Ok(Some(details.summary)),
            Err(MetadataError::NotFound) => {}
            Err(error) => return Err(error),
        }
    }
    if let Some(imdb) = &guess.imdb_id {
        let mut found = tmdb.find_by_imdb_id(imdb).await?;
        if found.len() == 1 {
            return Ok(found.pop());
        }
    }
    if guess.title.is_empty() {
        return Ok(None);
    }
    let results = tmdb.search(&guess.title).await?;
    Ok(confident_match(guess, &results).cloned())
}

async fn root_path(state: &AppState, caller: &Caller, id: i32) -> Result<PathBuf, Problem> {
    let root = db::root_folders::find(&state.db, id)
        .await
        .map_err(failed)?
        .ok_or_else(|| Problem::validation("rootFolderId", "must be the id of a root folder"))?;
    if !caller.allows_in_library(Permission::ImportRun, root.library_id) {
        return Err(Problem::not_found());
    }
    Ok(PathBuf::from(root.path))
}

/// Lists the folders of a root folder that no movie uses, with a proposed
/// TMDB match and the video files of each. Changes nothing, so it can run
/// again and again: adopted folders no longer appear.
#[utoipa::path(
    post,
    path = "/api/v1/library/scan",
    tag = "library",
    request_body = ScanRequest,
    responses(
        (status = 200, description = "The folders no movie uses", body = ScanResult),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 409, description = "The TMDB key is not set (`tmdb_not_configured`)", body = Problem),
        (status = 422, description = "No such root folder, or it cannot be read", body = Problem),
        (status = 502, description = "TMDB failed or refused the key", body = Problem),
    )
)]
pub async fn scan(
    caller: Require<ImportRun>,
    State(state): State<AppState>,
    Json(body): Json<ScanRequest>,
) -> Result<Json<ScanResult>, Problem> {
    let root = root_path(&state, &caller, body.root_folder_id).await?;
    let known = db::root_folders::movie_folder_names(&state.db, body.root_folder_id)
        .await
        .map_err(failed)?;
    let mut taken: HashSet<i32> = db::movies::tmdb_ids(&state.db)
        .await
        .map_err(failed)?
        .into_iter()
        .collect();
    let tmdb: Arc<dyn MetadataProvider> = Arc::from(client(&state).await?);

    let found = tokio::task::spawn_blocking(move || {
        let names = list_dir_names(&root)?;
        let folders: Vec<_> = root_folder::unmapped(names, &known)
            .into_iter()
            .map(|name| {
                let files = video_files(&root.join(&name)).unwrap_or_else(|error| {
                    tracing::warn!(%error, folder = %name, "library scan");
                    Vec::new()
                });
                (name, files)
            })
            .collect();
        std::io::Result::Ok(folders)
    })
    .await
    .map_err(failed)?
    .map_err(|_| Problem::validation("rootFolderId", "cannot be read"))?;

    // Folders without a video need no TMDB call.
    let lookups: Vec<_> = found
        .iter()
        .map(|(name, files)| (!files.is_empty()).then(|| guess_folder(name)))
        .collect();
    let provider = tmdb.clone();
    let resolved = in_parallel(lookups.clone(), move |guess| {
        let provider = provider.clone();
        async move {
            match guess {
                Some(guess) => resolve(&*provider, &guess).await,
                None => Ok(None),
            }
        }
    })
    .await;

    let mut folders = Vec::with_capacity(found.len());
    for (((name, files), guess), resolved) in found.into_iter().zip(lookups).zip(resolved) {
        let guess = guess.unwrap_or_else(|| guess_folder(&name));
        let matched = resolved.map_err(provider_error)?;
        let status = match (&matched, files.is_empty()) {
            (_, true) => ScanStatus::NoVideo,
            (None, _) => ScanStatus::Unmatched,
            (Some(movie), _) if !taken.insert(movie.tmdb_id) => ScanStatus::Duplicate,
            _ => ScanStatus::Matched,
        };
        folders.push(ScanFolder {
            folder_name: name,
            title: guess.title,
            year: guess.year,
            status,
            tmdb_id: matched.as_ref().map(|m| m.tmdb_id),
            match_title: matched.as_ref().map(|m| m.title.clone()),
            match_year: matched.as_ref().and_then(MovieSummary::year),
            poster_path: matched.and_then(|m| m.poster_path),
            files: files
                .into_iter()
                .map(|file| ScanFile {
                    relative_path: file.relative_path,
                    size: i64::try_from(file.size).unwrap_or(i64::MAX),
                })
                .collect(),
        });
    }
    Ok(Json(ScanResult { folders }))
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdoptItem {
    /// A sub-folder of the root folder.
    folder_name: String,
    tmdb_id: i32,
}

fn default_true() -> bool {
    true
}

fn default_availability() -> String {
    "released".to_owned()
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdoptRequest {
    root_folder_id: i32,
    /// The first is the primary profile; it gets the largest file.
    quality_profile_ids: Vec<i32>,
    #[serde(default = "default_true")]
    monitored: bool,
    /// `announced`, `inCinemas` or `released`.
    #[serde(default = "default_availability")]
    minimum_availability: String,
    #[serde(default)]
    tags: Vec<i32>,
    movies: Vec<AdoptItem>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Adopted {
    folder_name: String,
    movie_id: i32,
    /// Video files stored: the main file and the unmanaged ones.
    file_count: usize,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdoptFailure {
    folder_name: String,
    /// `folder_not_found`, `no_video_file`, `movie_exists`, `folder_taken`,
    /// `tmdb_not_found`, `no_release_date` or `tmdb_unavailable`.
    code: String,
    message: String,
}

#[derive(Serialize, ToSchema)]
pub struct AdoptResult {
    adopted: Vec<Adopted>,
    /// Folders that were not adopted. The others are adopted anyway.
    failed: Vec<AdoptFailure>,
}

fn failure(folder_name: &str, code: &str, message: &str) -> AdoptFailure {
    AdoptFailure {
        folder_name: folder_name.to_owned(),
        code: code.to_owned(),
        message: message.to_owned(),
    }
}

/// A folder name that is one plain component, so it cannot leave the root.
fn plain_folder(name: &str) -> bool {
    let mut parts = FsPath::new(name).components();
    matches!(parts.next(), Some(Component::Normal(_))) && parts.next().is_none()
}

/// Adopts the confirmed folders: each becomes a movie that keeps its folder
/// name and file names. The largest video file belongs to the primary
/// profile; other video files are stored as unmanaged files. The quality of
/// each file is read from its file name, then from the folder name, and is
/// `Unknown` when neither says. A folder that cannot be adopted is reported
/// and the others still are.
#[utoipa::path(
    post,
    path = "/api/v1/library/adopt",
    tag = "library",
    request_body = AdoptRequest,
    responses(
        (status = 200, description = "What was adopted and what was not", body = AdoptResult),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 409, description = "The TMDB key is not set (`tmdb_not_configured`)", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
    )
)]
pub async fn adopt(
    caller: Require<ImportRun>,
    State(state): State<AppState>,
    ip: ClientIp,
    Json(body): Json<AdoptRequest>,
) -> Result<Json<AdoptResult>, Problem> {
    if body.quality_profile_ids.is_empty() {
        return Err(Problem::validation(
            "qualityProfileIds",
            "must hold at least one quality profile",
        ));
    }
    if !AVAILABILITIES.contains(&body.minimum_availability.as_str()) {
        return Err(Problem::validation(
            "minimumAvailability",
            "must be `announced`, `inCinemas` or `released`",
        ));
    }
    let root = root_path(&state, &caller, body.root_folder_id).await?;
    for id in &body.quality_profile_ids {
        crate::movies::check_profile(&state, *id, "qualityProfileIds").await?;
    }
    crate::movies::check_tags(&state, &body.tags).await?;
    let tmdb: Arc<dyn MetadataProvider> = Arc::from(client(&state).await?);

    // Read the disk again: the client's view may be old, and its folder
    // names are untrusted.
    let mut failed_items = Vec::new();
    let mut items = Vec::new();
    for item in body.movies {
        if !plain_folder(&item.folder_name) {
            failed_items.push(failure(
                &item.folder_name,
                "folder_not_found",
                "The folder is not a sub-folder of the root folder",
            ));
        } else {
            items.push(item);
        }
    }
    let disk_root = root.clone();
    let on_disk = tokio::task::spawn_blocking(move || {
        items
            .into_iter()
            .map(|item| {
                let folder = disk_root.join(&item.folder_name);
                let files = folder
                    .is_dir()
                    .then(|| video_files(&folder).unwrap_or_default());
                (item, files)
            })
            .collect::<Vec<_>>()
    })
    .await
    .map_err(failed)?;

    let mut ready = Vec::new();
    for (item, files) in on_disk {
        match files {
            None => failed_items.push(failure(
                &item.folder_name,
                "folder_not_found",
                "The folder does not exist in the root folder",
            )),
            Some(files) if files.is_empty() => failed_items.push(failure(
                &item.folder_name,
                "no_video_file",
                "The folder holds no movie video file",
            )),
            Some(files) => ready.push((item, files)),
        }
    }

    let provider = tmdb.clone();
    let details = in_parallel(
        ready.iter().map(|(item, _)| item.tmdb_id).collect(),
        move |id: i32| {
            let provider = provider.clone();
            async move { provider.details(id).await }
        },
    )
    .await;

    let actor = Actor::of(&state, &caller).await;
    let mut adopted = Vec::new();
    for ((item, files), details) in ready.into_iter().zip(details) {
        let (metadata, collection) = match details {
            Ok(details) => match Metadata::from_details(&details) {
                Some(metadata) => (metadata, details.collection),
                None => {
                    failed_items.push(failure(
                        &item.folder_name,
                        "no_release_date",
                        "The movie has no release date on TMDB yet, so no year",
                    ));
                    continue;
                }
            },
            Err(MetadataError::NotFound) => {
                failed_items.push(failure(
                    &item.folder_name,
                    "tmdb_not_found",
                    "The TMDB id is not a movie",
                ));
                continue;
            }
            Err(error) => {
                tracing::warn!(%error, "library adopt");
                failed_items.push(failure(
                    &item.folder_name,
                    "tmdb_unavailable",
                    "TMDB failed or refused the key",
                ));
                continue;
            }
        };
        let collection_id = match &collection {
            Some(reference) => Some(
                db::collections::ensure(&state.db, tmdb.as_ref(), reference)
                    .await
                    .map_err(failed)?,
            ),
            None => None,
        };
        let stored: Vec<NewFile> = files
            .iter()
            .map(|file| NewFile {
                relative_path: file.relative_path.clone(),
                size: i64::try_from(file.size).unwrap_or(i64::MAX),
                quality_id: file_quality(&file.relative_path, &item.folder_name).id(),
            })
            .collect();
        let created = db::movies::create_with_files(
            &state.db,
            StoredNew {
                metadata,
                root_folder_id: body.root_folder_id,
                folder_name: item.folder_name.clone(),
                monitored: body.monitored,
                minimum_availability: body.minimum_availability.clone(),
                collection_id,
            },
            &body.quality_profile_ids,
            &body.tags,
            &stored,
        )
        .await;
        match created {
            Ok(movie) => {
                let event = Event::new("movie.adopted")
                    .target("movie", movie.model.id)
                    .data(json!({
                        "title": movie.model.title,
                        "tmdbId": movie.model.tmdb_id,
                        "folderName": item.folder_name,
                    }));
                record(&state, &actor, &ip, event).await;
                // Subtitle files next to the videos are recorded too. A
                // failure is logged; the movie is adopted either way.
                let _ = crate::subtitles::scan_movie(&state, &movie.model).await;
                adopted.push(Adopted {
                    folder_name: item.folder_name,
                    movie_id: movie.model.id,
                    file_count: stored.len(),
                });
            }
            Err(MovieError::TmdbTaken) => failed_items.push(failure(
                &item.folder_name,
                "movie_exists",
                "The movie is already in the library",
            )),
            Err(MovieError::FolderTaken) => failed_items.push(failure(
                &item.folder_name,
                "folder_taken",
                "Another movie in this root folder has this folder name",
            )),
            Err(error) => return Err(failed(format!("{error:?}"))),
        }
    }
    Ok(Json(AdoptResult {
        adopted,
        failed: failed_items,
    }))
}
