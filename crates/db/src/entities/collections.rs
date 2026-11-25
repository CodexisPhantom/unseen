use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "collections")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub tmdb_id: i32,
    pub title: String,
    pub sort_title: String,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
    pub monitored: bool,
    pub root_folder_id: Option<i32>,
    pub quality_profile_id: Option<i32>,
    pub minimum_availability: String,
    pub added_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
