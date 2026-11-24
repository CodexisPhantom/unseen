//! Basic movie queries (docs/04-database.md, "Library tables").

use sea_orm::{
    ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder, QuerySelect,
};

use crate::entities::{movie_files, movie_tags, movies, tags};

pub async fn find_by_tmdb_id(
    db: &DatabaseConnection,
    tmdb_id: i32,
) -> Result<Option<movies::Model>, DbErr> {
    movies::Entity::find()
        .filter(movies::Column::TmdbId.eq(tmdb_id))
        .one(db)
        .await
}

/// The labels of a movie's tags, sorted.
pub async fn tag_labels(db: &DatabaseConnection, movie_id: i32) -> Result<Vec<String>, DbErr> {
    let ids = movie_tags::Entity::find()
        .select_only()
        .column(movie_tags::Column::TagId)
        .filter(movie_tags::Column::MovieId.eq(movie_id))
        .into_tuple::<i32>()
        .all(db)
        .await?;
    Ok(tags::Entity::find()
        .filter(tags::Column::Id.is_in(ids))
        .order_by_asc(tags::Column::Label)
        .all(db)
        .await?
        .into_iter()
        .map(|tag| tag.label)
        .collect())
}

/// Every file of a movie, managed and unmanaged, by path.
pub async fn files_of(
    db: &DatabaseConnection,
    movie_id: i32,
) -> Result<Vec<movie_files::Model>, DbErr> {
    movie_files::Entity::find()
        .filter(movie_files::Column::MovieId.eq(movie_id))
        .order_by_asc(movie_files::Column::RelativePath)
        .all(db)
        .await
}

#[cfg(test)]
mod tests {
    use sea_orm::{ActiveModelTrait, Set, SqlErr};

    use super::*;
    use crate::entities::{movie_profiles, quality_profiles, root_folders};
    use crate::testing::temp_db;

    const NOW: &str = "2026-11-24T10:00:00.000Z";

