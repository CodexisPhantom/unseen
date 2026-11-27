//! Basic show, season, episode and episode file queries (docs/04-database.md, "TV tables (M7)").

use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder,
    Set,
};

use crate::entities::{
    download_episodes, episode_file_episodes, episode_files, episodes, seasons, shows,
};

/// Loads one show by id.
pub async fn find(db: &DatabaseConnection, id: i32) -> Result<Option<shows::Model>, DbErr> {
    shows::Entity::find_by_id(id).one(db).await
}

/// Loads one show by its TVDB id.
pub async fn find_by_tvdb_id(
    db: &DatabaseConnection,
    tvdb_id: i32,
) -> Result<Option<shows::Model>, DbErr> {
    shows::Entity::find()
        .filter(shows::Column::TvdbId.eq(tvdb_id))
        .one(db)
        .await
}

/// Loads one show by its TMDB id.
pub async fn find_by_tmdb_id(
    db: &DatabaseConnection,
    tmdb_id: i32,
) -> Result<Option<shows::Model>, DbErr> {
    shows::Entity::find()
        .filter(shows::Column::TmdbId.eq(tmdb_id))
        .one(db)
        .await
}

/// Every show, by sort title then id.
pub async fn list(db: &DatabaseConnection) -> Result<Vec<shows::Model>, DbErr> {
    shows::Entity::find()
        .order_by_asc(shows::Column::SortTitle)
        .order_by_asc(shows::Column::Id)
        .all(db)
        .await
}

/// Sets the monitored flag of a show. Returns `None` when missing.
pub async fn set_monitored(
    db: &DatabaseConnection,
    id: i32,
    monitored: bool,
) -> Result<Option<shows::Model>, DbErr> {
    let Some(current) = shows::Entity::find_by_id(id).one(db).await? else {
        return Ok(None);
    };
    let mut show: shows::ActiveModel = current.into();
    show.monitored = Set(monitored);
    Ok(Some(show.update(db).await?))
}

/// Every season of a show, by season number.
pub async fn seasons_of(
    db: &DatabaseConnection,
    show_id: i32,
) -> Result<Vec<seasons::Model>, DbErr> {
    seasons::Entity::find()
        .filter(seasons::Column::ShowId.eq(show_id))
        .order_by_asc(seasons::Column::SeasonNumber)
        .all(db)
        .await
}

/// Sets the monitored flag of one season. Returns `None` when missing.
pub async fn set_season_monitored(
    db: &DatabaseConnection,
    show_id: i32,
    season_number: i32,
    monitored: bool,
) -> Result<Option<seasons::Model>, DbErr> {
    let Some(current) = seasons::Entity::find_by_id((show_id, season_number))
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    let mut season: seasons::ActiveModel = current.into();
    season.monitored = Set(monitored);
    Ok(Some(season.update(db).await?))
}

/// Every episode of a show, by season then episode number.
pub async fn episodes_of(
    db: &DatabaseConnection,
    show_id: i32,
) -> Result<Vec<episodes::Model>, DbErr> {
    episodes::Entity::find()
        .filter(episodes::Column::ShowId.eq(show_id))
        .order_by_asc(episodes::Column::SeasonNumber)
        .order_by_asc(episodes::Column::EpisodeNumber)
        .all(db)
        .await
}

/// Every episode of one season, by episode number.
pub async fn episodes_of_season(
    db: &DatabaseConnection,
    show_id: i32,
    season_number: i32,
) -> Result<Vec<episodes::Model>, DbErr> {
    episodes::Entity::find()
        .filter(episodes::Column::ShowId.eq(show_id))
        .filter(episodes::Column::SeasonNumber.eq(season_number))
        .order_by_asc(episodes::Column::EpisodeNumber)
        .all(db)
        .await
}

/// Sets the monitored flag of one episode. Returns `None` when missing.
pub async fn set_episode_monitored(
    db: &DatabaseConnection,
    id: i32,
    monitored: bool,
) -> Result<Option<episodes::Model>, DbErr> {
    let Some(current) = episodes::Entity::find_by_id(id).one(db).await? else {
        return Ok(None);
    };
    let mut episode: episodes::ActiveModel = current.into();
    episode.monitored = Set(monitored);
    Ok(Some(episode.update(db).await?))
}

/// Every file of a show, by path.
pub async fn files_of(
    db: &DatabaseConnection,
    show_id: i32,
) -> Result<Vec<episode_files::Model>, DbErr> {
    episode_files::Entity::find()
        .filter(episode_files::Column::ShowId.eq(show_id))
        .order_by_asc(episode_files::Column::RelativePath)
        .all(db)
        .await
}

