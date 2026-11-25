//! Root folders and the server-side folder browser (docs/09-filesystem.md).

use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use axum::Json;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::StatusCode;
use db::entities::root_folders;
use db::root_folders::RootFolderError;
use serde::{Deserialize, Serialize};
use serde_json::json;
use unseen_core::root_folder::{self, Overlap};
use utoipa::{IntoParams, ToSchema};

use crate::AppState;
use crate::access::{Require, SettingsManage, SettingsRead};
use crate::audit::{Actor, ClientIp, Event, record};
use crate::error::Problem;

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RootFolder {
    id: i32,
    path: String,
    /// False when the folder is missing or cannot be read right now.
    accessible: bool,
    /// Bytes available to Unseen on the filesystem of the folder. Absent when
    /// the folder is not accessible.
    free_space: Option<u64>,
    /// Sub-folders on disk that no movie uses, sorted.
    unmapped_folders: Vec<String>,
    created_at: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewRootFolder {
    /// An absolute path on the server. It must exist, be a directory that
    /// Unseen can write to, and not lie inside or contain another root folder.
    path: String,
}

#[derive(Serialize, ToSchema)]
pub struct BrowserFolder {
    name: String,
    path: String,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FolderListing {
    /// The listed folder. Empty for the top level (the drives on Windows).
    path: String,
    /// The folder above, `None` at the top level.
    parent: Option<String>,
    /// The sub-folders, by name.
    folders: Vec<BrowserFolder>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct BrowseParams {
    /// Absolute path of the folder to list. Empty or absent lists the top level.
    path: Option<String>,
}

fn failed(error: impl std::fmt::Display) -> Problem {
    tracing::error!(%error, "root folders");
    Problem::internal()
}

fn root_folder_error(error: RootFolderError) -> Problem {
    match error {
        RootFolderError::NotFound => Problem::not_found(),
        RootFolderError::PathTaken => {
            Problem::conflict("path_taken", "This folder is already a root folder")
        }
        RootFolderError::InUse => Problem::conflict(
            "root_folder_in_use",
            "A movie lives in this root folder; remove the movies first",
        ),
        RootFolderError::Db(error) => failed(error),
    }
}

/// Runs blocking disk access off the async workers.
async fn blocking<T: Send + 'static>(
    job: impl FnOnce() -> T + Send + 'static,
) -> Result<T, Problem> {
    tokio::task::spawn_blocking(job).await.map_err(failed)
}

fn describe(model: root_folders::Model, known: &[String]) -> RootFolder {
    let path = Path::new(&model.path);
    let on_disk = list_dirs(path);
    RootFolder {
        id: model.id,
        accessible: on_disk.is_ok(),
        free_space: fs4::available_space(path).ok(),
        unmapped_folders: root_folder::unmapped(
            on_disk
                .unwrap_or_default()
                .into_iter()
                .map(|(name, _)| name),
            known,
        ),
        path: model.path,
        created_at: model.created_at,
    }
}

/// The sub-directories of `path` as (name, full path), in disk order.
fn list_dirs(path: &Path) -> std::io::Result<Vec<(String, PathBuf)>> {
    Ok(std::fs::read_dir(path)?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| Some((entry.file_name().into_string().ok()?, entry.path())))
        .collect())
}

/// An absolute path without `.`, `..` and trailing separators.
fn clean(path: &str) -> Result<PathBuf, Problem> {
    let path = Path::new(path.trim());
    if !path.is_absolute() {
        return Err(Problem::validation("path", "must be an absolute path"));
    }
    if path.components().any(|part| part == Component::ParentDir) {
        return Err(Problem::validation("path", "must not contain `..`"));
    }
    Ok(path.components().collect())
}

/// Checks that `path` is a directory Unseen can write to and returns its
/// canonical form, for comparison.
fn inspect(path: &Path) -> Result<PathBuf, &'static str> {
    let meta = std::fs::metadata(path).map_err(|error| match error.kind() {
        ErrorKind::NotFound => "does not exist",
        _ => "cannot be read",
    })?;
    if !meta.is_dir() {
        return Err("is not a directory");
    }
    let probe = path.join(format!(".unseen-write-test-{}", std::process::id()));
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&probe)
        .map_err(|_| "is not writable by Unseen")?;
    let _ = std::fs::remove_file(&probe);
    std::fs::canonicalize(path).map_err(|_| "cannot be read")
}

