//! Basic movie queries (docs/04-database.md, "Library tables").

use std::collections::HashMap;

use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, DbErr, EntityTrait,
    QueryFilter, QueryOrder, QuerySelect, Set, SqlErr, TransactionTrait,
};
use unseen_core::metadata::{MetadataError, MetadataProvider, MovieDetails};
use unseen_core::naming;

use crate::entities::{movie_files, movie_profiles, movie_tags, movies, tags};

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

/// The provider's data about a movie, as the `movies` table keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    pub tmdb_id: i32,
    pub imdb_id: Option<String>,
    pub title: String,
    pub original_title: Option<String>,
    pub sort_title: String,
    pub year: i32,
    pub overview: Option<String>,
    pub runtime: Option<i32>,
    pub status: String,
    pub in_cinemas: Option<String>,
    pub digital_release: Option<String>,
    pub physical_release: Option<String>,
    pub poster_path: Option<String>,
}

impl Metadata {
    /// `None` when the movie has no release date, so no year.
    pub fn from_details(details: &MovieDetails) -> Option<Self> {
        let summary = &details.summary;
        let dates = details.key_dates();
        let some = |text: &str| (!text.is_empty()).then(|| text.to_owned());
        Some(Self {
            tmdb_id: summary.tmdb_id,
            imdb_id: details.imdb_id.clone(),
            title: summary.title.clone(),
            original_title: some(&summary.original_title),
            sort_title: naming::sort_title(&summary.title),
            year: summary.year()?,
            overview: some(&summary.overview),
            runtime: details.runtime_minutes.and_then(|m| i32::try_from(m).ok()),
            status: details.radarr_status().to_owned(),
            in_cinemas: dates.in_cinemas,
            digital_release: dates.digital,
            physical_release: dates.physical,
            poster_path: summary.poster_path.clone(),
        })
    }
}

/// What a caller sets on a new movie.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewMovie {
    pub metadata: Metadata,
    pub root_folder_id: i32,
    pub folder_name: String,
    pub monitored: bool,
    pub minimum_availability: String,
    /// The collection the movie belongs to, from `collections::ensure`.
    pub collection_id: Option<i32>,
}

/// The editable parts of a movie. `None` keeps the stored value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Patch {
    pub monitored: Option<bool>,
    pub minimum_availability: Option<String>,
    /// Replaces the whole tag set.
    pub tags: Option<Vec<i32>>,
    /// A `movie_profiles` id of this movie.
    pub primary_profile_id: Option<i32>,
}

/// One quality profile of a movie and the id of its file, if it has one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileLink {
    pub id: i32,
    pub quality_profile_id: i32,
    pub is_primary: bool,
    pub file_id: Option<i32>,
}

/// A movie with its tag ids (sorted) and profiles (primary first).
#[derive(Debug, Clone, PartialEq)]
pub struct Movie {
    pub model: movies::Model,
    pub tags: Vec<i32>,
    pub profiles: Vec<ProfileLink>,
}

#[derive(Debug)]
pub enum MovieError {
    NotFound,
    /// The movie is in the library already.
    TmdbTaken,
    /// Another movie of the root folder has this folder name, ignoring case.
    FolderTaken,
    /// A root folder, quality profile or tag id does not exist.
    UnknownReference,
    /// The quality profile is attached to the movie already.
    ProfileTaken,
    /// The primary profile stays while another profile exists; make another
    /// profile primary first.
    PrimaryProfile,
    /// A movie keeps at least one profile.
    LastProfile,
    Db(DbErr),
}

impl From<DbErr> for MovieError {
    fn from(error: DbErr) -> Self {
        match error.sql_err() {
            Some(SqlErr::ForeignKeyConstraintViolation(_)) => Self::UnknownReference,
            _ => Self::Db(error),
        }
    }
}

fn unique(ids: &[i32]) -> Vec<i32> {
    let mut seen = Vec::new();
    for id in ids {
        if !seen.contains(id) {
            seen.push(*id);
        }
    }
    seen
}

async fn set_tags(
    db: &impl ConnectionTrait,
    movie_id: i32,
    tag_ids: &[i32],
) -> Result<(), MovieError> {
    movie_tags::Entity::delete_many()
        .filter(movie_tags::Column::MovieId.eq(movie_id))
        .exec(db)
        .await?;
    for tag_id in unique(tag_ids) {
        movie_tags::ActiveModel {
            movie_id: Set(movie_id),
            tag_id: Set(tag_id),
        }
        .insert(db)
        .await?;
    }
    Ok(())
}

