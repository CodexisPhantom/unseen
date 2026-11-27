//! Collections (docs/04-database.md, "Library tables"): the provider's
//! series of movies, with the defaults for the movies a monitored collection
//! adds. A collection row stays when its last movie is deleted.

use std::collections::HashMap;

use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Set, SqlErr,
};
use unseenarr_core::metadata::{CollectionRef, MetadataProvider};
use unseenarr_core::naming;

use crate::entities::{collections, movies};

/// The editable parts of a collection. `None` keeps the stored value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Patch {
    pub monitored: Option<bool>,
    pub root_folder_id: Option<i32>,
    pub quality_profile_id: Option<i32>,
    pub minimum_availability: Option<String>,
}

#[derive(Debug)]
pub enum CollectionError {
    NotFound,
    /// A root folder or quality profile id does not exist.
    UnknownReference,
    Db(DbErr),
}

impl From<DbErr> for CollectionError {
    fn from(error: DbErr) -> Self {
        match error.sql_err() {
            Some(SqlErr::ForeignKeyConstraintViolation(_)) => Self::UnknownReference,
            _ => Self::Db(error),
        }
    }
}

/// Every collection, by sort title.
pub async fn list(db: &DatabaseConnection) -> Result<Vec<collections::Model>, DbErr> {
    collections::Entity::find()
        .order_by_asc(collections::Column::SortTitle)
        .all(db)
        .await
}

pub async fn find(db: &DatabaseConnection, id: i32) -> Result<Option<collections::Model>, DbErr> {
    collections::Entity::find_by_id(id).one(db).await
}

pub async fn find_by_tmdb_id(
    db: &DatabaseConnection,
    tmdb_id: i32,
) -> Result<Option<collections::Model>, DbErr> {
    collections::Entity::find()
        .filter(collections::Column::TmdbId.eq(tmdb_id))
        .one(db)
        .await
}

/// How many movies of the library each collection holds. A collection
/// without movies is absent.
pub async fn movie_counts(db: &DatabaseConnection) -> Result<HashMap<i32, i64>, DbErr> {
    let ids = movies::Entity::find()
        .select_only()
        .column(movies::Column::CollectionId)
        .filter(movies::Column::CollectionId.is_not_null())
        .into_tuple::<i32>()
        .all(db)
        .await?;
    let mut counts = HashMap::new();
    for id in ids {
        *counts.entry(id).or_default() += 1;
    }
    Ok(counts)
}

/// The id of the collection a movie belongs to. The row is created on first
/// sight with the defaults of a collection that is not monitored.
pub async fn ensure(
    db: &DatabaseConnection,
    provider: &dyn MetadataProvider,
    reference: &CollectionRef,
) -> Result<i32, DbErr> {
    match find_by_tmdb_id(db, reference.tmdb_id).await? {
        Some(found) => Ok(found.id),
        None => sync(db, provider, reference).await,
    }
}

