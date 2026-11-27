//! Gives the files adopted with an `Unknown` quality the quality that their
//! names say (docs/09-filesystem.md, "Adopting an existing library").

use std::collections::HashMap;

use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, Set,
    TransactionTrait,
};
use unseenarr_core::backfill::file_quality;
use unseenarr_core::quality::Quality;

use crate::entities::{movie_files, movies};

/// What a run did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Files that got a quality.
    pub updated: usize,
    /// Files whose names say nothing; they stay `Unknown`. Each entry is the
    /// movie folder and the path of the file below it.
    pub unparsed: Vec<String>,
}

/// Parses the file name, then the folder name, of every file with `Unknown`
/// quality and stores the result. Safe to run again: it only touches files
/// that are still `Unknown`.
pub async fn run(db: &DatabaseConnection) -> Result<Report, DbErr> {
    let files = movie_files::Entity::find()
        .filter(movie_files::Column::QualityId.eq(Quality::Unknown.id()))
        .all(db)
        .await?;
    let folders: HashMap<i32, String> = movies::Entity::find()
        .all(db)
        .await?
        .into_iter()
        .map(|movie| (movie.id, movie.folder_name))
        .collect();

    let mut report = Report::default();
    let txn = db.begin().await?;
    for file in files {
        let folder = folders.get(&file.movie_id).map_or("", String::as_str);
        match file_quality(&file.relative_path, folder) {
            Quality::Unknown => report
                .unparsed
                .push(format!("{folder}/{}", file.relative_path)),
            quality => {
                let mut update: movie_files::ActiveModel = file.into();
                update.quality_id = Set(quality.id());
                update.update(&txn).await?;
                report.updated += 1;
            }
        }
    }
    txn.commit().await?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_db;

    async fn movie(db: &DatabaseConnection, tmdb_id: i32, folder: &str) -> i32 {
        let now = crate::now();
        movies::ActiveModel {
            tmdb_id: Set(tmdb_id),
            title: Set(folder.to_owned()),
            original_title: Set(None),
            sort_title: Set(folder.to_owned()),
            year: Set(2000),
            overview: Set(None),
            status: Set("released".to_owned()),
            root_folder_id: Set(root_folder(db).await),
            folder_name: Set(folder.to_owned()),
            monitored: Set(true),
            minimum_availability: Set("released".to_owned()),
            added_at: Set(now.clone()),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn root_folder(db: &DatabaseConnection) -> i32 {
        use crate::entities::root_folders;
        if let Some(root) = root_folders::Entity::find().one(db).await.unwrap() {
            return root.id;
        }
        root_folders::ActiveModel {
            path: Set("/media/movies".to_owned()),
            created_at: Set(crate::now()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn file(db: &DatabaseConnection, movie_id: i32, path: &str, quality: Quality) -> i32 {
        movie_files::ActiveModel {
            movie_id: Set(movie_id),
            relative_path: Set(path.to_owned()),
            size: Set(1),
            quality_id: Set(quality.id()),
            added_at: Set(crate::now()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    async fn quality_of(db: &DatabaseConnection, id: i32) -> i32 {
        movie_files::Entity::find_by_id(id)
            .one(db)
            .await
            .unwrap()
            .unwrap()
            .quality_id
    }

    #[tokio::test]
    async fn unknown_files_get_the_quality_of_their_names() {
        let (db, _dir) = temp_db().await;
        let a = movie(&db, 1, "The Matrix (1999)").await;
        let b = movie(&db, 2, "Dune.2021.2160p.WEB-DL.DDP5.1.H.265-FLUX").await;
        let c = movie(&db, 3, "Stray (2020)").await;
        let in_file = file(
            &db,
            a,
            "The Matrix (1999) Bluray-1080p.mkv",
            Quality::Unknown,
        )
        .await;
        let in_folder = file(&db, b, "movie.mkv", Quality::Unknown).await;
        let nothing = file(&db, c, "Stray (2020).mkv", Quality::Unknown).await;
        // A known quality is never replaced, even if the name says otherwise.
        let known = file(
            &db,
            a,
            "The Matrix (1999) Bluray-1080p.extra.mkv",
            Quality::Dvd,
        )
        .await;

        let report = run(&db).await.unwrap();
        assert_eq!(
            report,
            Report {
                updated: 2,
                unparsed: vec!["Stray (2020)/Stray (2020).mkv".to_owned()],
            }
        );
        assert_eq!(quality_of(&db, in_file).await, Quality::Bluray1080p.id());
        assert_eq!(quality_of(&db, in_folder).await, Quality::WebDl2160p.id());
        assert_eq!(quality_of(&db, nothing).await, Quality::Unknown.id());
        assert_eq!(quality_of(&db, known).await, Quality::Dvd.id());

        // A second run only meets the file that cannot be parsed.
        let again = run(&db).await.unwrap();
        assert_eq!(again.updated, 0);
        assert_eq!(again.unparsed.len(), 1);
    }
}