async fn insert_profile(
    db: &impl ConnectionTrait,
    movie_id: i32,
    quality_profile_id: i32,
    is_primary: bool,
) -> Result<movie_profiles::Model, MovieError> {
    movie_profiles::ActiveModel {
        movie_id: Set(movie_id),
        quality_profile_id: Set(quality_profile_id),
        is_primary: Set(is_primary),
        created_at: Set(crate::now()),
        ..Default::default()
    }
    .insert(db)
    .await
    .map_err(|error| match error.sql_err() {
        Some(SqlErr::UniqueConstraintViolation(_)) => MovieError::ProfileTaken,
        _ => error.into(),
    })
}

/// Which movies [`load`] reads.
#[derive(Clone, Copy)]
enum Scope {
    Library,
    Movie(i32),
    Tmdb(i32),
    Collection(i32),
}

/// Loads movies with their relations.
async fn load(db: &DatabaseConnection, scope: Scope) -> Result<Vec<Movie>, DbErr> {
    let mut query = movies::Entity::find().order_by_asc(movies::Column::SortTitle);
    match scope {
        Scope::Library => {}
        Scope::Movie(id) => query = query.filter(movies::Column::Id.eq(id)),
        Scope::Tmdb(tmdb_id) => query = query.filter(movies::Column::TmdbId.eq(tmdb_id)),
        Scope::Collection(id) => query = query.filter(movies::Column::CollectionId.eq(id)),
    }
    let models = query.all(db).await?;

    // A few movies are filtered in SQL by their ids; the whole library is
    // read unfiltered instead of with an id list of thousands of entries.
    let mut tags: HashMap<i32, Vec<i32>> = HashMap::new();
    let mut tag_query = movie_tags::Entity::find();
    let mut file_query = movie_files::Entity::find();
    let mut profile_query = movie_profiles::Entity::find();
    if !matches!(scope, Scope::Library) {
        let ids: Vec<i32> = models.iter().map(|model| model.id).collect();
        tag_query = tag_query.filter(movie_tags::Column::MovieId.is_in(ids.clone()));
        file_query = file_query.filter(movie_files::Column::MovieId.is_in(ids.clone()));
        profile_query = profile_query.filter(movie_profiles::Column::MovieId.is_in(ids));
    }
    for link in tag_query
        .order_by_asc(movie_tags::Column::TagId)
        .all(db)
        .await?
    {
        tags.entry(link.movie_id).or_default().push(link.tag_id);
    }

    let files: HashMap<i32, i32> = file_query
        .filter(movie_files::Column::MovieProfileId.is_not_null())
        .all(db)
        .await?
        .into_iter()
        .filter_map(|file| Some((file.movie_profile_id?, file.id)))
        .collect();
    let mut profiles: HashMap<i32, Vec<ProfileLink>> = HashMap::new();
    for profile in profile_query
        .order_by_desc(movie_profiles::Column::IsPrimary)
        .order_by_asc(movie_profiles::Column::Id)
        .all(db)
        .await?
    {
        profiles
            .entry(profile.movie_id)
            .or_default()
            .push(ProfileLink {
                id: profile.id,
                quality_profile_id: profile.quality_profile_id,
                is_primary: profile.is_primary,
                file_id: files.get(&profile.id).copied(),
            });
    }

    Ok(models
        .into_iter()
        .map(|model| Movie {
            tags: tags.remove(&model.id).unwrap_or_default(),
            profiles: profiles.remove(&model.id).unwrap_or_default(),
            model,
        })
        .collect())
}

/// Every movie, by sort title.
pub async fn list(db: &DatabaseConnection) -> Result<Vec<Movie>, DbErr> {
    load(db, Scope::Library).await
}

pub async fn find(db: &DatabaseConnection, id: i32) -> Result<Option<Movie>, DbErr> {
    Ok(load(db, Scope::Movie(id)).await?.pop())
}

/// Loads one movie with its tags and profiles by TMDB id.
pub async fn find_movie_by_tmdb_id(
    db: &DatabaseConnection,
    tmdb_id: i32,
) -> Result<Option<Movie>, DbErr> {
    Ok(load(db, Scope::Tmdb(tmdb_id)).await?.pop())
}

