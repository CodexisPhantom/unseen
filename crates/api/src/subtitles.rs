//! Language profiles, the subtitle settings and the subtitle files of a
//! movie (docs/03-domain-model.md, "Subtitles (M10)"). Subtitle files next
//! to a video are recorded when a library folder is adopted and when a
//! movie is scanned again; nothing on disk is changed.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use db::entities::{language_profiles, movie_files, movies, subtitles};
use db::subtitles::{Found, ProfileError, Values};
use serde::{Deserialize, Serialize};
use serde_json::json;
use unseenarr_core::permission::Permission;
use unseenarr_core::subtitle::{self, Item, Kind, Profile};
use utoipa::ToSchema;

use crate::AppState;
use crate::access::{LibraryRead, Require, SubtitlesManage, ensure_root_folder_access};
use crate::audit::{Actor, Changes, ClientIp, Event, record};
use crate::error::Problem;
use crate::movies::{movie_dir, movie_file_path, root_path};

fn failed(error: impl std::fmt::Display) -> Problem {
    tracing::error!(%error, "subtitles");
    Problem::internal()
}

/// What a subtitle is for.
#[derive(Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum SubtitleKind {
    Normal,
    /// Hearing impaired (SDH).
    Hi,
    /// Foreign parts only.
    Forced,
}

impl From<SubtitleKind> for Kind {
    fn from(kind: SubtitleKind) -> Self {
        match kind {
            SubtitleKind::Normal => Self::Normal,
            SubtitleKind::Hi => Self::Hi,
            SubtitleKind::Forced => Self::Forced,
        }
    }
}

impl From<Kind> for SubtitleKind {
    fn from(kind: Kind) -> Self {
        match kind {
            Kind::Normal => Self::Normal,
            Kind::Hi => Self::Hi,
            Kind::Forced => Self::Forced,
        }
    }
}

/// One wanted subtitle.
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct LanguageItem {
    /// A lower case ISO 639-1 code, with an upper case region where it
    /// matters: `en`, `pt-BR`.
    language: String,
    kind: SubtitleKind,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LanguageProfile {
    id: i32,
    name: String,
    /// In order of preference.
    items: Vec<LanguageItem>,
    /// Index into `items`: once that item and every item before it exist,
    /// Unseenarr stops looking. Absent: look for every item.
    cutoff: Option<i32>,
    created_at: String,
    updated_at: String,
}

impl TryFrom<language_profiles::Model> for LanguageProfile {
    type Error = serde_json::Error;

    fn try_from(model: language_profiles::Model) -> Result<Self, Self::Error> {
        Ok(Self {
            id: model.id,
            name: model.name,
            items: serde_json::from_value(model.items)?,
            cutoff: model.cutoff,
            created_at: model.created_at,
            updated_at: model.updated_at,
        })
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewLanguageProfile {
    name: String,
    items: Vec<LanguageItem>,
    #[serde(default)]
    cutoff: Option<i32>,
}

fn profile_error(error: ProfileError) -> Problem {
    match error {
        ProfileError::NotFound => Problem::not_found(),
        ProfileError::NameTaken => {
            Problem::conflict("name_taken", "A language profile has this name")
        }
        ProfileError::InUse => Problem::conflict(
            "profile_in_use",
            "A movie uses this language profile; change the movie first",
        ),
        ProfileError::Db(error) => failed(error),
    }
}

fn response(model: language_profiles::Model) -> Result<LanguageProfile, Problem> {
    model.try_into().map_err(failed)
}

/// Checks the rules and turns the profile into what the database stores.
fn checked(mut profile: NewLanguageProfile) -> Result<Values, Problem> {
    profile.name = profile.name.trim().to_owned();
    subtitle::validate(&Profile {
        name: profile.name.clone(),
        items: profile
            .items
            .iter()
            .map(|item| Item {
                language: item.language.clone(),
                kind: item.kind.into(),
            })
            .collect(),
        cutoff: profile
            .cutoff
            .map(|cutoff| usize::try_from(cutoff).unwrap_or(usize::MAX)),
    })
    .map_err(|(field, message)| Problem::validation(field, &message))?;
    Ok(Values {
        name: profile.name,
        items: serde_json::to_value(profile.items).map_err(failed)?,
        cutoff: profile.cutoff,
    })
}

async fn load_profile(state: &AppState, id: i32) -> Result<language_profiles::Model, Problem> {
    db::subtitles::find_profile(&state.db, id)
        .await
        .map_err(failed)?
        .ok_or_else(Problem::not_found)
}

/// A language profile id that must exist, for the field `field`.
async fn check_profile(state: &AppState, id: i32, field: &str) -> Result<(), Problem> {
    db::subtitles::find_profile(&state.db, id)
        .await
        .map_err(failed)?
        .map(|_| ())
        .ok_or_else(|| Problem::validation(field, "must be the id of a language profile"))
}

/// Every language profile, ordered by name.
#[utoipa::path(
    get,
    path = "/api/v1/language-profiles",
    tag = "subtitles",
    responses(
        (status = 200, description = "The profiles", body = Vec<LanguageProfile>),
        (status = 403, description = "Permission denied", body = Problem),
    )
)]
pub async fn list_language_profiles(
    _: Require<LibraryRead>,
    State(state): State<AppState>,
) -> Result<Json<Vec<LanguageProfile>>, Problem> {
    db::subtitles::list_profiles(&state.db)
        .await
        .map_err(failed)?
        .into_iter()
        .map(response)
        .collect::<Result<_, _>>()
        .map(Json)
}

/// Creates a language profile. The items are the wanted subtitles in order;
/// the cutoff is the index of one of them.
#[utoipa::path(
    post,
    path = "/api/v1/language-profiles",
    tag = "subtitles",
    request_body = NewLanguageProfile,
    responses(
        (status = 201, description = "The new profile", body = LanguageProfile),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 409, description = "The name is taken", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
    )
)]
pub async fn create_language_profile(
    caller: Require<SubtitlesManage>,
    State(state): State<AppState>,
    ip: ClientIp,
    Json(body): Json<NewLanguageProfile>,
) -> Result<(StatusCode, Json<LanguageProfile>), Problem> {
    let created = db::subtitles::create_profile(&state.db, checked(body)?)
        .await
        .map_err(profile_error)?;
    let event = Event::new("language_profile.created")
        .target("language_profile", created.id)
        .data(json!({ "name": created.name }));
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok((StatusCode::CREATED, Json(response(created)?)))
}