/// Every file covering an episode, by path.
pub async fn files_of_episode(
    db: &DatabaseConnection,
    episode_id: i32,
) -> Result<Vec<episode_files::Model>, DbErr> {
    let ids: Vec<i32> = episode_file_episodes::Entity::find()
        .filter(episode_file_episodes::Column::EpisodeId.eq(episode_id))
        .all(db)
        .await?
        .into_iter()
        .map(|link| link.episode_file_id)
        .collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    episode_files::Entity::find()
        .filter(episode_files::Column::Id.is_in(ids))
        .order_by_asc(episode_files::Column::RelativePath)
        .all(db)
        .await
}

/// Every episode a file covers, by season then episode number.
pub async fn episodes_of_file(
    db: &DatabaseConnection,
    episode_file_id: i32,
) -> Result<Vec<episodes::Model>, DbErr> {
    let ids: Vec<i32> = episode_file_episodes::Entity::find()
        .filter(episode_file_episodes::Column::EpisodeFileId.eq(episode_file_id))
        .all(db)
        .await?
        .into_iter()
        .map(|link| link.episode_id)
        .collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    episodes::Entity::find()
        .filter(episodes::Column::Id.is_in(ids))
        .order_by_asc(episodes::Column::SeasonNumber)
        .order_by_asc(episodes::Column::EpisodeNumber)
        .all(db)
        .await
}