/// Loads movie files by their primary keys.
pub async fn files_by_ids(
    db: &DatabaseConnection,
    ids: &[i32],
) -> Result<HashMap<i32, movie_files::Model>, DbErr> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = movie_files::Entity::find()
        .filter(movie_files::Column::Id.is_in(ids.iter().copied()))
        .all(db)
        .await?;
    Ok(rows.into_iter().map(|file| (file.id, file)).collect())
}

/// The movies of one collection that are in the library, by sort title.
pub async fn in_collection(
    db: &DatabaseConnection,
    collection_id: i32,
) -> Result<Vec<Movie>, DbErr> {
    load(db, Scope::Collection(collection_id)).await
}

/// A file of an adopted movie, as found on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewFile {
    /// Relative to the movie folder, with `/` between the parts.
    pub relative_path: String,
    pub size: i64,
    /// A quality id of `core::quality`; `0` is `Unknown`.
    pub quality_id: i32,
    /// Lowercase ISO 639-1 language codes (or `"multi"`) parsed from the file or
    /// folder name.
    pub languages: Vec<String>,
}

/// Adds a movie with its profiles (the first is primary) and tags in one
/// step: either everything is stored or nothing.
pub async fn create(
    db: &DatabaseConnection,
    new: NewMovie,
    quality_profile_ids: &[i32],
    tag_ids: &[i32],
) -> Result<Movie, MovieError> {
    create_with_files(db, new, quality_profile_ids, tag_ids, &[]).await
}

/// [`create`] for an existing folder. The first of `files` becomes the
/// managed file of the primary profile; the rest are unmanaged files.
pub async fn create_with_files(
    db: &DatabaseConnection,
    new: NewMovie,
    quality_profile_ids: &[i32],
    tag_ids: &[i32],
    files: &[NewFile],
) -> Result<Movie, MovieError> {
    let txn = db.begin().await?;
    // Checked first, so a movie added twice is reported as such and not as
    // a clash of its own folder name.
    if movies::Entity::find()
        .filter(movies::Column::TmdbId.eq(new.metadata.tmdb_id))
        .one(&txn)
        .await?
        .is_some()
    {
        return Err(MovieError::TmdbTaken);
    }
    let wanted = new.folder_name.to_lowercase();
    let clash = movies::Entity::find()
        .filter(movies::Column::RootFolderId.eq(new.root_folder_id))
        .all(&txn)
        .await?
        .iter()
        .any(|other| other.folder_name.to_lowercase() == wanted);
    if clash {
        return Err(MovieError::FolderTaken);
    }

    let meta = new.metadata;
    let now = crate::now();
    let movie = movies::ActiveModel {
        tmdb_id: Set(meta.tmdb_id),
        imdb_id: Set(meta.imdb_id),
        title: Set(meta.title),
        original_title: Set(meta.original_title),
        sort_title: Set(meta.sort_title),
        year: Set(meta.year),
        overview: Set(meta.overview),
        runtime: Set(meta.runtime),
        status: Set(meta.status),
        in_cinemas: Set(meta.in_cinemas),
        digital_release: Set(meta.digital_release),
        physical_release: Set(meta.physical_release),
        poster_path: Set(meta.poster_path),
        collection_id: Set(new.collection_id),
        root_folder_id: Set(new.root_folder_id),
        folder_name: Set(new.folder_name),
        monitored: Set(new.monitored),
        minimum_availability: Set(new.minimum_availability),
        added_at: Set(now.clone()),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&txn)
    .await
    .map_err(|error| match error.sql_err() {
        Some(SqlErr::UniqueConstraintViolation(_)) => MovieError::TmdbTaken,
        _ => error.into(),
    })?;
    let mut primary_profile = None;
    for (index, profile_id) in unique(quality_profile_ids).into_iter().enumerate() {
        let profile = insert_profile(&txn, movie.id, profile_id, index == 0).await?;
        if index == 0 {
            primary_profile = Some(profile.id);
        }
    }
    for (index, file) in files.iter().enumerate() {
        let languages = (!file.languages.is_empty()).then(|| serde_json::json!(file.languages));
        movie_files::ActiveModel {
            movie_id: Set(movie.id),
            relative_path: Set(file.relative_path.clone()),
            size: Set(file.size),
            quality_id: Set(file.quality_id),
            languages: Set(languages),
            movie_profile_id: Set(primary_profile.filter(|_| index == 0)),
            added_at: Set(crate::now()),
            ..Default::default()
        }
        .insert(&txn)
        .await?;
    }
    set_tags(&txn, movie.id, tag_ids).await?;
    txn.commit().await?;
    find(db, movie.id).await?.ok_or(MovieError::NotFound)
}