/// Replaces a language profile. The result must satisfy the same rules as a
/// new profile.
#[utoipa::path(
    put,
    path = "/api/v1/language-profiles/{id}",
    tag = "subtitles",
    params(("id" = i32, Path, description = "Profile id")),
    request_body = NewLanguageProfile,
    responses(
        (status = 200, description = "The changed profile", body = LanguageProfile),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such profile", body = Problem),
        (status = 409, description = "The name is taken", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
    )
)]
pub async fn update_language_profile(
    caller: Require<SubtitlesManage>,
    State(state): State<AppState>,
    ip: ClientIp,
    Path(id): Path<i32>,
    Json(body): Json<NewLanguageProfile>,
) -> Result<Json<LanguageProfile>, Problem> {
    let before = response(load_profile(&state, id).await?)?;
    let updated = db::subtitles::update_profile(&state.db, id, checked(body)?)
        .await
        .map_err(profile_error)?;
    let after = response(updated)?;
    let changed = Changes::new()
        .field("name", &before.name, &after.name)
        .field("items", &before.items, &after.items)
        .field("cutoff", before.cutoff, after.cutoff);
    let event = Event::new("language_profile.updated")
        .target("language_profile", id)
        .data(changed.into_data());
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok(Json(after))
}

/// Deletes a language profile that no movie uses. When it is the default
/// profile, the default is cleared.
#[utoipa::path(
    delete,
    path = "/api/v1/language-profiles/{id}",
    tag = "subtitles",
    params(("id" = i32, Path, description = "Profile id")),
    responses(
        (status = 204, description = "Deleted"),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such profile", body = Problem),
        (status = 409, description = "A movie uses the profile", body = Problem),
    )
)]
pub async fn delete_language_profile(
    caller: Require<SubtitlesManage>,
    State(state): State<AppState>,
    ip: ClientIp,
    Path(id): Path<i32>,
) -> Result<StatusCode, Problem> {
    let current = load_profile(&state, id).await?;
    db::subtitles::delete_profile(&state.db, id)
        .await
        .map_err(profile_error)?;
    let event = Event::new("language_profile.deleted")
        .target("language_profile", id)
        .data(json!({ "name": current.name }));
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleSettings {
    /// The language profile of a new movie. Absent: new movies want no
    /// subtitles.
    default_profile_id: Option<i32>,
}

/// The subtitle settings.
#[utoipa::path(
    get,
    path = "/api/v1/settings/subtitles",
    tag = "settings",
    responses(
        (status = 200, description = "The settings", body = SubtitleSettings),
        (status = 403, description = "Permission denied", body = Problem),
    )
)]
pub async fn get_settings(
    _: Require<LibraryRead>,
    State(state): State<AppState>,
) -> Result<Json<SubtitleSettings>, Problem> {
    Ok(Json(SubtitleSettings {
        default_profile_id: db::subtitles::default_profile(&state.db)
            .await
            .map_err(failed)?,
    }))
}