    async fn root_folder(db: &DatabaseConnection) -> i32 {
        root_folders::ActiveModel {
            path: Set("/movies".into()),
            created_at: Set(NOW.into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn profile(db: &DatabaseConnection, name: &str) -> i32 {
        quality_profiles::ActiveModel {
            name: Set(name.into()),
            upgrade_allowed: Set(true),
            cutoff_quality_id: Set(7),
            items: Set(serde_json::json!([])),
            language: Set("any".into()),
            min_format_score: Set(0),
            cutoff_format_score: Set(0),
            min_upgrade_format_score: Set(1),
            created_at: Set(NOW.into()),
            updated_at: Set(NOW.into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn movie(db: &DatabaseConnection, tmdb_id: i32, root: i32) -> Result<i32, DbErr> {
        movies::ActiveModel {
            tmdb_id: Set(tmdb_id),
            title: Set(format!("Movie {tmdb_id}")),
            sort_title: Set(format!("movie {tmdb_id}")),
            year: Set(2020),
            status: Set("released".into()),
            root_folder_id: Set(root),
            folder_name: Set(format!("Movie {tmdb_id} (2020)")),
            monitored: Set(true),
            minimum_availability: Set("released".into()),
            added_at: Set(NOW.into()),
            updated_at: Set(NOW.into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .map(|m| m.id)
    }

    async fn attach(
        db: &DatabaseConnection,
        movie_id: i32,
        quality_profile_id: i32,
        is_primary: bool,
    ) -> Result<i32, DbErr> {
        movie_profiles::ActiveModel {
            movie_id: Set(movie_id),
            quality_profile_id: Set(quality_profile_id),
            is_primary: Set(is_primary),
            created_at: Set(NOW.into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .map(|p| p.id)
    }

    async fn file(
        db: &DatabaseConnection,
        movie_id: i32,
        path: &str,
        movie_profile_id: Option<i32>,
    ) -> Result<(), DbErr> {
        movie_files::ActiveModel {
            movie_id: Set(movie_id),
            relative_path: Set(path.into()),
            size: Set(1_000),
            quality_id: Set(7),
            movie_profile_id: Set(movie_profile_id),
            added_at: Set(NOW.into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .map(|_| ())
    }

    #[tokio::test]
    async fn a_second_movie_with_the_same_tmdb_id_fails() {
        let (db, _dir) = temp_db().await;
        let root = root_folder(&db).await;
        let id = movie(&db, 603, root).await.unwrap();
        let error = movie(&db, 603, root).await.unwrap_err();
        assert!(matches!(
            error.sql_err(),
            Some(SqlErr::UniqueConstraintViolation(_))
        ));
        assert_eq!(find_by_tmdb_id(&db, 603).await.unwrap().unwrap().id, id);
        assert!(find_by_tmdb_id(&db, 1).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn deleting_a_movie_removes_its_tag_links_and_files() {
        let (db, _dir) = temp_db().await;
        let root = root_folder(&db).await;
        let id = movie(&db, 603, root).await.unwrap();
        let tag = tags::ActiveModel {
            label: Set("4k".into()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        movie_tags::ActiveModel {
            movie_id: Set(id),
            tag_id: Set(tag.id),
        }
        .insert(&db)
        .await
        .unwrap();
        file(&db, id, "a.mkv", None).await.unwrap();
        assert_eq!(tag_labels(&db, id).await.unwrap(), ["4k"]);

        movies::Entity::delete_by_id(id).exec(&db).await.unwrap();

        assert!(
            movie_tags::Entity::find()
                .all(&db)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            movie_files::Entity::find()
                .all(&db)
                .await
                .unwrap()
                .is_empty()
        );
        // The tag itself stays.
        assert_eq!(tags::Entity::find().all(&db).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn one_movie_keeps_two_managed_files_and_an_unmanaged_one() {
        let (db, _dir) = temp_db().await;
        let root = root_folder(&db).await;
        let id = movie(&db, 603, root).await.unwrap();
        let hd = attach(&db, id, profile(&db, "HD").await, true)
            .await
            .unwrap();
        let uhd = attach(&db, id, profile(&db, "4K").await, false)
            .await
            .unwrap();

        file(&db, id, "hd.mkv", Some(hd)).await.unwrap();
        file(&db, id, "uhd.mkv", Some(uhd)).await.unwrap();
        file(&db, id, "extra.mkv", None).await.unwrap();
        file(&db, id, "extra2.mkv", None).await.unwrap();
        assert_eq!(files_of(&db, id).await.unwrap().len(), 4);

        // A profile has one managed file, and a path appears once per movie.
        assert!(file(&db, id, "other.mkv", Some(hd)).await.is_err());
        assert!(file(&db, id, "extra.mkv", None).await.is_err());
    }

    #[tokio::test]
    async fn the_database_rejects_a_second_primary_profile() {
        let (db, _dir) = temp_db().await;
        let root = root_folder(&db).await;
        let id = movie(&db, 603, root).await.unwrap();
        let other = movie(&db, 604, root).await.unwrap();
        let hd = profile(&db, "HD").await;
        let uhd = profile(&db, "4K").await;
        attach(&db, id, hd, true).await.unwrap();

        assert!(attach(&db, id, uhd, true).await.is_err());
        // Another movie has its own primary, and a movie takes a profile once.
        attach(&db, other, hd, true).await.unwrap();
        assert!(attach(&db, id, hd, false).await.is_err());
        attach(&db, id, uhd, false).await.unwrap();
    }

    #[tokio::test]
    async fn a_used_root_folder_or_profile_cannot_be_deleted() {
        let (db, _dir) = temp_db().await;
        let root = root_folder(&db).await;
        let quality = profile(&db, "HD").await;
        let id = movie(&db, 603, root).await.unwrap();
        attach(&db, id, quality, true).await.unwrap();

        assert!(
            root_folders::Entity::delete_by_id(root)
                .exec(&db)
                .await
                .is_err()
        );
        assert!(
            quality_profiles::Entity::delete_by_id(quality)
                .exec(&db)
                .await
                .is_err()
        );
    }
}
