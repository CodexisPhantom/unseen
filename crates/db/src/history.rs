//! History: what happened to a movie, kept for the activity view
//! (docs/04-database.md).

use sea_orm::{ActiveModelTrait, DatabaseConnection, DbErr, Set};

use crate::entities::history;

/// Records that a release was sent to a download client. `data` is the JSON
/// payload the activity view shows.
pub async fn record_grabbed(
    db: &DatabaseConnection,
    movie_id: i32,
    data: String,
) -> Result<history::Model, DbErr> {
    history::ActiveModel {
        movie_id: Set(Some(movie_id)),
        event: Set("grabbed".into()),
        data: Set(data),
        created_at: Set(crate::now()),
        ..Default::default()
    }
    .insert(db)
    .await
}
