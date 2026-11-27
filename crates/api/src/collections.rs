//! The collections resource (docs/03-domain-model.md, docs/05-api.md):
//! read the collections of the library and monitor them.

use std::collections::{HashMap, HashSet};

use axum::Json;
use axum::extract::{Path, State};
use db::collections::{CollectionError, Patch};
use db::entities::collections;
use db::movies::{Metadata, MovieError, NewMovie};
use serde::{Deserialize, Serialize};
use serde_json::json;
use unseenarr_core::metadata::{MetadataError, MetadataProvider, MovieSummary};
use unseenarr_core::naming;
use unseenarr_core::permission::Permission;
use utoipa::ToSchema;

use crate::AppState;
use crate::access::{
    LibraryEdit, LibraryRead, Require, allowed_root_folders, ensure_root_folder_access,
};
use crate::audit::{Actor, Changes, ClientIp, Event, record};
use crate::error::Problem;
use crate::movies::{Movie, check_availability, check_profile};
use crate::tmdb::{client, provider_error};

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    id: i32,
    tmdb_id: i32,
    title: String,
    sort_title: String,
    overview: Option<String>,
    /// Path on TMDB's image host; the browser loads the image.
    poster_path: Option<String>,
    /// A monitored collection has its missing movies added.
    monitored: bool,
    /// Where the movies added from this collection go.
    root_folder_id: Option<i32>,
    /// The primary profile of the movies added from this collection.
    quality_profile_id: Option<i32>,
    /// `announced`, `inCinemas` or `released`.
    minimum_availability: String,
    /// How many movies of this collection are in the library.
    movie_count: i64,
    added_at: String,
    updated_at: String,
}

impl Collection {
    fn new(model: collections::Model, movie_count: i64) -> Self {
        Self {
            id: model.id,
            tmdb_id: model.tmdb_id,
            title: model.title,
            sort_title: model.sort_title,
            overview: model.overview,
            poster_path: model.poster_path,
            monitored: model.monitored,
            root_folder_id: model.root_folder_id,
            quality_profile_id: model.quality_profile_id,
            minimum_availability: model.minimum_availability,
            movie_count,
            added_at: model.added_at,
            updated_at: model.updated_at,
        }
    }
}

/// A movie of the collection that is not in the library.
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MissingMovie {
    tmdb_id: i32,
    title: String,
    /// Absent when TMDB has no release date yet.
    year: Option<i32>,
    release_date: Option<String>,
    overview: Option<String>,
    poster_path: Option<String>,
}

impl From<MovieSummary> for MissingMovie {
    fn from(summary: MovieSummary) -> Self {
        Self {
            tmdb_id: summary.tmdb_id,
            year: summary.year(),
            title: summary.title,
            release_date: summary.release_date,
            overview: Some(summary.overview).filter(|text| !text.is_empty()),
            poster_path: summary.poster_path,
        }
    }
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CollectionDetail {
    #[serde(flatten)]
    collection: Collection,
    /// The movies of the collection that are in the library.
    movies: Vec<Movie>,
    /// The movies of the collection on TMDB that are not in the library.
    missing: Vec<MissingMovie>,
}

/// A field that is absent keeps the stored value.
#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CollectionPatch {
    /// `true` needs a root folder and a quality profile, set here or before,
    /// and adds the missing released and announced movies.
    monitored: Option<bool>,
    root_folder_id: Option<i32>,
    quality_profile_id: Option<i32>,
    /// `announced`, `inCinemas` or `released`.
    minimum_availability: Option<String>,
}

fn failed(error: impl std::fmt::Display) -> Problem {
    tracing::error!(%error, "collections");
    Problem::internal()
}

async fn load(state: &AppState, id: i32) -> Result<collections::Model, Problem> {
    db::collections::find(&state.db, id)
        .await
        .map_err(failed)?
        .ok_or_else(Problem::not_found)
}

async fn movie_count(state: &AppState, id: i32) -> Result<i64, Problem> {
    Ok(db::collections::movie_counts(&state.db)
        .await
        .map_err(failed)?
        .get(&id)
        .copied()
        .unwrap_or(0))
}

/// Every collection of the library, by sort title.
#[utoipa::path(
    get,
    path = "/api/v1/collections",
    tag = "collections",
    responses(
        (status = 200, description = "The collections", body = Vec<Collection>),
        (status = 403, description = "Permission denied", body = Problem),
    )
)]
pub async fn list_collections(
    caller: Require<LibraryRead>,
    State(state): State<AppState>,
) -> Result<Json<Vec<Collection>>, Problem> {
    let allowed = allowed_root_folders(&state, &caller, Permission::LibraryRead).await?;
    let rows = db::collections::list(&state.db).await.map_err(failed)?;
    match allowed {
        None => {
            let counts = db::collections::movie_counts(&state.db)
                .await
                .map_err(failed)?;
            Ok(Json(
                rows.into_iter()
                    .map(|row| {
                        let count = counts.get(&row.id).copied().unwrap_or(0);
                        Collection::new(row, count)
                    })
                    .collect(),
            ))
        }
        Some(allowed) => {
            let mut counts: HashMap<i32, i64> = HashMap::new();
            for movie in db::movies::list(&state.db).await.map_err(failed)? {
                if allowed.contains(&movie.model.root_folder_id)
                    && let Some(cid) = movie.model.collection_id
                {
                    *counts.entry(cid).or_insert(0) += 1;
                }
            }
            Ok(Json(
                rows.into_iter()
                    .filter_map(|mut row| {
                        let count = counts.get(&row.id).copied().unwrap_or(0);
                        let folder_visible =
                            row.root_folder_id.is_some_and(|rf| allowed.contains(&rf));
                        if count == 0 && !folder_visible {
                            return None;
                        }
                        if !folder_visible {
                            row.root_folder_id = None;
                        }
                        Some(Collection::new(row, count))
                    })
                    .collect(),
            ))
        }
    }
}