/// Every episode a download targets, by season then episode number.
pub async fn episodes_of_download(
    db: &DatabaseConnection,
    download_id: i32,
) -> Result<Vec<episodes::Model>, DbErr> {
    let ids: Vec<i32> = download_episodes::Entity::find()
        .filter(download_episodes::Column::DownloadId.eq(download_id))
        .all(db)
        .await?
        .into_iter()
        .map(|link| link.episode_id)
        .collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    episodes::Entity::find()
        .filter(episodes::Column::Id.is_in(ids))
        .order_by_asc(episodes::Column::SeasonNumber)
        .order_by_asc(episodes::Column::EpisodeNumber)
        .all(db)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{downloads, movies, root_folders};
    use crate::testing::temp_db;

    const NOW: &str = "2026-11-24T10:00:00.000Z";

    async fn root_folder(db: &DatabaseConnection) -> i32 {
        root_folders::ActiveModel {
            path: Set("/shows".into()),
            created_at: Set(NOW.into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn show(
        db: &DatabaseConnection,
        tvdb: Option<i32>,
        tmdb: Option<i32>,
        sort_title: &str,
        root: i32,
    ) -> i32 {
        shows::ActiveModel {
            tvdb_id: Set(tvdb),
            tmdb_id: Set(tmdb),
            metadata_provider: Set(if tvdb.is_some() { "tvdb" } else { "tmdb" }.into()),
            title: Set(sort_title.into()),
            sort_title: Set(sort_title.into()),
            year: Set(2020),
            status: Set("continuing".into()),
            series_type: Set("standard".into()),
            folder_name: Set(format!("{sort_title} (2020)")),
            monitored: Set(true),
            root_folder_id: Set(root),
            added_at: Set(NOW.into()),
            updated_at: Set(NOW.into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn season(db: &DatabaseConnection, show_id: i32, number: i32, monitored: bool) {
        seasons::ActiveModel {
            show_id: Set(show_id),
            season_number: Set(number),
            monitored: Set(monitored),
        }
        .insert(db)
        .await
        .unwrap();
    }

    async fn episode(
        db: &DatabaseConnection,
        show_id: i32,
        season: i32,
        number: i32,
        monitored: bool,
    ) -> i32 {
        episodes::ActiveModel {
            show_id: Set(show_id),
            season_number: Set(season),
            episode_number: Set(number),
            title: Set(format!("S{season:02}E{number:02}")),
            monitored: Set(monitored),
            added_at: Set(NOW.into()),
            updated_at: Set(NOW.into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn file(db: &DatabaseConnection, show_id: i32, path: &str) -> i32 {
        episode_files::ActiveModel {
            show_id: Set(show_id),
            relative_path: Set(path.into()),
            size: Set(1_000),
            quality_id: Set(7),
            added_at: Set(NOW.into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn link_file(db: &DatabaseConnection, file_id: i32, episode_id: i32) {
        episode_file_episodes::ActiveModel {
            episode_file_id: Set(file_id),
            episode_id: Set(episode_id),
        }
        .insert(db)
        .await
        .unwrap();
    }

    async fn link_download(db: &DatabaseConnection, download_id: i32, episode_id: i32) {
        download_episodes::ActiveModel {
            download_id: Set(download_id),
            episode_id: Set(episode_id),
        }
        .insert(db)
        .await
        .unwrap();
    }

    fn check_violation(error: &DbErr) -> bool {
        error.to_string().contains("CHECK constraint failed")
    }

    #[tokio::test]
    async fn finds_shows_by_provider_id_and_lists_by_sort_title() {
        let (db, _dir) = temp_db().await;
        let root = root_folder(&db).await;
        let alpha = show(&db, Some(1), Some(10), "alpha", root).await;
        show(&db, None, Some(20), "zeta", root).await;

        assert_eq!(find(&db, alpha).await.unwrap().unwrap().id, alpha);
        assert_eq!(find_by_tvdb_id(&db, 1).await.unwrap().unwrap().id, alpha);
        assert!(find_by_tvdb_id(&db, 999).await.unwrap().is_none());
        assert_eq!(
            find_by_tmdb_id(&db, 20).await.unwrap().unwrap().sort_title,
            "zeta"
        );
        assert!(find_by_tmdb_id(&db, 999).await.unwrap().is_none());

        let listed = list(&db).await.unwrap();
        assert_eq!(
            listed
                .iter()
                .map(|s| s.sort_title.clone())
                .collect::<Vec<_>>(),
            ["alpha", "zeta"]
        );
        let updated = set_monitored(&db, alpha, false).await.unwrap().unwrap();
        assert!(!updated.monitored);
        assert!(set_monitored(&db, 999, false).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn seasons_and_episodes_are_listed_and_monitored_independently() {
        let (db, _dir) = temp_db().await;
        let root = root_folder(&db).await;
        let id = show(&db, Some(1), None, "alpha", root).await;
        season(&db, id, 0, true).await;
        season(&db, id, 1, true).await;
        let e2 = episode(&db, id, 1, 2, true).await;
        let e1 = episode(&db, id, 1, 1, true).await;

        let seasons = seasons_of(&db, id).await.unwrap();
        assert_eq!(
            seasons.iter().map(|s| s.season_number).collect::<Vec<_>>(),
            [0, 1]
        );
        let all = episodes_of(&db, id).await.unwrap();
        assert_eq!(all.iter().map(|e| e.id).collect::<Vec<_>>(), [e1, e2]);
        let s1 = episodes_of_season(&db, id, 1).await.unwrap();
        assert_eq!(s1.len(), 2);
        assert!(episodes_of_season(&db, id, 0).await.unwrap().is_empty());

        let updated = set_season_monitored(&db, id, 1, false)
            .await
            .unwrap()
            .unwrap();
        assert!(!updated.monitored);
        let seasons = seasons_of(&db, id).await.unwrap();
        assert!(seasons[0].monitored);
        assert!(!seasons[1].monitored);
        assert!(
            set_season_monitored(&db, id, 9, false)
                .await
                .unwrap()
                .is_none()
        );

        let updated = set_episode_monitored(&db, e1, false)
            .await
            .unwrap()
            .unwrap();
        assert!(!updated.monitored);
        assert!(episodes_of(&db, id).await.unwrap()[1].monitored);
        assert!(
            set_episode_monitored(&db, 999, false)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn an_episode_file_covers_several_episodes() {
        let (db, _dir) = temp_db().await;
        let root = root_folder(&db).await;
        let id = show(&db, Some(1), None, "alpha", root).await;
        season(&db, id, 1, true).await;
        let e1 = episode(&db, id, 1, 1, true).await;
        let e2 = episode(&db, id, 1, 2, true).await;

        let f = file(&db, id, "b.mkv").await;
        link_file(&db, f, e1).await;
        link_file(&db, f, e2).await;

        assert_eq!(files_of(&db, id).await.unwrap().len(), 1);
        assert_eq!(files_of_episode(&db, e1).await.unwrap().len(), 1);
        assert_eq!(files_of_episode(&db, e2).await.unwrap().len(), 1);
        let covered = episodes_of_file(&db, f).await.unwrap();
        assert_eq!(covered.iter().map(|e| e.id).collect::<Vec<_>>(), [e1, e2]);

        let g = file(&db, id, "a.mkv").await;
        link_file(&db, g, e1).await;
        let by_path = files_of(&db, id).await.unwrap();
        assert_eq!(by_path.iter().map(|x| x.id).collect::<Vec<_>>(), [g, f]);
        let of_ep = files_of_episode(&db, e1).await.unwrap();
        assert_eq!(of_ep.iter().map(|x| x.id).collect::<Vec<_>>(), [g, f]);
    }

    #[tokio::test]
    async fn deleting_a_show_cascades_to_its_rows() {
        let (db, _dir) = temp_db().await;
        let root = root_folder(&db).await;
        let id = show(&db, Some(1), None, "alpha", root).await;
        season(&db, id, 1, true).await;
        let e1 = episode(&db, id, 1, 1, true).await;
        let e2 = episode(&db, id, 1, 2, true).await;
        let f = file(&db, id, "a.mkv").await;
        link_file(&db, f, e1).await;
        link_file(&db, f, e2).await;
        let dl = downloads::ActiveModel {
            movie_id: Set(None),
            show_id: Set(Some(id)),
            client_download_id: Set("hash-tv".into()),
            release_title: Set("Show.S01E01.1080p".into()),
            quality_id: Set(7),
            size: Set(1),
            state: Set("downloading".into()),
            grabbed_at: Set(NOW.into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        link_download(&db, dl.id, e1).await;

        shows::Entity::delete_by_id(id).exec(&db).await.unwrap();

        assert!(seasons::Entity::find().all(&db).await.unwrap().is_empty());
        assert!(episodes::Entity::find().all(&db).await.unwrap().is_empty());
        assert!(
            episode_files::Entity::find()
                .all(&db)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            episode_file_episodes::Entity::find()
                .all(&db)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            download_episodes::Entity::find()
                .all(&db)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(downloads::Entity::find().all(&db).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_download_targets_a_movie_or_a_show() {
        let (db, _dir) = temp_db().await;
        let root = root_folder(&db).await;
        let movie = movies::ActiveModel {
            tmdb_id: Set(603),
            title: Set("The Matrix".into()),
            sort_title: Set("matrix".into()),
            year: Set(1999),
            status: Set("released".into()),
            root_folder_id: Set(root),
            folder_name: Set("The Matrix (1999)".into()),
            monitored: Set(true),
            minimum_availability: Set("released".into()),
            added_at: Set(NOW.into()),
            updated_at: Set(NOW.into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let show_id = show(&db, Some(1), None, "alpha", root).await;
        season(&db, show_id, 1, true).await;
        let ep = episode(&db, show_id, 1, 1, true).await;

        let movie_dl = downloads::ActiveModel {
            movie_id: Set(Some(movie.id)),
            show_id: Set(None),
            client_download_id: Set("hash-movie".into()),
            release_title: Set("The.Matrix.1999.1080p".into()),
            quality_id: Set(7),
            size: Set(1_000),
            state: Set("downloading".into()),
            grabbed_at: Set(NOW.into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        assert_eq!(movie_dl.movie_id, Some(movie.id));
        assert_eq!(movie_dl.show_id, None);

        let show_dl = downloads::ActiveModel {
            movie_id: Set(None),
            show_id: Set(Some(show_id)),
            client_download_id: Set("hash-show".into()),
            release_title: Set("Show.S01E01.1080p".into()),
            quality_id: Set(7),
            size: Set(1_000),
            state: Set("downloading".into()),
            grabbed_at: Set(NOW.into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        link_download(&db, show_dl.id, ep).await;
        let eps = episodes_of_download(&db, show_dl.id).await.unwrap();
        assert_eq!(eps.len(), 1);
        assert_eq!(eps[0].id, ep);

        let both = downloads::ActiveModel {
            movie_id: Set(Some(movie.id)),
            show_id: Set(Some(show_id)),
            client_download_id: Set("hash-both".into()),
            release_title: Set("Both".into()),
            quality_id: Set(7),
            size: Set(1),
            state: Set("downloading".into()),
            grabbed_at: Set(NOW.into()),
            ..Default::default()
        }
        .insert(&db)
        .await;
        let error = both.unwrap_err();
        assert!(
            check_violation(&error),
            "expected the target check, got {error}"
        );
        let neither = downloads::ActiveModel {
            movie_id: Set(None),
            show_id: Set(None),
            client_download_id: Set("hash-neither".into()),
            release_title: Set("Neither".into()),
            quality_id: Set(7),
            size: Set(1),
            state: Set("downloading".into()),
            grabbed_at: Set(NOW.into()),
            ..Default::default()
        }
        .insert(&db)
        .await;
        let error = neither.unwrap_err();
        assert!(
            check_violation(&error),
            "expected the target check, got {error}"
        );
    }
}
