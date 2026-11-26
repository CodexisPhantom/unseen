use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "blocklist")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub movie_id: i32,
    pub release_title: String,
    pub indexer_id: Option<i32>,
    pub reason: String,
    pub created_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