/// Changes the subtitle settings. `null` clears the default profile.
#[utoipa::path(
    put,
    path = "/api/v1/settings/subtitles",
    tag = "settings",
    request_body = SubtitleSettings,
    responses(
        (status = 200, description = "The settings after the change", body = SubtitleSettings),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
    )
)]
pub async fn update_settings(
    caller: Require<SubtitlesManage>,
    State(state): State<AppState>,
    ip: ClientIp,
    Json(body): Json<SubtitleSettings>,
) -> Result<Json<SubtitleSettings>, Problem> {
    let before = db::subtitles::default_profile(&state.db)
        .await
        .map_err(failed)?;
    match body.default_profile_id {
        Some(id) => {
            check_profile(&state, id, "defaultProfileId").await?;
            db::settings::set_i64(&state.db, db::subtitles::DEFAULT_PROFILE_KEY, id.into())
                .await
                .map_err(failed)?;
        }
        None => db::settings::remove(&state.db, db::subtitles::DEFAULT_PROFILE_KEY)
            .await
            .map_err(failed)?,
    }
    let changes = Changes::new().field("defaultProfileId", before, body.default_profile_id);
    let event = Event::new("setting.updated")
        .target("setting", "subtitles")
        .data(changes.into_data());
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok(Json(body))
}

/// A subtitle file of a movie file.
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Subtitle {
    id: i32,
    movie_file_id: i32,
    /// `en`, `pt-BR`, or `und` when the file name has no language.
    language: String,
    kind: String,
    /// Relative to the movie's folder.
    relative_path: String,
    /// `opensubtitles` for a downloaded subtitle. Absent for a file that was
    /// found next to the video.
    provider: Option<String>,
    score: Option<i32>,
    added_at: String,
}

impl From<subtitles::Model> for Subtitle {
    fn from(row: subtitles::Model) -> Self {
        Self {
            id: row.id,
            movie_file_id: row.movie_file_id,
            language: row.language,
            kind: row.kind,
            relative_path: row.relative_path,
            provider: row.provider,
            score: row.score,
            added_at: row.added_at,
        }
    }
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MovieSubtitles {
    /// The movie's language profile. Absent: no subtitles are wanted.
    language_profile_id: Option<i32>,
    subtitles: Vec<Subtitle>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MovieLanguageProfile {
    /// `null` clears the profile: no subtitles are wanted.
    language_profile_id: Option<i32>,
}

async fn load_movie(state: &AppState, id: i32) -> Result<movies::Model, Problem> {
    Ok(db::movies::find(&state.db, id)
        .await
        .map_err(failed)?
        .ok_or_else(Problem::not_found)?
        .model)
}

async fn movie_subtitles(state: &AppState, id: i32) -> Result<Json<MovieSubtitles>, Problem> {
    let movie = load_movie(state, id).await?;
    let rows = db::subtitles::of_movie(&state.db, id)
        .await
        .map_err(failed)?;
    Ok(Json(MovieSubtitles {
        language_profile_id: movie.language_profile_id,
        subtitles: rows.into_iter().map(Subtitle::from).collect(),
    }))
}

/// The movie's language profile and the subtitle files of all its video
/// files.
#[utoipa::path(
    get,
    path = "/api/v1/movies/{id}/subtitles",
    tag = "subtitles",
    params(("id" = i32, Path, description = "Movie id")),
    responses(
        (status = 200, description = "The subtitles", body = MovieSubtitles),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such movie", body = Problem),
    )
)]
pub async fn list_movie_subtitles(
    caller: Require<LibraryRead>,
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<MovieSubtitles>, Problem> {
    let movie = load_movie(&state, id).await?;
    ensure_root_folder_access(
        &state,
        &caller,
        Permission::LibraryRead,
        movie.root_folder_id,
    )
    .await?;
    movie_subtitles(&state, id).await
}

/// Sets or clears the language profile of a movie.
#[utoipa::path(
    put,
    path = "/api/v1/movies/{id}/language-profile",
    tag = "subtitles",
    params(("id" = i32, Path, description = "Movie id")),
    request_body = MovieLanguageProfile,
    responses(
        (status = 200, description = "The movie's subtitles after the change", body = MovieSubtitles),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such movie", body = Problem),
        (status = 422, description = "No such language profile", body = Problem),
    )
)]
pub async fn set_movie_language_profile(
    caller: Require<SubtitlesManage>,
    State(state): State<AppState>,
    ip: ClientIp,
    Path(id): Path<i32>,
    Json(body): Json<MovieLanguageProfile>,
) -> Result<Json<MovieSubtitles>, Problem> {
    let movie = load_movie(&state, id).await?;
    ensure_root_folder_access(
        &state,
        &caller,
        Permission::SubtitlesManage,
        movie.root_folder_id,
    )
    .await?;
    if let Some(profile_id) = body.language_profile_id {
        check_profile(&state, profile_id, "languageProfileId").await?;
    }
    db::subtitles::set_movie_profile(&state.db, id, body.language_profile_id)
        .await
        .map_err(failed)?;
    let changes = Changes::new().field(
        "languageProfileId",
        movie.language_profile_id,
        body.language_profile_id,
    );
    let event = Event::new("movie.language_profile_changed")
        .target("movie", id)
        .data(changes.into_data());
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    movie_subtitles(&state, id).await
}

