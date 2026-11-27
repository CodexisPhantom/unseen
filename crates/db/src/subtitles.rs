//! Language profiles and subtitle files (docs/04-database.md, "Subtitle
//! tables (M10)"). The items of a profile are stored as one JSON list;
//! `core::subtitle` defines and validates them.

use std::collections::HashSet;

use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, Set, SqlErr, TransactionTrait,
};
use unseenarr_core::subtitle::Kind;

use crate::entities::{language_profiles, movie_files, movies, subtitles};

/// The setting that holds the language profile of a new movie.
pub const DEFAULT_PROFILE_KEY: &str = "subtitles.default_profile_id";

/// What a caller sets on a language profile.
#[derive(Debug, Clone, PartialEq)]
pub struct Values {
    pub name: String,
    pub items: serde_json::Value,
    pub cutoff: Option<i32>,
}

#[derive(Debug)]
pub enum ProfileError {
    NotFound,
    NameTaken,
    /// A movie uses the profile.
    InUse,
    Db(DbErr),
}

impl From<DbErr> for ProfileError {
    fn from(error: DbErr) -> Self {
        if matches!(error.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) {
            Self::NameTaken
        } else {
            Self::Db(error)
        }
    }
}

/// Every language profile, by name.
pub async fn list_profiles(
    db: &DatabaseConnection,
) -> Result<Vec<language_profiles::Model>, DbErr> {
    language_profiles::Entity::find()
        .order_by_asc(language_profiles::Column::Name)
        .all(db)
        .await
}

pub async fn find_profile(
    db: &DatabaseConnection,
    id: i32,
) -> Result<Option<language_profiles::Model>, DbErr> {
    language_profiles::Entity::find_by_id(id).one(db).await
}

pub async fn create_profile(
    db: &DatabaseConnection,
    values: Values,
) -> Result<language_profiles::Model, ProfileError> {
    let now = crate::now();
    Ok(language_profiles::ActiveModel {
        name: Set(values.name),
        items: Set(values.items),
        cutoff: Set(values.cutoff),
        created_at: Set(now.clone()),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await?)
}

/// Replaces every editable field.
pub async fn update_profile(
    db: &DatabaseConnection,
    id: i32,
    values: Values,
) -> Result<language_profiles::Model, ProfileError> {
    let mut profile: language_profiles::ActiveModel = find_profile(db, id)
        .await?
        .ok_or(ProfileError::NotFound)?
        .into();
    profile.name = Set(values.name);
    profile.items = Set(values.items);
    profile.cutoff = Set(values.cutoff);
    profile.updated_at = Set(crate::now());
    Ok(profile.update(db).await?)
}

/// Deletes a profile that no movie uses. When it is the default profile, the
/// default is cleared with it.
pub async fn delete_profile(db: &DatabaseConnection, id: i32) -> Result<(), ProfileError> {
    if movie_count(db, id).await? > 0 {
        return Err(ProfileError::InUse);
    }
    if language_profiles::Entity::delete_by_id(id)
        .exec(db)
        .await?
        .rows_affected
        == 0
    {
        return Err(ProfileError::NotFound);
    }
    if crate::settings::get_i64(db, DEFAULT_PROFILE_KEY).await? == Some(id.into()) {
        crate::settings::remove(db, DEFAULT_PROFILE_KEY).await?;
    }
    Ok(())
}

/// How many movies use the profile.
pub async fn movie_count(db: &DatabaseConnection, id: i32) -> Result<u64, DbErr> {
    movies::Entity::find()
        .filter(movies::Column::LanguageProfileId.eq(id))
        .count(db)
        .await
}

/// The profile of a new movie, from the setting. `None` when it is not set
/// or points at a profile that is gone.
pub async fn default_profile(db: &DatabaseConnection) -> Result<Option<i32>, DbErr> {
    let Some(id) = crate::settings::get_i64(db, DEFAULT_PROFILE_KEY)
        .await?
        .and_then(|id| i32::try_from(id).ok())
    else {
        return Ok(None);
    };
    Ok(find_profile(db, id).await?.map(|profile| profile.id))
}

/// Sets the language profile of a movie, or clears it with `None`. `false`
/// when there is no such movie.
pub async fn set_movie_profile(
    db: &DatabaseConnection,
    movie_id: i32,
    profile_id: Option<i32>,
) -> Result<bool, DbErr> {
    Ok(movies::Entity::update_many()
        .col_expr(movies::Column::LanguageProfileId, profile_id.into())
        .col_expr(movies::Column::UpdatedAt, crate::now().into())
        .filter(movies::Column::Id.eq(movie_id))
        .exec(db)
        .await?
        .rows_affected
        > 0)
}

/// Every subtitle of every file of a movie, by file, language and kind.
pub async fn of_movie(
    db: &DatabaseConnection,
    movie_id: i32,
) -> Result<Vec<subtitles::Model>, DbErr> {
    let files: Vec<i32> = movie_files::Entity::find()
        .filter(movie_files::Column::MovieId.eq(movie_id))
        .all(db)
        .await?
        .into_iter()
        .map(|file| file.id)
        .collect();
    subtitles::Entity::find()
        .filter(subtitles::Column::MovieFileId.is_in(files))
        .order_by_asc(subtitles::Column::MovieFileId)
        .order_by_asc(subtitles::Column::Language)
        .order_by_asc(subtitles::Column::Kind)
        .all(db)
        .await
}

/// A subtitle file found next to a video.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub language: String,
    pub kind: Kind,
    /// Relative to the movie folder, with `/` between the parts.
    pub relative_path: String,
}

