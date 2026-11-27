use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "shows")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub tvdb_id: Option<i32>,
    pub tmdb_id: Option<i32>,
    pub imdb_id: Option<String>,
    pub metadata_provider: String,
    pub title: String,
    pub original_title: Option<String>,
    pub sort_title: String,
    pub year: i32,
    pub overview: Option<String>,
    pub runtime: Option<i32>,
    pub status: String,
    pub series_type: String,
    pub network: Option<String>,
    pub poster_path: Option<String>,
    pub season_folder: bool,
    pub root_folder_id: i32,
    pub folder_name: String,
    pub monitored: bool,
    pub monitor_new_items: String,
    pub refreshed_at: Option<String>,
    pub added_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