async fn known_names(state: &AppState, id: i32) -> Result<Vec<String>, Problem> {
    db::root_folders::movie_folder_names(&state.db, id)
        .await
        .map_err(failed)
}

/// Every root folder with its free space and unmapped folders.
#[utoipa::path(
    get,
    path = "/api/v1/root-folders",
    tag = "root-folders",
    responses(
        (status = 200, description = "The root folders", body = Vec<RootFolder>),
        (status = 403, description = "Permission denied", body = Problem),
    )
)]
pub async fn list_root_folders(
    _: Require<SettingsRead>,
    State(state): State<AppState>,
) -> Result<Json<Vec<RootFolder>>, Problem> {
    let mut folders = Vec::new();
    for model in db::root_folders::list(&state.db).await.map_err(failed)? {
        let known = known_names(&state, model.id).await?;
        folders.push((model, known));
    }
    let described = blocking(move || {
        folders
            .into_iter()
            .map(|(model, known)| describe(model, &known))
            .collect()
    })
    .await?;
    Ok(Json(described))
}

/// Adds a root folder after checking the path on disk.
#[utoipa::path(
    post,
    path = "/api/v1/root-folders",
    tag = "root-folders",
    request_body = NewRootFolder,
    responses(
        (status = 201, description = "The new root folder", body = RootFolder),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 409, description = "The folder is already a root folder", body = Problem),
        (status = 422, description = "The path is not usable", body = Problem),
    )
)]
pub async fn create_root_folder(
    caller: Require<SettingsManage>,
    State(state): State<AppState>,
    ip: ClientIp,
    Json(body): Json<NewRootFolder>,
) -> Result<(StatusCode, Json<RootFolder>), Problem> {
    let path = clean(&body.path)?;
    let existing = db::root_folders::list(&state.db).await.map_err(failed)?;
    let checked = {
        let path = path.clone();
        blocking(move || {
            let canonical = inspect(&path)?;
            // A stored folder that is gone is compared as written.
            let others: Vec<(PathBuf, String)> = existing
                .iter()
                .map(|other| {
                    let stored = PathBuf::from(&other.path);
                    (
                        std::fs::canonicalize(&stored).unwrap_or(stored),
                        other.path.clone(),
                    )
                })
                .collect();
            let clash = root_folder::overlap(
                &canonical,
                others.iter().map(|(canonical, _)| canonical.as_path()),
            )
            .map(|overlap| {
                let shown = |found: &Path| {
                    others
                        .iter()
                        .find(|(canonical, _)| canonical == found)
                        .map_or_else(String::new, |(_, stored)| stored.clone())
                };
                match overlap {
                    Overlap::Same(_) => Err(None),
                    Overlap::Inside(other) => {
                        Err(Some(format!("is inside the root folder {}", shown(other))))
                    }
                    Overlap::Contains(other) => {
                        Err(Some(format!("contains the root folder {}", shown(other))))
                    }
                }
            });
            Ok::<_, &'static str>(clash.unwrap_or(Ok(())))
        })
        .await?
    };
    match checked {
        Err(message) => return Err(Problem::validation("path", message)),
        Ok(Err(None)) => return Err(root_folder_error(RootFolderError::PathTaken)),
        Ok(Err(Some(message))) => return Err(Problem::validation("path", &message)),
        Ok(Ok(())) => {}
    }
    let shown = path
        .to_str()
        .ok_or_else(|| Problem::validation("path", "must be valid text"))?;
    let created = db::root_folders::create(&state.db, shown.to_owned())
        .await
        .map_err(root_folder_error)?;
    let event = Event::new("root_folder.created")
        .target("root_folder", created.id)
        .data(json!({ "path": created.path }));
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    let described = blocking(move || describe(created, &[])).await?;
    Ok((StatusCode::CREATED, Json(described)))
}