/// Makes the subtitles of a file without a provider match what is on disk:
/// new ones are added, and ones whose file is gone are removed. When two
/// files have the same language and kind, the first of `found` wins. A
/// subtitle that came from a provider is never changed or replaced.
pub async fn sync_external(
    db: &DatabaseConnection,
    movie_file_id: i32,
    found: &[Found],
) -> Result<(), DbErr> {
    let txn = db.begin().await?;
    let existing = subtitles::Entity::find()
        .filter(subtitles::Column::MovieFileId.eq(movie_file_id))
        .all(&txn)
        .await?;
    let mut taken: HashSet<(String, String)> = existing
        .iter()
        .map(|row| (row.language.clone(), row.kind.clone()))
        .collect();
    let on_disk: HashSet<&str> = found.iter().map(|f| f.relative_path.as_str()).collect();
    for row in &existing {
        if row.provider.is_none() && !on_disk.contains(row.relative_path.as_str()) {
            subtitles::Entity::delete_by_id(row.id).exec(&txn).await?;
            taken.remove(&(row.language.clone(), row.kind.clone()));
        }
    }
    for file in found {
        if !taken.insert((file.language.clone(), file.kind.as_str().to_owned())) {
            continue;
        }
        subtitles::ActiveModel {
            movie_file_id: Set(movie_file_id),
            language: Set(file.language.clone()),
            kind: Set(file.kind.as_str().to_owned()),
            relative_path: Set(file.relative_path.clone()),
            added_at: Set(crate::now()),
            ..Default::default()
        }
        .insert(&txn)
        .await?;
    }
    txn.commit().await
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::testing::temp_db;

    fn values(name: &str) -> Values {
        Values {
            name: name.to_owned(),
            items: json!([{ "language": "en", "kind": "normal" }]),
            cutoff: None,
        }
    }

    fn found(language: &str, kind: Kind, path: &str) -> Found {
        Found {
            language: language.to_owned(),
            kind,
            relative_path: path.to_owned(),
        }
    }

    /// A movie with one file, built with SQL to stay independent of the
    /// movie code.
    async fn movie_with_file(db: &DatabaseConnection) -> (i32, i32) {
        use sea_orm::{ConnectionTrait, Statement};
        let backend = db.get_database_backend();
        for sql in [
            "INSERT INTO root_folders (id, path, created_at) VALUES (1, '/m', 'now')",
            "INSERT INTO movies (id, tmdb_id, title, sort_title, year, status, root_folder_id, \
             folder_name, monitored, minimum_availability, added_at, updated_at) \
             VALUES (1, 603, 'The Matrix', 'matrix', 1999, 'released', 1, 'The Matrix (1999)', \
             1, 'released', 'now', 'now')",
            "INSERT INTO movie_files (id, movie_id, relative_path, size, quality_id, added_at) \
             VALUES (1, 1, 'The Matrix.mkv', 1, 0, 'now')",
        ] {
            db.execute_raw(Statement::from_string(backend, sql))
                .await
                .unwrap();
        }
        (1, 1)
    }

    #[tokio::test]
    async fn profiles_are_created_renamed_and_listed_by_name() {
        let (db, _dir) = temp_db().await;
        let b = create_profile(&db, values("B")).await.unwrap();
        create_profile(&db, values("A")).await.unwrap();
        assert!(matches!(
            create_profile(&db, values("A")).await,
            Err(ProfileError::NameTaken)
        ));
        let names: Vec<_> = list_profiles(&db)
            .await
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(names, ["A", "B"]);

        let mut changed = values("C");
        changed.cutoff = Some(0);
        let updated = update_profile(&db, b.id, changed).await.unwrap();
        assert_eq!((updated.name.as_str(), updated.cutoff), ("C", Some(0)));
        assert!(matches!(
            update_profile(&db, b.id, values("A")).await,
            Err(ProfileError::NameTaken)
        ));
        assert!(matches!(
            update_profile(&db, 99, values("Z")).await,
            Err(ProfileError::NotFound)
        ));
    }

    #[tokio::test]
    async fn a_profile_that_a_movie_uses_cannot_be_deleted() {
        let (db, _dir) = temp_db().await;
        let (movie, _) = movie_with_file(&db).await;
        let profile = create_profile(&db, values("English")).await.unwrap();
        assert!(
            set_movie_profile(&db, movie, Some(profile.id))
                .await
                .unwrap()
        );
        assert_eq!(movie_count(&db, profile.id).await.unwrap(), 1);
        assert!(matches!(
            delete_profile(&db, profile.id).await,
            Err(ProfileError::InUse)
        ));

        assert!(set_movie_profile(&db, movie, None).await.unwrap());
        delete_profile(&db, profile.id).await.unwrap();
        assert!(matches!(
            delete_profile(&db, profile.id).await,
            Err(ProfileError::NotFound)
        ));
        assert!(!set_movie_profile(&db, 99, None).await.unwrap());
    }

    #[tokio::test]
    async fn deleting_the_default_profile_clears_the_default() {
        let (db, _dir) = temp_db().await;
        let profile = create_profile(&db, values("English")).await.unwrap();
        assert_eq!(default_profile(&db).await.unwrap(), None);
        crate::settings::set_i64(&db, DEFAULT_PROFILE_KEY, profile.id.into())
            .await
            .unwrap();
        assert_eq!(default_profile(&db).await.unwrap(), Some(profile.id));
        delete_profile(&db, profile.id).await.unwrap();
        assert_eq!(default_profile(&db).await.unwrap(), None);
        assert_eq!(
            crate::settings::get_i64(&db, DEFAULT_PROFILE_KEY)
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn sync_adds_new_files_removes_gone_ones_and_keeps_provider_rows() {
        let (db, _dir) = temp_db().await;
        let (movie, file) = movie_with_file(&db).await;
        sync_external(
            &db,
            file,
            &[
                found("en", Kind::Normal, "The Matrix.en.srt"),
                // Same language and kind: the first wins.
                found("en", Kind::Normal, "The Matrix.en.ass"),
                found("fr", Kind::Forced, "The Matrix.fr.forced.srt"),
            ],
        )
        .await
        .unwrap();
        let rows = of_movie(&db, movie).await.unwrap();
        let listed: Vec<_> = rows
            .iter()
            .map(|r| {
                (
                    r.language.as_str(),
                    r.kind.as_str(),
                    r.relative_path.as_str(),
                )
            })
            .collect();
        assert_eq!(
            listed,
            [
                ("en", "normal", "The Matrix.en.srt"),
                ("fr", "forced", "The Matrix.fr.forced.srt"),
            ]
        );
        assert!(rows.iter().all(|r| r.provider.is_none()));

        // A provider subtitle for de, then a second sync: the French file is
        // gone, a Spanish one is new, the provider row stays.
        subtitles::ActiveModel {
            movie_file_id: Set(file),
            language: Set("de".to_owned()),
            kind: Set("normal".to_owned()),
            relative_path: Set("The Matrix.de.srt".to_owned()),
            provider: Set(Some("opensubtitles".to_owned())),
            added_at: Set(crate::now()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        sync_external(
            &db,
            file,
            &[
                found("en", Kind::Normal, "The Matrix.en.srt"),
                found("es", Kind::Normal, "The Matrix.es.srt"),
                // The provider's language and kind: not replaced.
                found("de", Kind::Normal, "The Matrix.ger.srt"),
            ],
        )
        .await
        .unwrap();
        let listed: Vec<_> = of_movie(&db, movie)
            .await
            .unwrap()
            .into_iter()
            .map(|r| (r.language, r.provider))
            .collect();
        assert_eq!(
            listed,
            [
                ("de".to_owned(), Some("opensubtitles".to_owned())),
                ("en".to_owned(), None),
                ("es".to_owned(), None),
            ]
        );
    }

    #[tokio::test]
    async fn subtitles_go_with_their_file() {
        let (db, _dir) = temp_db().await;
        let (movie, file) = movie_with_file(&db).await;
        sync_external(&db, file, &[found("en", Kind::Normal, "a.en.srt")])
            .await
            .unwrap();
        movie_files::Entity::delete_by_id(file)
            .exec(&db)
            .await
            .unwrap();
        assert!(of_movie(&db, movie).await.unwrap().is_empty());
    }
}