/// The TMDB ids of every movie in the library.
pub async fn tmdb_ids(db: &DatabaseConnection) -> Result<Vec<i32>, DbErr> {
    movies::Entity::find()
        .select_only()
        .column(movies::Column::TmdbId)
        .into_tuple()
        .all(db)
        .await
}

/// Changes the editable parts of a movie.
pub async fn update(db: &DatabaseConnection, id: i32, patch: Patch) -> Result<Movie, MovieError> {
    let txn = db.begin().await?;
    let current = movies::Entity::find_by_id(id)
        .one(&txn)
        .await?
        .ok_or(MovieError::NotFound)?;
    let mut movie: movies::ActiveModel = current.into();
    if let Some(monitored) = patch.monitored {
        movie.monitored = Set(monitored);
    }
    if let Some(minimum_availability) = patch.minimum_availability {
        movie.minimum_availability = Set(minimum_availability);
    }
    movie.updated_at = Set(crate::now());
    movie.update(&txn).await?;
    if let Some(tags) = &patch.tags {
        set_tags(&txn, id, tags).await?;
    }
    if let Some(profile_id) = patch.primary_profile_id {
        let profiles = movie_profiles::Entity::find()
            .filter(movie_profiles::Column::MovieId.eq(id))
            .all(&txn)
            .await?;
        if !profiles.iter().any(|p| p.id == profile_id) {
            return Err(MovieError::UnknownReference);
        }
        // At most one primary row can exist, so clear the old one first.
        movie_profiles::Entity::update_many()
            .col_expr(movie_profiles::Column::IsPrimary, false.into())
            .filter(movie_profiles::Column::MovieId.eq(id))
            .exec(&txn)
            .await?;
        movie_profiles::Entity::update_many()
            .col_expr(movie_profiles::Column::IsPrimary, true.into())
            .filter(movie_profiles::Column::Id.eq(profile_id))
            .exec(&txn)
            .await?;
    }
    txn.commit().await?;
    find(db, id).await?.ok_or(MovieError::NotFound)
}

/// Deletes the movie row; tag links, profiles and files go with it.
pub async fn delete(db: &DatabaseConnection, id: i32) -> Result<(), MovieError> {
    if movies::Entity::delete_by_id(id)
        .exec(db)
        .await?
        .rows_affected
        == 0
    {
        return Err(MovieError::NotFound);
    }
    Ok(())
}

/// Attaches a quality profile to a movie as a non-primary profile.
pub async fn add_profile(
    db: &DatabaseConnection,
    movie_id: i32,
    quality_profile_id: i32,
) -> Result<Movie, MovieError> {
    if movies::Entity::find_by_id(movie_id)
        .one(db)
        .await?
        .is_none()
    {
        return Err(MovieError::NotFound);
    }
    insert_profile(db, movie_id, quality_profile_id, false).await?;
    find(db, movie_id).await?.ok_or(MovieError::NotFound)
}

/// Detaches a profile. Its file, if any, stays as an unmanaged file. The
/// primary profile cannot go while another exists, and the last one never.
pub async fn remove_profile(
    db: &DatabaseConnection,
    movie_id: i32,
    movie_profile_id: i32,
) -> Result<Movie, MovieError> {
    let profiles = movie_profiles::Entity::find()
        .filter(movie_profiles::Column::MovieId.eq(movie_id))
        .all(db)
        .await?;
    let target = profiles
        .iter()
        .find(|profile| profile.id == movie_profile_id)
        .ok_or(MovieError::NotFound)?;
    if profiles.len() == 1 {
        return Err(MovieError::LastProfile);
    }
    if target.is_primary {
        return Err(MovieError::PrimaryProfile);
    }
    movie_profiles::Entity::delete_by_id(movie_profile_id)
        .exec(db)
        .await?;
    find(db, movie_id).await?.ok_or(MovieError::NotFound)
}

