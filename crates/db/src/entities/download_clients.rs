use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "download_clients")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    pub kind: String,
    pub url: String,
    pub username: Option<String>,
    /// Encrypted (`v1:...`), never plain text.
    pub password: Option<String>,
    /// Encrypted (`v1:...`), never plain text.
    pub api_key: Option<String>,
    pub category: String,
    pub remote_path: Option<String>,
    pub local_path: Option<String>,
    pub remove_completed: bool,
    pub enabled: bool,
    pub created_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
