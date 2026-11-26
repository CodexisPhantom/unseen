//! Manual release search for a movie (docs/05-api.md, docs/07-release-scoring.md,
//! docs/08-integrations.md).
//!
//! Queries all enabled indexers in parallel, parses and scores every release
//! against one of the movie's quality profiles, caches the results so a grab
//! can reference them by id, and reports per-indexer failures alongside the
//! merged results.

use std::collections::HashMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use chrono::Utc;
use db::entities::movie_files;
use db::secret::decrypt;
use serde::{Deserialize, Serialize};
use tokio::task::JoinSet;
use unseen_core::newznab::{MovieQuery as NewznabMovieQuery, NewznabClient, NzbRelease};
use unseen_core::permission::Permission;
use unseen_core::profile::{Item, Profile};
use unseen_core::quality::{Quality, SizeLimits};
use unseen_core::release;
use unseen_core::scoring::{self, ProtocolPreference, ReleaseRef};
use unseen_core::search::CachedRelease;
use unseen_core::torznab::{MovieQuery, Release as TorznabRelease, TorznabClient};
use utoipa::{IntoParams, ToSchema};

use crate::AppState;
use crate::access::{Require, SearchRun, ensure_root_folder_access};
use crate::auth::hex;
use crate::error::Problem;
use crate::proxy::indexer_client;

/// Supported indexer kinds.
const TORZNAB: &str = "torznab";
const NEWZNAB: &str = "newznab";

/// Query of `GET /api/v1/movies/{id}/releases`.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(rename_all = "camelCase")]
pub struct SearchParams {
    /// Movie profile attachment id (`profiles[].id`). Defaults to the movie's
    /// primary profile.
    profile_id: Option<i32>,
}

/// Why a release was rejected by the profile or movie rules.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseRejection {
    /// Stable machine-readable rejection code, e.g. `quality_not_allowed`.
    pub code: String,
    /// Human-readable explanation.
    pub reason: String,
}

/// The factors used to rank a release (docs/07-release-scoring.md, "Scoring").
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseScore {
    /// 1-based position in the sorted list (1 is best).
    pub rank: u32,
    /// Position of the quality in the profile, higher is better. Absent when
    /// the quality is not listed in the profile.
    pub quality_rank: Option<u32>,
    /// Total custom format score for the profile (0 until custom formats are
    /// evaluated).
    pub custom_format_score: i32,
    /// Revision number: 1 for a first release, 2 for PROPER/REPACK, and higher
    /// for subsequent revisions.
    pub revision: u8,
    /// Indexer priority (1 to 50; lower wins).
    pub indexer_priority: i32,
    /// Seeders on a log10 scale (`round(log10(seeders + 1))`).
    pub seeder_score: u32,
    /// Distance in bytes from the quality definition's preferred size, when
    /// both are known.
    pub preferred_size_distance: Option<u64>,
}

/// One parsed and scored release returned from a manual search.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    /// Short-lived cache reference id to pass to grab.
    pub id: String,
    pub guid: String,
    pub title: String,
    pub indexer_id: i32,
    pub indexer_name: String,
    /// Radarr quality id (`0` is `Unknown`).
    pub quality_id: i32,
    /// Radarr quality name, e.g. `Bluray-1080p`.
    pub quality_name: String,
    /// Size in bytes, if reported by the indexer.
    pub size: Option<u64>,
    pub seeders: Option<u32>,
    pub leechers: Option<u32>,
    /// UTC ISO 8601 publish timestamp, if reported by the indexer.
    pub published_at: Option<String>,
    /// Age in hours since `publishedAt`.
    pub age_hours: Option<f64>,
    /// Download protocol (`torrent`).
    pub protocol: String,
    /// Lowercase ISO 639-1 language codes (or `multi`) parsed from the title.
    pub languages: Vec<String>,
    pub release_group: Option<String>,
    pub edition: Option<String>,
    pub proper: bool,
    pub repack: bool,
    pub accepted: bool,
    /// Empty when `accepted` is true; otherwise every broken rule in order.
    pub rejections: Vec<ReleaseRejection>,
    pub score: ReleaseScore,
}

/// A failure from one indexer during a manual search.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct IndexerSearchError {
    pub indexer_id: i32,
    pub indexer_name: String,
    pub message: String,
}

/// Result of a manual release search for one movie and one movie profile.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MovieReleases {
    pub movie_id: i32,
    pub movie_profile_id: i32,
    pub quality_profile_id: i32,
    /// Accepted releases first, then rejected releases, sorted best-first.
    pub releases: Vec<Release>,
    /// Per-indexer failures; a failing indexer does not fail the search.
    pub indexer_errors: Vec<IndexerSearchError>,
}

enum FetchedPayload {
    Torrent(TorznabRelease),
    Usenet(NzbRelease),
}