pub async fn find_file(
    db: &DatabaseConnection,
    movie_id: i32,
    file_id: i32,
) -> Result<Option<movie_files::Model>, DbErr> {
    movie_files::Entity::find_by_id(file_id)
        .filter(movie_files::Column::MovieId.eq(movie_id))
        .one(db)
        .await
}

pub async fn delete_file(db: &DatabaseConnection, file_id: i32) -> Result<(), DbErr> {
    movie_files::Entity::delete_by_id(file_id).exec(db).await?;
    Ok(())
}

#[derive(Debug)]
pub enum RefreshError {
    NotFound,
    /// The provider has no release date for the movie, so there is no year.
    NoYear,
    Provider(MetadataError),
    Db(DbErr),
}

impl From<DbErr> for RefreshError {
    fn from(error: DbErr) -> Self {
        Self::Db(error)
    }
}

/// Fetches the movie again and stores the new metadata. The folder name
/// stays as it is: renaming existing folders is a separate action.
pub async fn refresh(
    db: &DatabaseConnection,
    provider: &dyn MetadataProvider,
    id: i32,
) -> Result<movies::Model, RefreshError> {
    let current = movies::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or(RefreshError::NotFound)?;
    let details = provider
        .details(current.tmdb_id)
        .await
        .map_err(RefreshError::Provider)?;
    let meta = Metadata::from_details(&details).ok_or(RefreshError::NoYear)?;
    // A movie that left its collection at the provider leaves it here.
    let collection_id = match &details.collection {
        Some(reference) => Some(crate::collections::sync(db, provider, reference).await?),
        None => None,
    };
    let mut movie: movies::ActiveModel = current.into();
    movie.imdb_id = Set(meta.imdb_id);
    movie.title = Set(meta.title);
    movie.original_title = Set(meta.original_title);
    movie.sort_title = Set(meta.sort_title);
    movie.year = Set(meta.year);
    movie.overview = Set(meta.overview);
    movie.runtime = Set(meta.runtime);
    movie.status = Set(meta.status);
    movie.in_cinemas = Set(meta.in_cinemas);
    movie.digital_release = Set(meta.digital_release);
    movie.physical_release = Set(meta.physical_release);
    movie.poster_path = Set(meta.poster_path);
    movie.collection_id = Set(collection_id);
    movie.updated_at = Set(crate::now());
    Ok(movie.update(db).await?)
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

#[cfg(test)]
mod lifecycle_tests {
    use async_trait::async_trait;
    use unseen_core::metadata::{Collection, MovieSummary, ReleaseDate, ReleaseKind};

    use super::*;
    use crate::entities::{quality_profiles, root_folders};
    use crate::testing::temp_db;

    fn metadata(tmdb_id: i32, title: &str) -> Metadata {
        Metadata {
            tmdb_id,
            imdb_id: Some(format!("tt{tmdb_id}")),
            title: title.into(),
            original_title: None,
            sort_title: naming::sort_title(title),
            year: 1999,
            overview: None,
            runtime: Some(136),
            status: "released".into(),
            in_cinemas: None,
            digital_release: None,
            physical_release: None,
            poster_path: None,
        }
    }

    fn new_movie(tmdb_id: i32, title: &str, root: i32) -> NewMovie {
        NewMovie {
            metadata: metadata(tmdb_id, title),
            root_folder_id: root,
            folder_name: naming::movie_folder_name(title, 1999),
            monitored: true,
            minimum_availability: "released".into(),
            collection_id: None,
        }
    }

    async fn root(db: &DatabaseConnection) -> i32 {
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
            created_at: Set(crate::now()),
            updated_at: Set(crate::now()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn tag(db: &DatabaseConnection, label: &str) -> i32 {
        crate::tags::create(db, label.into()).await.unwrap().id
    }

    #[tokio::test]
    async fn create_stores_profiles_with_the_first_as_primary_and_tags() {
        let (db, _dir) = temp_db().await;
        let root = root(&db).await;
        let (hd, uhd) = (profile(&db, "HD").await, profile(&db, "UHD").await);
        let t = tag(&db, "kids").await;

        let movie = create(
            &db,
            new_movie(603, "The Matrix", root),
            &[uhd, hd, uhd],
            &[t, t],
        )
        .await
        .unwrap();
        assert_eq!(movie.model.sort_title, "matrix");
        assert_eq!(movie.model.folder_name, "The Matrix (1999)");
        assert_eq!(movie.tags, [t]);
        let found: Vec<_> = movie
            .profiles
            .iter()
            .map(|p| (p.quality_profile_id, p.is_primary))
            .collect();
        assert_eq!(found, [(uhd, true), (hd, false)]);
        assert_eq!(list(&db).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn create_refuses_a_duplicate_movie_a_folder_clash_and_unknown_ids() {
        let (db, _dir) = temp_db().await;
        let root = root(&db).await;
        let hd = profile(&db, "HD").await;
        create(&db, new_movie(603, "The Matrix", root), &[hd], &[])
            .await
            .unwrap();

        assert!(matches!(
            create(&db, new_movie(603, "The Matrix", root), &[hd], &[]).await,
            Err(MovieError::TmdbTaken)
        ));
        // The same folder under another TMDB id, in another case.
        let mut clash = new_movie(604, "the matrix", root);
        clash.folder_name = "THE MATRIX (1999)".into();
        assert!(matches!(
            create(&db, clash, &[hd], &[]).await,
            Err(MovieError::FolderTaken)
        ));
        for (root_id, profiles, tags) in [
            (999, vec![hd], vec![]),
            (root, vec![999], vec![]),
            (root, vec![hd], vec![999]),
        ] {
            assert!(matches!(
                create(&db, new_movie(700, "Other", root_id), &profiles, &tags).await,
                Err(MovieError::UnknownReference)
            ));
        }
        assert!(
            find_by_tmdb_id(&db, 700).await.unwrap().is_none(),
            "nothing stored"
        );
    }

    #[tokio::test]
    async fn update_changes_fields_tags_and_the_primary_profile() {
        let (db, _dir) = temp_db().await;
        let root = root(&db).await;
        let (hd, uhd) = (profile(&db, "HD").await, profile(&db, "UHD").await);
        let (a, b) = (tag(&db, "a").await, tag(&db, "b").await);
        let movie = create(&db, new_movie(1, "M", root), &[hd, uhd], &[a])
            .await
            .unwrap();
        let second = movie.profiles[1].id;

        let patched = update(
            &db,
            movie.model.id,
            Patch {
                monitored: Some(false),
                minimum_availability: Some("announced".into()),
                tags: Some(vec![b]),
                primary_profile_id: Some(second),
            },
        )
        .await
        .unwrap();
        assert!(!patched.model.monitored);
        assert_eq!(patched.model.minimum_availability, "announced");
        assert_eq!(patched.tags, [b]);
        assert_eq!(patched.profiles[0].id, second);
        assert!(patched.profiles[0].is_primary && !patched.profiles[1].is_primary);

        // An id of another movie's profile is refused and changes nothing.
        assert!(matches!(
            update(
                &db,
                movie.model.id,
                Patch {
                    monitored: Some(true),
                    primary_profile_id: Some(999),
                    ..Patch::default()
                }
            )
            .await,
            Err(MovieError::UnknownReference)
        ));
        assert!(
            !find(&db, movie.model.id)
                .await
                .unwrap()
                .unwrap()
                .model
                .monitored
        );
        assert!(matches!(
            update(&db, 999, Patch::default()).await,
            Err(MovieError::NotFound)
        ));
    }

    #[tokio::test]
    async fn profiles_are_added_and_removed_by_the_primary_rule() {
        let (db, _dir) = temp_db().await;
        let root = root(&db).await;
        let (hd, uhd) = (profile(&db, "HD").await, profile(&db, "UHD").await);
        let movie = create(&db, new_movie(1, "M", root), &[hd], &[])
            .await
            .unwrap();
        let id = movie.model.id;
        let primary = movie.profiles[0].id;

        assert!(matches!(
            remove_profile(&db, id, primary).await,
            Err(MovieError::LastProfile)
        ));
        let with_two = add_profile(&db, id, uhd).await.unwrap();
        assert!(matches!(
            add_profile(&db, id, uhd).await,
            Err(MovieError::ProfileTaken)
        ));
        assert!(matches!(
            add_profile(&db, 999, uhd).await,
            Err(MovieError::NotFound)
        ));
        let extra = with_two.profiles[1].id;
        assert!(!with_two.profiles[1].is_primary);

        assert!(matches!(
            remove_profile(&db, id, primary).await,
            Err(MovieError::PrimaryProfile)
        ));
        assert!(matches!(
            remove_profile(&db, id, 999).await,
            Err(MovieError::NotFound)
        ));

        // The file of a removed profile stays as an unmanaged file.
        movie_files::ActiveModel {
            movie_id: Set(id),
            relative_path: Set("a.mkv".into()),
            size: Set(1),
            quality_id: Set(7),
            movie_profile_id: Set(Some(extra)),
            added_at: Set(crate::now()),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        assert!(
            find(&db, id).await.unwrap().unwrap().profiles[1]
                .file_id
                .is_some()
        );
        let after = remove_profile(&db, id, extra).await.unwrap();
        assert_eq!(after.profiles.len(), 1);
        let files = files_of(&db, id).await.unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].movie_profile_id, None);
        let file = find_file(&db, id, files[0].id).await.unwrap().unwrap();
        assert!(find_file(&db, 999, file.id).await.unwrap().is_none());
        delete_file(&db, file.id).await.unwrap();
        assert!(files_of(&db, id).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn delete_removes_the_movie_and_reports_a_missing_one() {
        let (db, _dir) = temp_db().await;
        let root = root(&db).await;
        let hd = profile(&db, "HD").await;
        let movie = create(&db, new_movie(1, "M", root), &[hd], &[])
            .await
            .unwrap();
        delete(&db, movie.model.id).await.unwrap();
        assert!(find(&db, movie.model.id).await.unwrap().is_none());
        assert!(matches!(
            delete(&db, movie.model.id).await,
            Err(MovieError::NotFound)
        ));
    }

    struct Fixed(Option<MovieDetails>);

    #[async_trait]
    impl MetadataProvider for Fixed {
        async fn search(&self, _: &str) -> Result<Vec<MovieSummary>, MetadataError> {
            unreachable!()
        }
        async fn find_by_imdb_id(&self, _: &str) -> Result<Vec<MovieSummary>, MetadataError> {
            unreachable!()
        }
        async fn details(&self, _: i32) -> Result<MovieDetails, MetadataError> {
            self.0.clone().ok_or(MetadataError::NotFound)
        }
        async fn collection(&self, _: i32) -> Result<Collection, MetadataError> {
            unreachable!()
        }
    }

    fn details(title: &str, date: Option<&str>) -> MovieDetails {
        MovieDetails {
            summary: MovieSummary {
                tmdb_id: 1,
                title: title.into(),
                original_title: String::new(),
                overview: "New overview".into(),
                release_date: date.map(Into::into),
                poster_path: Some("/p.jpg".into()),
            },
            imdb_id: Some("tt1".into()),
            runtime_minutes: Some(120),
            status: "Released".into(),
            genres: vec![],
            collection: None,
            release_dates: vec![ReleaseDate {
                country: "US".into(),
                kind: ReleaseKind::Digital,
                date: "2000-01-01T00:00:00.000Z".into(),
                certification: None,
            }],
        }
    }

    #[tokio::test]
    async fn refresh_updates_the_metadata_and_keeps_the_folder_name() {
        let (db, _dir) = temp_db().await;
        let root = root(&db).await;
        let hd = profile(&db, "HD").await;
        let movie = create(&db, new_movie(1, "Old Title", root), &[hd], &[])
            .await
            .unwrap();
        let id = movie.model.id;

        let provider = Fixed(Some(details("New Title", Some("2001-05-05"))));
        let refreshed = refresh(&db, &provider, id).await.unwrap();
        assert_eq!(refreshed.title, "New Title");
        assert_eq!(refreshed.year, 2001);
        assert_eq!(refreshed.overview.as_deref(), Some("New overview"));
        assert_eq!(
            refreshed.digital_release.as_deref(),
            Some("2000-01-01T00:00:00.000Z")
        );
        assert_eq!(refreshed.folder_name, "Old Title (1999)");

        assert!(matches!(
            refresh(&db, &Fixed(Some(details("X", None))), id).await,
            Err(RefreshError::NoYear)
        ));
        assert!(matches!(
            refresh(&db, &Fixed(None), id).await,
            Err(RefreshError::Provider(MetadataError::NotFound))
        ));
        assert!(matches!(
            refresh(&db, &provider, 999).await,
            Err(RefreshError::NotFound)
        ));
    }
}