/// Deletes a root folder that no movie uses. The folder on disk stays.
#[utoipa::path(
    delete,
    path = "/api/v1/root-folders/{id}",
    tag = "root-folders",
    params(("id" = i32, Path, description = "Root folder id")),
    responses(
        (status = 204, description = "Deleted"),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such root folder", body = Problem),
        (status = 409, description = "A movie lives in the root folder", body = Problem),
    )
)]
pub async fn delete_root_folder(
    caller: Require<SettingsManage>,
    State(state): State<AppState>,
    ip: ClientIp,
    UrlPath(id): UrlPath<i32>,
) -> Result<StatusCode, Problem> {
    let current = db::root_folders::find(&state.db, id)
        .await
        .map_err(failed)?
        .ok_or_else(Problem::not_found)?;
    db::root_folders::delete(&state.db, id)
        .await
        .map_err(root_folder_error)?;
    let event = Event::new("root_folder.deleted")
        .target("root_folder", id)
        .data(json!({ "path": current.path }));
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok(StatusCode::NO_CONTENT)
}

/// Lists the sub-folders of a folder on the server, for choosing a root
/// folder. Without a path it lists the top level.
#[utoipa::path(
    get,
    path = "/api/v1/filesystem",
    tag = "root-folders",
    params(BrowseParams),
    responses(
        (status = 200, description = "The sub-folders", body = FolderListing),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 422, description = "The path cannot be listed", body = Problem),
    )
)]
pub async fn browse(
    _: Require<SettingsManage>,
    Query(params): Query<BrowseParams>,
) -> Result<Json<FolderListing>, Problem> {
    let requested = params.path.unwrap_or_default();
    let path = if requested.trim().is_empty() {
        None
    } else {
        Some(clean(&requested)?)
    };
    let listing =
        blocking(move || match path {
            None => Ok(top_level()),
            Some(path) => list_dirs(&path)
                .map(|dirs| listing(&path, dirs))
                .map_err(|error| match error.kind() {
                    ErrorKind::NotFound => "does not exist",
                    ErrorKind::NotADirectory => "is not a directory",
                    _ => "cannot be read",
                }),
        })
        .await?;
    listing
        .map(Json)
        .map_err(|message| Problem::validation("path", message))
}

fn listing(path: &Path, dirs: Vec<(String, PathBuf)>) -> FolderListing {
    let mut folders: Vec<BrowserFolder> = dirs
        .into_iter()
        .map(|(name, path)| BrowserFolder {
            name,
            path: path.to_string_lossy().into_owned(),
        })
        .collect();
    folders.sort_by_key(|folder| folder.name.to_lowercase());
    FolderListing {
        path: path.to_string_lossy().into_owned(),
        // Above the root of a Windows drive come the drives.
        parent: path
            .parent()
            .map(|parent| parent.to_string_lossy().into_owned())
            .or_else(|| cfg!(windows).then(String::new)),
        folders,
    }
}

/// `/` on Unix; the drives on Windows.
fn top_level() -> FolderListing {
    #[cfg(windows)]
    {
        let folders = ('A'..='Z')
            .map(|letter| format!("{letter}:\\"))
            .filter(|drive| Path::new(drive).is_dir())
            .map(|drive| BrowserFolder {
                name: drive.clone(),
                path: drive,
            })
            .collect();
        FolderListing {
            path: String::new(),
            parent: None,
            folders,
        }
    }
    #[cfg(not(windows))]
    {
        listing(
            Path::new("/"),
            list_dirs(Path::new("/")).unwrap_or_default(),
        )
    }
}