impl FetchedPayload {
    fn as_release_ref(&self) -> ReleaseRef<'_> {
        match self {
            Self::Torrent(r) => r.into(),
            Self::Usenet(r) => r.into(),
        }
    }
}

struct FetchedRelease {
    indexer_id: i32,
    indexer_name: String,
    indexer_priority: i32,
    indexer_tags: Vec<i32>,
    payload: FetchedPayload,
}

struct IndexerOutcome {
    order: usize,
    indexer_id: i32,
    indexer_name: String,
    indexer_priority: i32,
    indexer_tags: Vec<i32>,
    result: Result<Vec<FetchedPayload>, String>,
}

fn failed(error: impl std::fmt::Display) -> Problem {
    tracing::error!(%error, "releases");
    Problem::internal()
}

fn existing_file_version(file: &movie_files::Model) -> u8 {
    let name = file.scene_name.as_deref().unwrap_or_else(|| {
        file.relative_path
            .rsplit('/')
            .next()
            .unwrap_or(&file.relative_path)
    });
    release::parse(name).version
}

/// Queries all enabled indexers in parallel for releases of a movie, parses
/// and scores each result against one of the movie's quality profiles, and
/// caches the results so a grab can reference them by id.
#[utoipa::path(
    get,
    path = "/api/v1/movies/{id}/releases",
    tag = "releases",
    params(
        ("id" = i32, Path, description = "Movie id"),
        SearchParams,
    ),
    responses(
        (status = 200, description = "Scored releases and per-indexer errors", body = MovieReleases),
        (status = 403, description = "Permission denied", body = Problem),
        (status = 404, description = "No such movie", body = Problem),
        (status = 422, description = "Invalid profileId", body = Problem),
    )
)]
pub async fn search_movie(
    caller: Require<SearchRun>,
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Query(params): Query<SearchParams>,
) -> Result<Json<MovieReleases>, Problem> {
    let movie = db::movies::find(&state.db, id)
        .await
        .map_err(failed)?
        .ok_or_else(Problem::not_found)?;
    ensure_root_folder_access(
        &state,
        &caller,
        Permission::SearchRun,
        movie.model.root_folder_id,
    )
    .await?;

    let selected_profile = match params.profile_id {
        Some(profile_id) => movie
            .profiles
            .iter()
            .find(|profile| profile.id == profile_id)
            .ok_or_else(|| Problem::validation("profileId", "must be a profile of this movie"))?,
        None => movie
            .profiles
            .iter()
            .find(|profile| profile.is_primary)
            .or_else(|| movie.profiles.first())
            .ok_or_else(|| failed("movie has no quality profile"))?,
    };

    let quality_profile_model = db::profiles::find(&state.db, selected_profile.quality_profile_id)
        .await
        .map_err(failed)?
        .ok_or_else(|| failed("movie profile references missing quality profile"))?;
    let profile_items: Vec<Item> =
        serde_json::from_value(quality_profile_model.items).map_err(failed)?;
    let scoring_profile = Profile {
        name: quality_profile_model.name,
        items: profile_items,
        cutoff_quality_id: quality_profile_model.cutoff_quality_id,
        language: quality_profile_model.language,
        min_upgrade_format_score: quality_profile_model.min_upgrade_format_score,
    };

    let size_limits: HashMap<Quality, SizeLimits> = db::quality_definitions::list(&state.db)
        .await
        .map_err(failed)?
        .into_iter()
        .filter_map(|row| {
            let quality = Quality::from_id(row.quality_id)?;
            Some((
                quality,
                SizeLimits {
                    min: row.min_size,
                    preferred: row.preferred_size,
                    max: row.max_size,
                },
            ))
        })
        .collect();

    let existing = match selected_profile.file_id {
        Some(file_id) => db::movies::find_file(&state.db, movie.model.id, file_id)
            .await
            .map_err(failed)?
            .map(|file| scoring::Existing {
                quality: Quality::from_id(file.quality_id).unwrap_or(Quality::Unknown),
                version: existing_file_version(&file),
            }),
        None => None,
    };

    let enabled_indexers: Vec<_> = db::indexers::list(&state.db)
        .await
        .map_err(failed)?
        .into_iter()
        .filter(|indexer| indexer.model.enabled)
        .collect();

    let proxied_client = if enabled_indexers.iter().any(|i| i.model.use_proxy) {
        Some(indexer_client(&state, true).await)
    } else {
        None
    };
    let direct_client = if enabled_indexers.iter().any(|i| !i.model.use_proxy) {
        Some(indexer_client(&state, false).await)
    } else {
        None
    };

    let mut ordered_errors: Vec<(usize, IndexerSearchError)> = Vec::new();
    let mut join_set: JoinSet<IndexerOutcome> = JoinSet::new();

    for (order, indexer) in enabled_indexers.into_iter().enumerate() {
        let model = indexer.model;
        let kind = model.kind.clone();
        if !matches!(kind.as_str(), TORZNAB | NEWZNAB) {
            ordered_errors.push((
                order,
                IndexerSearchError {
                    indexer_id: model.id,
                    indexer_name: model.name,
                    message: format!("unsupported indexer kind `{kind}`"),
                },
            ));
            continue;
        }
        let api_key = match decrypt(&state.secrets, &model.api_key) {
            Ok(key) => key,
            Err(_) => {
                ordered_errors.push((
                    order,
                    IndexerSearchError {
                        indexer_id: model.id,
                        indexer_name: model.name,
                        message: "The stored API key cannot be read; enter it again".to_owned(),
                    },
                ));
                continue;
            }
        };
        let categories: Vec<i32> = match serde_json::from_value(model.categories) {
            Ok(categories) => categories,
            Err(_) => {
                ordered_errors.push((
                    order,
                    IndexerSearchError {
                        indexer_id: model.id,
                        indexer_name: model.name,
                        message: "The stored categories could not be read".to_owned(),
                    },
                ));
                continue;
            }
        };
        let client_res = if model.use_proxy {
            proxied_client.as_ref().expect("built for proxied indexers")
        } else {
            direct_client.as_ref().expect("built for direct indexers")
        };
        let http = match client_res {
            Ok(http) => http.clone(),
            Err(problem) => {
                ordered_errors.push((
                    order,
                    IndexerSearchError {
                        indexer_id: model.id,
                        indexer_name: model.name,
                        message: problem
                            .detail
                            .clone()
                            .unwrap_or_else(|| problem.title.clone()),
                    },
                ));
                continue;
            }
        };

        let indexer_id = model.id;
        let indexer_name = model.name;
        let indexer_url = model.url;
        let indexer_priority = model.priority;
        let indexer_tags = indexer.tags;
        let tmdb_id = Some(movie.model.tmdb_id);
        let imdb_id = movie.model.imdb_id.clone();
        let title = movie.model.title.clone();
        let year = Some(movie.model.year);

        join_set.spawn(async move {
            let result = match kind.as_str() {
                NEWZNAB => {
                    let query = NewznabMovieQuery {
                        tmdb_id,
                        imdb_id,
                        title,
                        year,
                        categories,
                    };
                    let client = NewznabClient::new(http, &indexer_url, api_key.expose());
                    async {
                        let caps = client.caps().await?;
                        client.search_movie(&caps, &query).await
                    }
                    .await
                    .map(|items| items.into_iter().map(FetchedPayload::Usenet).collect())
                    .map_err(|error| error.to_string())
                }
                _ => {
                    let query = MovieQuery {
                        tmdb_id,
                        imdb_id,
                        title,
                        year,
                        categories,
                    };
                    let client = TorznabClient::new(http, &indexer_url, api_key.expose());
                    async {
                        let caps = client.caps().await?;
                        client.search_movie(&caps, &query).await
                    }
                    .await
                    .map(|items| items.into_iter().map(FetchedPayload::Torrent).collect())
                    .map_err(|error| error.to_string())
                }
            };
            IndexerOutcome {
                order,
                indexer_id,
                indexer_name,
                indexer_priority,
                indexer_tags,
                result,
            }
        });
    }

    let mut fetched_batches: Vec<(usize, Vec<FetchedRelease>)> = Vec::new();
    while let Some(joined) = join_set.join_next().await {
        match joined {
            Ok(outcome) => match outcome.result {
                Ok(found) => {
                    let batch = found
                        .into_iter()
                        .map(|payload| FetchedRelease {
                            indexer_id: outcome.indexer_id,
                            indexer_name: outcome.indexer_name.clone(),
                            indexer_priority: outcome.indexer_priority,
                            indexer_tags: outcome.indexer_tags.clone(),
                            payload,
                        })
                        .collect();
                    fetched_batches.push((outcome.order, batch));
                }
                Err(message) => {
                    tracing::warn!(
                        indexer_id = outcome.indexer_id,
                        indexer = %outcome.indexer_name,
                        error = %message,
                        "indexer search failed"
                    );
                    ordered_errors.push((
                        outcome.order,
                        IndexerSearchError {
                            indexer_id: outcome.indexer_id,
                            indexer_name: outcome.indexer_name,
                            message,
                        },
                    ));
                }
            },
            Err(error) => {
                tracing::error!(%error, "indexer search task failed");
            }
        }
    }

    fetched_batches.sort_by_key(|(order, _)| *order);
    let fetched: Vec<FetchedRelease> = fetched_batches
        .into_iter()
        .flat_map(|(_, batch)| batch)
        .collect();

    ordered_errors.sort_by_key(|(order, _)| *order);
    let indexer_errors: Vec<IndexerSearchError> =
        ordered_errors.into_iter().map(|(_, error)| error).collect();

    let now = Utc::now();
    let scoring_ctx = scoring::Context {
        movie: scoring::Movie {
            title: &movie.model.title,
            year: u16::try_from(movie.model.year).ok(),
            runtime_minutes: movie
                .model
                .runtime
                .and_then(|minutes| u32::try_from(minutes).ok()),
            original_language: None,
            tags: &movie.tags,
        },
        profile: &scoring_profile,
        upgrade_allowed: quality_profile_model.upgrade_allowed,
        size_limits: &size_limits,
        existing,
        protocol_preference: ProtocolPreference::None,
        retention_days: None,
        now: Some(now),
    };

    let ranked = scoring::rank_entries(
        &scoring_ctx,
        fetched.iter().map(|item| {
            let candidate = scoring::Candidate {
                release: item.payload.as_release_ref(),
                indexer_priority: item.indexer_priority,
                indexer_tags: &item.indexer_tags,
            };
            (item, candidate)
        }),
    );

    let now_secs = now.timestamp();
    let mut releases = Vec::with_capacity(ranked.len());
    let mut cache_entries = Vec::with_capacity(ranked.len());

    for (index, (fetched_item, decision)) in ranked.into_iter().enumerate() {
        let release_id = hex(&rand::random::<[u8; 16]>());
        let raw = decision.candidate.release;
        let accepted = decision.accepted();
        let protocol = decision.protocol().as_str().to_owned();
        let (download_url, magnet_url, info_hash, seeders, leechers) = match raw {
            ReleaseRef::Torrent(torrent) => {
                let leechers = torrent
                    .peers
                    .zip(torrent.seeders)
                    .map(|(peers, seeders)| peers.saturating_sub(seeders));
                (
                    torrent.download_url.clone(),
                    torrent.magnet_url.clone(),
                    torrent.info_hash.clone(),
                    torrent.seeders,
                    leechers,
                )
            }
            ReleaseRef::Usenet(nzb) => (Some(nzb.download_url.clone()), None, None, None, None),
        };
        let published = raw.published();
        let published_at = published.map(|dt| dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string());
        let age_hours = published.map(|dt| {
            let elapsed = (now_secs - dt.timestamp()).max(0);
            elapsed as f64 / 3600.0
        });
        let languages = decision
            .parsed
            .languages
            .iter()
            .map(|language| language.code().to_owned())
            .collect();
        let rejections = decision
            .rejections
            .iter()
            .map(|rejection| ReleaseRejection {
                code: rejection.code().to_owned(),
                reason: rejection.reason().to_owned(),
            })
            .collect();
        let score = ReleaseScore {
            rank: u32::try_from(index + 1).unwrap_or(u32::MAX),
            quality_rank: decision
                .quality_rank()
                .and_then(|rank| u32::try_from(rank).ok()),
            custom_format_score: 0,
            revision: decision.parsed.version,
            indexer_priority: fetched_item.indexer_priority,
            seeder_score: decision.seeder_score(),
            preferred_size_distance: decision.preferred_distance(),
        };

        cache_entries.push((
            release_id.clone(),
            CachedRelease {
                movie_id: movie.model.id,
                movie_profile_id: selected_profile.id,
                indexer_id: fetched_item.indexer_id,
                indexer_name: fetched_item.indexer_name.clone(),
                guid: raw.guid().to_owned(),
                title: raw.title().to_owned(),
                download_url,
                magnet_url,
                info_hash,
                size: raw.size(),
                quality_id: decision.quality.id(),
                custom_format_score: 0,
                protocol: protocol.clone(),
                accepted,
            },
        ));

        releases.push(Release {
            id: release_id,
            guid: raw.guid().to_owned(),
            title: raw.title().to_owned(),
            indexer_id: fetched_item.indexer_id,
            indexer_name: fetched_item.indexer_name.clone(),
            quality_id: decision.quality.id(),
            quality_name: decision.quality.name().to_owned(),
            size: raw.size(),
            seeders,
            leechers,
            published_at,
            age_hours,
            protocol,
            languages,
            release_group: decision.parsed.release_group,
            edition: decision.parsed.edition,
            proper: decision.parsed.proper,
            repack: decision.parsed.repack,
            accepted,
            rejections,
            score,
        });
    }

    state.search_cache.insert_batch(cache_entries);

    Ok(Json(MovieReleases {
        movie_id: movie.model.id,
        movie_profile_id: selected_profile.id,
        quality_profile_id: selected_profile.quality_profile_id,
        releases,
        indexer_errors,
    }))
}
