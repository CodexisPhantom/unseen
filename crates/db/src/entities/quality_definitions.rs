use sea_orm::entity::prelude::*;

/// Megabytes per minute of runtime. The quality itself is a Rust enum in
/// `core`; `quality_id` is Radarr's id.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "quality_definitions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub quality_id: i32,
    pub title: String,
    pub min_size: f64,
    pub preferred_size: Option<f64>,
    pub max_size: Option<f64>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
