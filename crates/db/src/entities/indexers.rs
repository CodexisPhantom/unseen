use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "indexers")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    pub kind: String,
    pub url: String,
    /// Encrypted (`v1:...`), never plain text.
    pub api_key: String,
    pub categories: Json,
    pub priority: i32,
    pub use_proxy: bool,
    pub enabled: bool,
    pub created_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