/// Looks for subtitle files next to the video files of a movie, and records
/// them. A subtitle whose file is gone is forgotten; one that came from a
/// provider is left alone. Nothing on disk changes.
#[utoipa::path(
    post,
    path = "/api/v1/movies/{id}/subtitles/scan",
    tag = "subtitles",
    params(("id" = i32, Path, description = "Movie id")),
    responses(
        (status = 200, description = "The subtitles after the scan", body = MovieSubtitles),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such movie", body = Problem),
    )
)]
pub async fn scan_subtitles(
    caller: Require<SubtitlesManage>,
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<MovieSubtitles>, Problem> {
    let movie = load_movie(&state, id).await?;
    ensure_root_folder_access(
        &state,
        &caller,
        Permission::SubtitlesManage,
        movie.root_folder_id,
    )
    .await?;
    scan_movie(&state, &movie).await?;
    movie_subtitles(&state, id).await
}

/// Preference when two files have the same language and kind: the common
/// text format first.
fn extension_rank(name: &str) -> usize {
    let extension = name.rsplit_once('.').map_or("", |(_, e)| e);
    subtitle::EXTENSIONS
        .iter()
        .position(|known| known.eq_ignore_ascii_case(extension))
        .unwrap_or(usize::MAX)
}

/// The subtitle files in the folder of `video`, found by name.
fn sidecars_of(dir: &std::path::Path, relative_video: &str) -> std::io::Result<Vec<Found>> {
    let (folder, video_name) = relative_video
        .rsplit_once('/')
        .map_or(("", relative_video), |(folder, name)| (folder, name));
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir.join(folder))? {
        let entry = entry?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if !entry.file_type()?.is_file() {
            continue;
        }
        if let Some(sidecar) = subtitle::parse_sidecar(video_name, &name) {
            found.push(Found {
                language: sidecar.language,
                kind: sidecar.kind,
                relative_path: if folder.is_empty() {
                    name
                } else {
                    format!("{folder}/{name}")
                },
            });
        }
    }
    found.sort_by_key(|f| (extension_rank(&f.relative_path), f.relative_path.clone()));
    Ok(found)
}

/// Records the subtitle files next to every video file of `movie`. Used by
/// adoption and by the scan route.
pub(crate) async fn scan_movie(state: &AppState, movie: &movies::Model) -> Result<(), Problem> {
    let root = root_path(state, movie).await?;
    let Some(dir) = movie_dir(&root, &movie.folder_name) else {
        return Ok(());
    };
    let files: Vec<movie_files::Model> = db::movies::files_of(&state.db, movie.id)
        .await
        .map_err(failed)?;
    let found = tokio::task::spawn_blocking(move || {
        files
            .into_iter()
            .filter(|file| movie_file_path(&dir, &file.relative_path).is_some())
            // A folder that cannot be read keeps what is recorded.
            .filter_map(|file| {
                sidecars_of(&dir, &file.relative_path)
                    .map_err(|error| {
                        tracing::warn!(%error, file = %file.relative_path, "subtitle scan");
                    })
                    .ok()
                    .map(|found| (file.id, found))
            })
            .collect::<Vec<_>>()
    })
    .await
    .map_err(failed)?;
    for (file_id, found) in found {
        db::subtitles::sync_external(&state.db, file_id, &found)
            .await
            .map_err(failed)?;
    }
    Ok(())
}