/// A collection with its movies in the library and the ones that are
/// missing. The missing ones come from TMDB.
#[utoipa::path(
    get,
    path = "/api/v1/collections/{id}",
    tag = "collections",
    params(("id" = i32, Path, description = "Collection id")),
    responses(
        (status = 200, description = "The collection", body = CollectionDetail),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such collection", body = Problem),
        (status = 409, description = "The TMDB key is not set (`tmdb_not_configured`)", body = Problem),
        (status = 502, description = "TMDB failed or refused the key", body = Problem),
    )
)]
pub async fn get_collection(
    caller: Require<LibraryRead>,
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<CollectionDetail>, Problem> {
    let allowed = allowed_root_folders(&state, &caller, Permission::LibraryRead).await?;
    let mut row = load(&state, id).await?;
    let all_in_collection = db::movies::in_collection(&state.db, id)
        .await
        .map_err(failed)?;
    let (in_library, taken): (_, HashSet<i32>) = match &allowed {
        None => {
            let taken = db::movies::tmdb_ids(&state.db)
                .await
                .map_err(failed)?
                .into_iter()
                .collect();
            (all_in_collection, taken)
        }
        Some(allowed) => {
            let visible: Vec<_> = all_in_collection
                .into_iter()
                .filter(|movie| allowed.contains(&movie.model.root_folder_id))
                .collect();
            let folder_visible = row.root_folder_id.is_some_and(|rf| allowed.contains(&rf));
            if visible.is_empty() && !folder_visible {
                return Err(Problem::not_found());
            }
            if !folder_visible {
                row.root_folder_id = None;
            }
            let taken = db::movies::list(&state.db)
                .await
                .map_err(failed)?
                .into_iter()
                .filter(|movie| allowed.contains(&movie.model.root_folder_id))
                .map(|movie| movie.model.tmdb_id)
                .collect();
            (visible, taken)
        }
    };
    let listed = match client(&state).await?.collection(row.tmdb_id).await {
        Ok(collection) => collection.movies,
        // TMDB dropped the collection: the library still shows its movies.
        Err(MetadataError::NotFound) => Vec::new(),
        Err(error) => return Err(provider_error(error)),
    };
    Ok(Json(CollectionDetail {
        collection: Collection::new(row, i64::try_from(in_library.len()).unwrap_or(i64::MAX)),
        movies: in_library.into_iter().map(Movie::from).collect(),
        missing: listed
            .into_iter()
            .filter(|summary| !taken.contains(&summary.tmdb_id))
            .map(MissingMovie::from)
            .collect(),
    }))
}

/// Adds the listed movies that are not in the library, with the defaults of
/// the collection. Movies that TMDB has not dated or announced yet, or that
/// it canceled, wait for a later run.
async fn add_missing(
    state: &AppState,
    provider: &dyn MetadataProvider,
    row: &collections::Model,
    listed: &[MovieSummary],
    actor: &Actor,
    ip: &ClientIp,
) -> Result<(), Problem> {
    let (Some(root_folder_id), Some(profile_id)) = (row.root_folder_id, row.quality_profile_id)
    else {
        return Ok(());
    };
    let mut taken: HashSet<i32> = db::movies::tmdb_ids(&state.db)
        .await
        .map_err(failed)?
        .into_iter()
        .collect();
    for summary in listed {
        if !taken.insert(summary.tmdb_id) {
            continue;
        }
        let details = match provider.details(summary.tmdb_id).await {
            Ok(details) => details,
            Err(MetadataError::NotFound) => continue,
            Err(error) => return Err(provider_error(error)),
        };
        if !matches!(details.radarr_status(), "released" | "announced") {
            continue;
        }
        let Some(metadata) = Metadata::from_details(&details) else {
            continue;
        };
        let folder_name = naming::movie_folder_name(&metadata.title, metadata.year);
        let created = db::movies::create(
            &state.db,
            NewMovie {
                metadata,
                root_folder_id,
                folder_name,
                monitored: true,
                minimum_availability: row.minimum_availability.clone(),
                collection_id: Some(row.id),
            },
            &[profile_id],
            &[],
        )
        .await;
        match created {
            Ok(movie) => {
                let event = Event::new("movie.created")
                    .target("movie", movie.model.id)
                    .data(json!({
                        "title": movie.model.title,
                        "tmdbId": movie.model.tmdb_id,
                        "collectionId": row.id,
                    }));
                record(state, actor, ip, event).await;
            }
            // Another movie holds the folder name; the user can add this
            // one by hand with a name of their choice.
            Err(MovieError::TmdbTaken | MovieError::FolderTaken) => {}
            Err(error) => return Err(failed(format!("{error:?}"))),
        }
    }
    Ok(())
}

/// Changes the monitored state and the defaults of a collection. Setting
/// `monitored` to `true` adds the movies of the collection that are missing,
/// released or announced, with the root folder, quality profile and minimum
/// availability of the collection. Send it again to retry after a failure.
#[utoipa::path(
    patch,
    path = "/api/v1/collections/{id}",
    tag = "collections",
    params(("id" = i32, Path, description = "Collection id")),
    request_body = CollectionPatch,
    responses(
        (status = 200, description = "The changed collection", body = Collection),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such collection", body = Problem),
        (status = 409, description = "The TMDB key is not set (`tmdb_not_configured`)", body = Problem),
        (status = 422, description = "Validation failed", body = Problem),
        (status = 502, description = "TMDB failed or refused the key", body = Problem),
    )
)]
pub async fn update_collection(
    caller: Require<LibraryEdit>,
    State(state): State<AppState>,
    ip: ClientIp,
    Path(id): Path<i32>,
    Json(body): Json<CollectionPatch>,
) -> Result<Json<Collection>, Problem> {
    let before = load(&state, id).await?;
    let allowed = allowed_root_folders(&state, &caller, Permission::LibraryEdit).await?;
    if let Some(allowed) = &allowed {
        let in_collection = db::movies::in_collection(&state.db, id)
            .await
            .map_err(failed)?;
        let has_visible_movie = in_collection
            .iter()
            .any(|movie| allowed.contains(&movie.model.root_folder_id));
        let folder_visible = before
            .root_folder_id
            .is_some_and(|rf| allowed.contains(&rf));
        if !has_visible_movie && !folder_visible {
            return Err(Problem::not_found());
        }
    }
    if let Some(value) = &body.minimum_availability {
        check_availability(value)?;
    }
    if let Some(root_folder_id) = body.root_folder_id {
        let root = db::root_folders::find(&state.db, root_folder_id)
            .await
            .map_err(failed)?
            .ok_or_else(|| {
                Problem::validation("rootFolderId", "must be the id of a root folder")
            })?;
        if !caller.allows_in_library(Permission::LibraryEdit, root.library_id) {
            return Err(Problem::not_found());
        }
    }
    if let Some(profile_id) = body.quality_profile_id {
        check_profile(&state, profile_id, "qualityProfileId").await?;
    }
    let monitoring = body.monitored == Some(true);
    if monitoring {
        let Some(effective_root) = body.root_folder_id.or(before.root_folder_id) else {
            return Err(Problem::validation(
                "rootFolderId",
                "must be set to monitor a collection",
            ));
        };
        ensure_root_folder_access(&state, &caller, Permission::LibraryEdit, effective_root).await?;
        if body
            .quality_profile_id
            .or(before.quality_profile_id)
            .is_none()
        {
            return Err(Problem::validation(
                "qualityProfileId",
                "must be set to monitor a collection",
            ));
        }
    }

    // TMDB is asked before anything is stored, so a failure leaves the
    // collection as it was.
    let provider = if monitoring {
        Some(client(&state).await?)
    } else {
        None
    };
    let listed = match &provider {
        Some(provider) => match provider.collection(before.tmdb_id).await {
            Ok(collection) => collection.movies,
            Err(error) => return Err(provider_error(error)),
        },
        None => Vec::new(),
    };

    let after = db::collections::update(
        &state.db,
        id,
        Patch {
            monitored: body.monitored,
            root_folder_id: body.root_folder_id,
            quality_profile_id: body.quality_profile_id,
            minimum_availability: body.minimum_availability,
        },
    )
    .await
    .map_err(|error| match error {
        CollectionError::NotFound => Problem::not_found(),
        CollectionError::UnknownReference => Problem::validation(
            "rootFolderId",
            "and qualityProfileId must be ids of existing records",
        ),
        CollectionError::Db(error) => failed(error),
    })?;

    let changes = Changes::new()
        .field("monitored", before.monitored, after.monitored)
        .field("rootFolderId", before.root_folder_id, after.root_folder_id)
        .field(
            "qualityProfileId",
            before.quality_profile_id,
            after.quality_profile_id,
        )
        .field(
            "minimumAvailability",
            &before.minimum_availability,
            &after.minimum_availability,
        );
    let event = Event::new("collection.updated")
        .target("collection", id)
        .data(changes.into_data());
    let actor = Actor::of(&state, &caller).await;
    record(&state, &actor, &ip, event).await;

    if let Some(provider) = provider {
        add_missing(&state, provider.as_ref(), &after, &listed, &actor, &ip).await?;
    }
    let count = movie_count(&state, id).await?;
    Ok(Json(Collection::new(after, count)))
}
