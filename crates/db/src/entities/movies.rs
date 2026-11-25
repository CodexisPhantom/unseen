use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "movies")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
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
    pub root_folder_id: i32,
    pub folder_name: String,
    pub monitored: bool,
    pub minimum_availability: String,
    pub language_profile_id: Option<i32>,
    pub added_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
