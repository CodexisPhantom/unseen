use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "episodes")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub show_id: i32,
    pub season_number: i32,
    pub episode_number: i32,
    pub absolute_number: Option<i32>,
    pub title: String,
    pub overview: Option<String>,
    pub air_date: Option<String>,
    pub air_date_utc: Option<String>,
    pub runtime: Option<i32>,
    pub tvdb_id: Option<i32>,
    pub tmdb_id: Option<i32>,
    pub monitored: bool,
    pub added_at: String,
    pub updated_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
