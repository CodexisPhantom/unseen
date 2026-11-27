use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "episode_files")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub show_id: i32,
    pub relative_path: String,
    pub size: i64,
    pub quality_id: i32,
    pub release_group: Option<String>,
    pub scene_name: Option<String>,
    pub media_info: Option<Json>,
    pub languages: Option<Json>,
    pub added_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
