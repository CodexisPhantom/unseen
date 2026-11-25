//! The movie naming settings and their live preview (docs/09-filesystem.md,
//! "Naming templates"). The engine is `unseen_core::naming`.

use axum::Json;
use axum::extract::State;
use db::settings::{get_bool, get_string, set_bool, set_string};
use serde::{Deserialize, Serialize};
use unseen_core::naming::{
    self, ColonReplacement, MovieFacts, NamingConfig, TemplateError, validate_template,
};
use utoipa::ToSchema;

use crate::AppState;
use crate::access::{Require, SettingsManage, SettingsRead};
use crate::audit::{Actor, Changes, ClientIp, Event, record};
use crate::error::Problem;

const MOVIE_FOLDER: &str = "naming.movie_folder";
const MOVIE_FILE: &str = "naming.movie_file";
const RENAME_MOVIES: &str = "naming.rename_movies";
const REPLACE_ILLEGAL: &str = "naming.replace_illegal_characters";
const COLON: &str = "naming.colon_replacement";

fn failed(error: impl std::fmt::Display) -> Problem {
    tracing::error!(%error, "naming settings");
    Problem::internal()
}

fn overridden(key: &str) -> Problem {
    let mut problem = Problem::conflict("setting_overridden", "The environment sets this setting");
    problem.detail = Some(format!(
        "{key} is set by {} and cannot be changed here. Remove the variable to edit it.",
        db::settings::Overrides::var_name(key)
    ));
    problem
}

async fn text(state: &AppState, key: &str) -> Result<Option<String>, Problem> {
    if let Some(value) = state.settings_overrides.get(key) {
        return Ok(Some(value.expose().to_owned()));
    }
    get_string(&state.db, key).await.map_err(failed)
}

async fn flag(state: &AppState, key: &str) -> Result<Option<bool>, Problem> {
    if let Some(value) = state.settings_overrides.get(key) {
        return Ok(Some(!matches!(
            value.expose().to_ascii_lowercase().as_str(),
            "false" | "0" | "no"
        )));
    }
    get_bool(&state.db, key).await.map_err(failed)
}

/// The stored settings with the defaults for what is not stored.
pub async fn config(state: &AppState) -> Result<NamingConfig, Problem> {
    let defaults = NamingConfig::default();
    Ok(NamingConfig {
        movie_folder: text(state, MOVIE_FOLDER)
            .await?
            .unwrap_or(defaults.movie_folder),
        movie_file: text(state, MOVIE_FILE)
            .await?
            .unwrap_or(defaults.movie_file),
        rename_movies: flag(state, RENAME_MOVIES)
            .await?
            .unwrap_or(defaults.rename_movies),
        replace_illegal_characters: flag(state, REPLACE_ILLEGAL)
            .await?
            .unwrap_or(defaults.replace_illegal_characters),
        colon_replacement: text(state, COLON)
            .await?
            .and_then(|value| ColonReplacement::parse(&value))
            .unwrap_or(defaults.colon_replacement),
    })
}

