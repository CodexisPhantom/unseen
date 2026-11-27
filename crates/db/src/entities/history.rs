use sea_orm::entity::prelude::*;

/// One activity entry, such as a release being sent to a download client
/// (docs/04-database.md).
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "history")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub movie_id: Option<i32>,
    pub event: String,
    pub data: String,
    pub created_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