/// Fetches the collection and stores its title, overview and poster, as a
/// new row or over the stored one. When the provider fails, a new row keeps
/// only the name of the reference and a stored row stays as it is: the
/// movie that led here must not fail for it.
pub async fn sync(
    db: &DatabaseConnection,
    provider: &dyn MetadataProvider,
    reference: &CollectionRef,
) -> Result<i32, DbErr> {
    let stored = find_by_tmdb_id(db, reference.tmdb_id).await?;
    let (title, overview, poster_path) = match provider.collection(reference.tmdb_id).await {
        Ok(fetched) => (
            Some(fetched.name).filter(|name| !name.is_empty()),
            Some(fetched.overview).filter(|text| !text.is_empty()),
            fetched.poster_path,
        ),
        Err(_) => match stored {
            Some(stored) => return Ok(stored.id),
            None => (None, None, None),
        },
    };
    let title = title.unwrap_or_else(|| reference.name.clone());
    let now = crate::now();
    if let Some(stored) = stored {
        let mut row: collections::ActiveModel = stored.into();
        row.sort_title = Set(naming::sort_title(&title));
        row.title = Set(title);
        row.overview = Set(overview);
        row.poster_path = Set(poster_path);
        row.updated_at = Set(now);
        return Ok(row.update(db).await?.id);
    }
    let inserted = collections::ActiveModel {
        tmdb_id: Set(reference.tmdb_id),
        sort_title: Set(naming::sort_title(&title)),
        title: Set(title),
        overview: Set(overview),
        poster_path: Set(poster_path),
        monitored: Set(false),
        minimum_availability: Set("released".to_owned()),
        added_at: Set(now.clone()),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await;
    match inserted {
        Ok(row) => Ok(row.id),
        // A second movie of the collection was added at the same moment.
        Err(error) if matches!(error.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) => {
            find_by_tmdb_id(db, reference.tmdb_id)
                .await?
                .map(|row| row.id)
                .ok_or(error)
        }
        Err(error) => Err(error),
    }
}

/// Changes the editable parts of a collection.
pub async fn update(
    db: &DatabaseConnection,
    id: i32,
    patch: Patch,
) -> Result<collections::Model, CollectionError> {
    let current = find(db, id).await?.ok_or(CollectionError::NotFound)?;
    let mut row: collections::ActiveModel = current.into();
    if let Some(monitored) = patch.monitored {
        row.monitored = Set(monitored);
    }
    if let Some(root_folder_id) = patch.root_folder_id {
        row.root_folder_id = Set(Some(root_folder_id));
    }
    if let Some(quality_profile_id) = patch.quality_profile_id {
        row.quality_profile_id = Set(Some(quality_profile_id));
    }
    if let Some(minimum_availability) = patch.minimum_availability {
        row.minimum_availability = Set(minimum_availability);
    }
    row.updated_at = Set(crate::now());
    Ok(row.update(db).await?)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use unseenarr_core::metadata::{Collection, MetadataError, MovieDetails, MovieSummary};

    use super::*;
    use crate::entities::root_folders;
    use crate::testing::temp_db;

    /// A provider that knows one collection and counts the calls for it.
    struct Provider {
        collection: Option<Collection>,
        details: Option<MovieDetails>,
        calls: AtomicUsize,
    }

    impl Provider {
        fn new(collection: Option<Collection>) -> Self {
            Self {
                collection,
                details: None,
                calls: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl MetadataProvider for Provider {
        async fn search(&self, _: &str) -> Result<Vec<MovieSummary>, MetadataError> {
            unreachable!()
        }
        async fn find_by_imdb_id(&self, _: &str) -> Result<Vec<MovieSummary>, MetadataError> {
            unreachable!()
        }
        async fn details(&self, _: i32) -> Result<MovieDetails, MetadataError> {
            self.details.clone().ok_or(MetadataError::NotFound)
        }
        async fn collection(&self, _: i32) -> Result<Collection, MetadataError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.collection
                .clone()
                .ok_or(MetadataError::Unavailable("down".into()))
        }
    }

    fn matrix() -> Collection {
        Collection {
            tmdb_id: 2344,
            name: "The Matrix Collection".into(),
            overview: "Machines.".into(),
            poster_path: Some("/m.jpg".into()),
            movies: vec![],
        }
    }

    fn reference() -> CollectionRef {
        CollectionRef {
            tmdb_id: 2344,
            name: "The Matrix Collection".into(),
        }
    }

    async fn movie(db: &DatabaseConnection, tmdb_id: i32, collection_id: Option<i32>) -> i32 {
        let root = match root_folders::Entity::find().one(db).await.unwrap() {
            Some(root) => root.id,
            None => {
                root_folders::ActiveModel {
                    path: Set("/movies".into()),
                    created_at: Set(crate::now()),
                    ..Default::default()
                }
                .insert(db)
                .await
                .unwrap()
                .id
            }
        };
        movies::ActiveModel {
            tmdb_id: Set(tmdb_id),
            title: Set(format!("Movie {tmdb_id}")),
            sort_title: Set(format!("movie {tmdb_id}")),
            year: Set(1999),
            status: Set("released".into()),
            collection_id: Set(collection_id),
            root_folder_id: Set(root),
            folder_name: Set(format!("Movie {tmdb_id} (1999)")),
            monitored: Set(true),
            minimum_availability: Set("released".into()),
            added_at: Set(crate::now()),
            updated_at: Set(crate::now()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn a_collection_is_created_once_with_the_provider_data() {
        let (db, _dir) = temp_db().await;
        let provider = Provider::new(Some(matrix()));

        let first = ensure(&db, &provider, &reference()).await.unwrap();
        let second = ensure(&db, &provider, &reference()).await.unwrap();

        assert_eq!(first, second);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        let rows = list(&db).await.unwrap();
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.tmdb_id, 2344);
        assert_eq!(row.title, "The Matrix Collection");
        assert_eq!(row.sort_title, "matrix collection");
        assert_eq!(row.overview.as_deref(), Some("Machines."));
        assert_eq!(row.poster_path.as_deref(), Some("/m.jpg"));
        assert!(!row.monitored);
        assert_eq!(row.root_folder_id, None);
        assert_eq!(row.quality_profile_id, None);
        assert_eq!(row.minimum_availability, "released");
    }

    #[tokio::test]
    async fn a_failing_provider_still_gives_a_row_with_the_name() {
        let (db, _dir) = temp_db().await;
        let down = Provider::new(None);

        let id = ensure(&db, &down, &reference()).await.unwrap();
        let row = find(&db, id).await.unwrap().unwrap();
        assert_eq!(row.title, "The Matrix Collection");
        assert_eq!(row.overview, None);

        // A later sync fills the row; a failing one leaves it alone.
        sync(&db, &Provider::new(Some(matrix())), &reference())
            .await
            .unwrap();
        assert_eq!(sync(&db, &down, &reference()).await.unwrap(), id);
        let row = find(&db, id).await.unwrap().unwrap();
        assert_eq!(row.overview.as_deref(), Some("Machines."));
        assert_eq!(list(&db).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn deleting_the_last_movie_keeps_the_collection() {
        let (db, _dir) = temp_db().await;
        let id = ensure(&db, &Provider::new(Some(matrix())), &reference())
            .await
            .unwrap();
        let movie_id = movie(&db, 603, Some(id)).await;
        assert_eq!(movie_counts(&db).await.unwrap().get(&id), Some(&1));
        assert_eq!(
            crate::movies::in_collection(&db, id).await.unwrap().len(),
            1
        );

        crate::movies::delete(&db, movie_id).await.unwrap();

        assert!(find(&db, id).await.unwrap().is_some());
        assert!(movie_counts(&db).await.unwrap().is_empty());
        assert!(
            crate::movies::in_collection(&db, id)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn update_changes_only_the_given_fields() {
        let (db, _dir) = temp_db().await;
        let id = ensure(&db, &Provider::new(Some(matrix())), &reference())
            .await
            .unwrap();
        movie(&db, 603, None).await; // creates a root folder
        let root = root_folders::Entity::find()
            .one(&db)
            .await
            .unwrap()
            .unwrap()
            .id;

        let row = update(
            &db,
            id,
            Patch {
                monitored: Some(true),
                root_folder_id: Some(root),
                minimum_availability: Some("announced".into()),
                ..Patch::default()
            },
        )
        .await
        .unwrap();
        assert!(row.monitored);
        assert_eq!(row.root_folder_id, Some(root));
        assert_eq!(row.quality_profile_id, None);
        assert_eq!(row.minimum_availability, "announced");

        let unknown = Patch {
            quality_profile_id: Some(999),
            ..Patch::default()
        };
        assert!(matches!(
            update(&db, id, unknown).await,
            Err(CollectionError::UnknownReference)
        ));
        assert!(matches!(
            update(&db, 999, Patch::default()).await,
            Err(CollectionError::NotFound)
        ));
    }

    #[tokio::test]
    async fn deleting_the_root_folder_clears_the_collection_default() {
        let (db, _dir) = temp_db().await;
        let id = ensure(&db, &Provider::new(Some(matrix())), &reference())
            .await
            .unwrap();
        let root = root_folders::ActiveModel {
            path: Set("/other".into()),
            created_at: Set(crate::now()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap()
        .id;
        let patch = Patch {
            root_folder_id: Some(root),
            ..Patch::default()
        };
        update(&db, id, patch).await.unwrap();

        root_folders::Entity::delete_by_id(root)
            .exec(&db)
            .await
            .unwrap();

        assert_eq!(find(&db, id).await.unwrap().unwrap().root_folder_id, None);
    }

    #[tokio::test]
    async fn refresh_links_and_unlinks_the_collection() {
        let (db, _dir) = temp_db().await;
        let movie_id = movie(&db, 603, None).await;
        let mut provider = Provider::new(Some(matrix()));
        let mut details = MovieDetails {
            summary: MovieSummary {
                tmdb_id: 603,
                title: "The Matrix".into(),
                original_title: String::new(),
                overview: String::new(),
                release_date: Some("1999-03-30".into()),
                poster_path: None,
            },
            imdb_id: None,
            runtime_minutes: None,
            status: "Released".into(),
            genres: vec![],
            collection: Some(reference()),
            release_dates: vec![],
        };
        provider.details = Some(details.clone());

        let linked = crate::movies::refresh(&db, &provider, movie_id)
            .await
            .unwrap();
        let collection = find_by_tmdb_id(&db, 2344).await.unwrap().unwrap();
        assert_eq!(linked.collection_id, Some(collection.id));

        details.collection = None;
        provider.details = Some(details);
        let unlinked = crate::movies::refresh(&db, &provider, movie_id)
            .await
            .unwrap();
        assert_eq!(unlinked.collection_id, None);
        assert!(find(&db, collection.id).await.unwrap().is_some());
    }
}