/// The folder for a new movie, from the stored folder template. The default
/// template is the fallback when the stored one renders nothing for this movie.
pub async fn movie_folder(
    state: &AppState,
    movie: &db::movies::Metadata,
    collection: Option<&str>,
) -> Result<String, Problem> {
    let facts = MovieFacts {
        title: movie.title.clone(),
        original_title: movie.original_title.clone(),
        year: movie.year,
        collection: collection.map(str::to_owned),
        imdb_id: movie.imdb_id.clone(),
        tmdb_id: movie.tmdb_id,
    };
    let folder = config(state).await?.movie_folder(&facts);
    Ok(if folder.is_empty() {
        naming::movie_folder_name(&movie.title, movie.year)
    } else {
        folder
    })
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NamingSettings {
    /// Template of the movie folder, for example
    /// `{Movie Title} ({Release Year})`.
    movie_folder: String,
    /// Template of the movie file name, without the extension.
    movie_file: String,
    /// Off: an imported file keeps its original name.
    rename_movies: bool,
    /// Off: illegal characters are removed instead of replaced.
    replace_illegal_characters: bool,
    /// How a colon is written: `delete`, `dash`, `spaceDash`,
    /// `spaceDashSpace` or `smart`.
    colon_replacement: String,
    /// Fields set by an environment variable (`UNSEEN_SETTING_...`). They are
    /// read-only: a change is refused with 409 `setting_overridden`.
    overridden: Vec<&'static str>,
}

/// A field that is absent keeps the stored value.
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NamingSettingsPatch {
    movie_folder: Option<String>,
    movie_file: Option<String>,
    rename_movies: Option<bool>,
    replace_illegal_characters: Option<bool>,
    /// `delete`, `dash`, `spaceDash`, `spaceDashSpace` or `smart`.
    colon_replacement: Option<String>,
}

const FIELDS: [(&str, &str); 5] = [
    ("movieFolder", MOVIE_FOLDER),
    ("movieFile", MOVIE_FILE),
    ("renameMovies", RENAME_MOVIES),
    ("replaceIllegalCharacters", REPLACE_ILLEGAL),
    ("colonReplacement", COLON),
];

async fn current(state: &AppState) -> Result<NamingSettings, Problem> {
    let config = config(state).await?;
    Ok(NamingSettings {
        movie_folder: config.movie_folder,
        movie_file: config.movie_file,
        rename_movies: config.rename_movies,
        replace_illegal_characters: config.replace_illegal_characters,
        colon_replacement: config.colon_replacement.as_str().to_owned(),
        overridden: FIELDS
            .iter()
            .filter(|(_, key)| state.settings_overrides.contains(key))
            .map(|(field, _)| *field)
            .collect(),
    })
}

fn invalid_template(field: &str, error: &TemplateError) -> Problem {
    Problem::validation(field, &error.to_string())
}

/// Applies the patch to `config`; a bad template or colon rule is a 422.
fn apply(config: &mut NamingConfig, patch: &NamingSettingsPatch) -> Result<(), Problem> {
    let (movie, file) = naming::sample();
    if let Some(template) = &patch.movie_folder {
        validate_template(template).map_err(|error| invalid_template("movieFolder", &error))?;
        config.movie_folder.clone_from(template);
        if config.movie_folder(&movie).is_empty() {
            return Err(Problem::validation("movieFolder", "must produce a name"));
        }
    }
    if let Some(template) = &patch.movie_file {
        validate_template(template).map_err(|error| invalid_template("movieFile", &error))?;
        config.movie_file.clone_from(template);
        if config.movie_file(&movie, &file).is_empty() {
            return Err(Problem::validation("movieFile", "must produce a name"));
        }
    }
    if let Some(value) = patch.rename_movies {
        config.rename_movies = value;
    }
    if let Some(value) = patch.replace_illegal_characters {
        config.replace_illegal_characters = value;
    }
    if let Some(value) = &patch.colon_replacement {
        config.colon_replacement = ColonReplacement::parse(value).ok_or_else(|| {
            Problem::validation(
                "colonReplacement",
                "must be delete, dash, spaceDash, spaceDashSpace or smart",
            )
        })?;
    }
    Ok(())
}

/// The naming settings.
#[utoipa::path(
    get,
    path = "/api/v1/settings/naming",
    tag = "settings",
    responses(
        (status = 200, description = "The settings", body = NamingSettings),
        (status = 403, description = "Permission denied", body = Problem),
    )
)]
pub async fn get_naming(
    _: Require<SettingsRead>,
    State(state): State<AppState>,
) -> Result<Json<NamingSettings>, Problem> {
    current(&state).await.map(Json)
}

