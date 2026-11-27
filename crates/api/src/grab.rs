//! Sending a searched release to a download client (docs/05-api.md,
//! docs/06-state-machines.md).
//!
//! The release comes from the short-lived search cache, so a grab never asks
//! an indexer again and the indexer's download URL stays on the server.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use unseen_core::permission::Permission;
use utoipa::ToSchema;

use crate::AppState;
use crate::access::{ReleaseGrab, Require, ensure_root_folder_access};
use crate::download_clients::{ConnectedClient, connect, qb_error, sab_error};
use crate::error::Problem;

/// SABnzbd's client kind; every other kind speaks qBittorrent.
const SABNZBD: &str = "sabnzbd";
/// The protocol of Usenet releases.
const USENET: &str = "usenet";

/// Body of `POST /api/v1/releases/grab`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GrabRequest {
    /// Release id from a manual search.
    pub id: String,
}

/// The download row a grab created.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Grabbed {
    pub download_id: i32,
    pub download_client_id: i32,
    /// The torrent hash, or the nzo id SABnzbd assigned.
    pub client_download_id: String,
}

/// Sends a release of the search cache to the download client that serves the
/// movie, and records the download as `queued`.
#[utoipa::path(
    post,
    path = "/api/v1/releases/grab",
    tag = "releases",
    request_body = GrabRequest,
    responses(
        (status = 200, description = "The release was sent to the download client", body = Grabbed),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "The release id is unknown or the movie is gone", body = Problem),
        (status = 409, description = "The movie profile already has an active download, or no enabled download client serves the movie", body = Problem),
        (status = 422, description = "The profile rejected the release", body = Problem),
        (status = 502, description = "The download client did not accept the release", body = Problem),
    )
)]
pub async fn grab_release(
    caller: Require<ReleaseGrab>,
    State(state): State<AppState>,
    Json(body): Json<GrabRequest>,
) -> Result<Json<Grabbed>, Problem> {
    let release = state
        .search_cache
        .remove(&body.id)
        .ok_or_else(Problem::not_found)?;
    let movie = db::movies::find(&state.db, release.movie_id)
        .await
        .map_err(failed)?
        .ok_or_else(Problem::not_found)?;
    ensure_root_folder_access(
        &state,
        &caller,
        Permission::ReleaseGrab,
        movie.model.root_folder_id,
    )
    .await?;
    if !release.accepted {
        return Err(Problem::validation(
            "id",
            "the movie's quality profile rejected this release",
        ));
    }
    if db::downloads::has_active_for_profile(&state.db, release.movie_profile_id)
        .await
        .map_err(failed)?
    {
        return Err(Problem::conflict(
            "download_exists",
            "The movie profile already has an active download",
        ));
    }
    let usenet = release.protocol == USENET;
    let client = db::download_clients::list(&state.db)
        .await
        .map_err(failed)?
        .into_iter()
        .filter(|client| serves_movie(client, &movie.tags))
        .find(|client| (client.model.kind == SABNZBD) == usenet)
        .ok_or_else(|| {
            Problem::conflict(
                "no_download_client",
                "No enabled download client serves this movie",
            )
        })?;
    let url = release
        .magnet_url
        .clone()
        .or_else(|| release.download_url.clone())
        .ok_or_else(|| Problem::validation("id", "the release has no download link"))?;
    let category = client.model.category.clone();
    let connected = connect(&state, &client.model)?;
    let client_download_id = match connected {
        ConnectedClient::Qbittorrent(qbittorrent) => {
            match qbittorrent.add(&category, &url).await.map_err(qb_error)? {
                Some(hash) => hash,
                // qBittorrent fetches a torrent URL itself and reports no hash,
                // so the torrent is found again in the list of the category.
                None => {
                    let torrents = qbittorrent
                        .torrents(&category)
                        .await
                        .map_err(qb_error)?;
                    let wanted = release
                        .info_hash
                        .as_deref()
                        .map(str::to_ascii_lowercase);
                    let found = wanted
                        .as_deref()
                        .and_then(|hash| torrents.iter().find(|torrent| torrent.hash == hash))
                        .or_else(|| torrents.iter().find(|torrent| torrent.name == release.title))
                        .ok_or_else(|| {
                            Problem::new(
                                StatusCode::BAD_GATEWAY,
                                "client_torrent_missing",
                                "The client accepted the release but did not list it",
                            )
                        })?;
                    found.hash.clone()
                }
            }
        }
        ConnectedClient::Sabnzbd(sabnzbd) => sabnzbd
            .add(&category, &url, Some(&release.title))
            .await
            .map_err(sab_error)?
            .ok_or_else(|| {
                Problem::new(
                    StatusCode::BAD_GATEWAY,
                    "client_download_id_missing",
                    "SABnzbd accepted the NZB but returned no id",
                )
            })?,
    };
    // The indexer may have been deleted while the release sat in the cache.
    let indexer_id = db::indexers::find(&state.db, release.indexer_id)
        .await
        .map_err(failed)?
        .map(|indexer| indexer.model.id);
    let download = db::downloads::create(        &state.db,
        db::downloads::NewDownload {
            movie_id: release.movie_id,
            movie_profile_id: Some(release.movie_profile_id),
            indexer_id,
            download_client_id: Some(client.model.id),
            client_download_id: client_download_id.clone(),
            release_title: release.title.clone(),
            quality_id: release.quality_id,
            custom_format_score: release.custom_format_score,
            size: i64::try_from(release.size.unwrap_or(0)).unwrap_or(i64::MAX),
            state: "queued".into(),
        },
    )
    .await
    .map_err(failed)?;
    let data = serde_json::json!({
        "downloadId": download.id,
        "downloadClientId": client.model.id,
        "clientDownloadId": client_download_id,
        "releaseTitle": release.title,
        "indexerId": release.indexer_id,
        "qualityId": release.quality_id,
        "size": release.size,
    });
    if let Err(error) = db::history::record_grabbed(&state.db, release.movie_id, data.to_string())
        .await
    {
        // The client already holds the release: a missing activity row must not
        // turn a successful grab into an error.
        tracing::error!(%error, download_id = download.id, "could not record the grab");
    }
    Ok(Json(Grabbed {
        download_id: download.id,
        download_client_id: client.model.id,
        client_download_id,
    }))
}

/// The tag rule (docs/04-database.md): a client without tags serves every
/// movie, one with tags only the movies that share a tag with it.
fn serves_movie(client: &db::download_clients::Client, movie_tags: &[i32]) -> bool {
    client.model.enabled
        && (client.tags.is_empty() || client.tags.iter().any(|tag| movie_tags.contains(tag)))
}

fn failed(error: impl std::fmt::Display) -> Problem {
    tracing::error!(%error, "grab failed");
    Problem::internal()
}
