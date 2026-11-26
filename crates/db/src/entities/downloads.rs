use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "downloads")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub movie_id: i32,
    pub movie_profile_id: Option<i32>,
    pub indexer_id: Option<i32>,
    pub download_client_id: Option<i32>,
    pub client_download_id: String,
    pub release_title: String,
    pub quality_id: i32,
    pub custom_format_score: i32,
    pub size: i64,
    pub state: String,
    pub error: Option<String>,
    pub attempts: i32,
    pub grabbed_at: String,
    pub completed_at: Option<String>,
    pub imported_at: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
