//! Size limits per quality (docs/04-database.md, "Library tables"). The rows
//! are seeded by a migration; there is one per Radarr quality and none is
//! created or deleted afterwards.

use sea_orm::{ActiveModelTrait, DatabaseConnection, DbErr, EntityTrait, QueryOrder, Set};

use crate::entities::quality_definitions;

/// What a caller sets on a definition.
#[derive(Debug, Clone, PartialEq)]
pub struct Values {
    pub title: String,
    pub min_size: f64,
    pub preferred_size: Option<f64>,
    pub max_size: Option<f64>,
}

/// Every definition, by quality id. The caller orders them.
pub async fn list(db: &DatabaseConnection) -> Result<Vec<quality_definitions::Model>, DbErr> {
    quality_definitions::Entity::find()
        .order_by_asc(quality_definitions::Column::QualityId)
        .all(db)
        .await
}

pub async fn find(
    db: &DatabaseConnection,
    quality_id: i32,
) -> Result<Option<quality_definitions::Model>, DbErr> {
    quality_definitions::Entity::find_by_id(quality_id)
        .one(db)
        .await
}

/// Replaces the values of one definition. `None` when there is no such row.
pub async fn update(
    db: &DatabaseConnection,
    quality_id: i32,
    values: Values,
) -> Result<Option<quality_definitions::Model>, DbErr> {
    let Some(current) = find(db, quality_id).await? else {
        return Ok(None);
    };
    let mut row: quality_definitions::ActiveModel = current.into();
    row.title = Set(values.title);
    row.min_size = Set(values.min_size);
    row.preferred_size = Set(values.preferred_size);
    row.max_size = Set(values.max_size);
    row.update(db).await.map(Some)
}

#[cfg(test)]
mod tests {
    use serde_json::Value;
    use unseenarr_core::quality::Quality;

    use super::*;
    use crate::testing::temp_db;

    /// A response of `GET /api/v3/qualitydefinition`, recorded from a new
    /// Radarr 7.0.3 (see docs/07-release-scoring.md).
    const RADARR: &str = include_str!("../../../fixtures/radarr/qualitydefinition.json");

    fn number(value: &Value) -> Option<f64> {
        value.as_f64()
    }

    #[tokio::test]
    async fn the_seed_equals_radarrs_defaults_for_every_quality() {
        let (db, _dir) = temp_db().await;
        let seeded = list(&db).await.unwrap();
        let recorded: Vec<Value> = serde_json::from_str(RADARR).unwrap();

        assert_eq!(seeded.len(), Quality::ALL.len());
        assert_eq!(recorded.len(), Quality::ALL.len());
        for quality in Quality::ALL {
            let row = seeded
                .iter()
                .find(|row| row.quality_id == quality.id())
                .unwrap_or_else(|| panic!("no row for {}", quality.name()));
            let radarr = recorded
                .iter()
                .find(|entry| entry["quality"]["id"] == quality.id())
                .unwrap_or_else(|| panic!("Radarr has no {}", quality.name()));
            assert_eq!(radarr["quality"]["name"], quality.name());
            assert_eq!(row.title, radarr["title"].as_str().unwrap());
            assert_eq!(Some(row.min_size), number(&radarr["minSize"]));
            assert_eq!(row.preferred_size, number(&radarr["preferredSize"]));
            assert_eq!(row.max_size, number(&radarr["maxSize"]));
        }
    }

    #[tokio::test]
    async fn update_replaces_the_values_and_reports_a_missing_row() {
        let (db, _dir) = temp_db().await;
        let id = Quality::Bluray1080p.id();
        let changed = update(
            &db,
            id,
            Values {
                title: "Blu-ray 1080p".into(),
                min_size: 12.5,
                preferred_size: None,
                max_size: Some(90.0),
            },
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(changed.max_size, Some(90.0));
        assert_eq!(find(&db, id).await.unwrap().unwrap(), changed);
        assert!(
            update(
                &db,
                999,
                Values {
                    title: "x".into(),
                    min_size: 0.0,
                    preferred_size: None,
                    max_size: None,
                },
            )
            .await
            .unwrap()
            .is_none()
        );
    }
}