/// Changes the naming settings. A template with an unknown token is refused.
#[utoipa::path(
    put,
    path = "/api/v1/settings/naming",
    tag = "settings",
    request_body = NamingSettingsPatch,
    responses(
        (status = 200, description = "The settings after the change", body = NamingSettings),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 409, description = "The environment sets this setting", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
    )
)]
pub async fn update_naming(
    caller: Require<SettingsManage>,
    State(state): State<AppState>,
    ip: ClientIp,
    Json(body): Json<NamingSettingsPatch>,
) -> Result<Json<NamingSettings>, Problem> {
    let mut changed = config(&state).await?;
    apply(&mut changed, &body)?;
    let sent = [
        body.movie_folder.is_some(),
        body.movie_file.is_some(),
        body.rename_movies.is_some(),
        body.replace_illegal_characters.is_some(),
        body.colon_replacement.is_some(),
    ];
    for ((_, key), sent) in FIELDS.iter().zip(sent) {
        if sent && state.settings_overrides.contains(key) {
            return Err(overridden(key));
        }
    }

    let before = current(&state).await?;
    let db = &state.db;
    if body.movie_folder.is_some() {
        set_string(db, MOVIE_FOLDER, &changed.movie_folder)
            .await
            .map_err(failed)?;
    }
    if body.movie_file.is_some() {
        set_string(db, MOVIE_FILE, &changed.movie_file)
            .await
            .map_err(failed)?;
    }
    if body.rename_movies.is_some() {
        set_bool(db, RENAME_MOVIES, changed.rename_movies)
            .await
            .map_err(failed)?;
    }
    if body.replace_illegal_characters.is_some() {
        set_bool(db, REPLACE_ILLEGAL, changed.replace_illegal_characters)
            .await
            .map_err(failed)?;
    }
    if body.colon_replacement.is_some() {
        set_string(db, COLON, changed.colon_replacement.as_str())
            .await
            .map_err(failed)?;
    }
    let after = current(&state).await?;

    let changes = Changes::new()
        .field("movieFolder", &before.movie_folder, &after.movie_folder)
        .field("movieFile", &before.movie_file, &after.movie_file)
        .field("renameMovies", before.rename_movies, after.rename_movies)
        .field(
            "replaceIllegalCharacters",
            before.replace_illegal_characters,
            after.replace_illegal_characters,
        )
        .field(
            "colonReplacement",
            &before.colon_replacement,
            &after.colon_replacement,
        );
    let event = Event::new("setting.updated")
        .target("setting", "naming")
        .data(changes.into_data());
    record(&state, &Actor::of(&state, &caller).await, &ip, event).await;
    Ok(Json(after))
}

/// The settings to preview: a field that is absent uses the stored value.
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NamingPreviewRequest {
    movie_folder: Option<String>,
    movie_file: Option<String>,
    replace_illegal_characters: Option<bool>,
    /// `delete`, `dash`, `spaceDash`, `spaceDashSpace` or `smart`.
    colon_replacement: Option<String>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NamingPreview {
    /// The folder of Radarr's example movie, `The Movie: Title` (2010).
    movie_folder_example: String,
    /// The file name of the example movie, without the extension.
    movie_file_example: String,
}

/// Renders the example movie with the given settings, saved or not.
#[utoipa::path(
    post,
    path = "/api/v1/settings/naming/preview",
    tag = "settings",
    request_body = NamingPreviewRequest,
    responses(
        (status = 200, description = "The example names", body = NamingPreview),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 422, description = "A template has an unknown token", body = Problem),
    )
)]
pub async fn preview_naming(
    _: Require<SettingsRead>,
    State(state): State<AppState>,
    Json(body): Json<NamingPreviewRequest>,
) -> Result<Json<NamingPreview>, Problem> {
    let mut config = config(&state).await?;
    apply(
        &mut config,
        &NamingSettingsPatch {
            movie_folder: body.movie_folder,
            movie_file: body.movie_file,
            rename_movies: None,
            replace_illegal_characters: body.replace_illegal_characters,
            colon_replacement: body.colon_replacement,
        },
    )?;
    let (movie, file) = naming::sample();
    Ok(Json(NamingPreview {
        movie_folder_example: config.movie_folder(&movie),
        movie_file_example: config.movie_file(&movie, &file),
    }))
}
