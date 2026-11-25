use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "subtitles")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub movie_file_id: i32,
    pub language: String,
    pub kind: String,
    /// Relative to the movie folder, with `/` between the parts.
    pub relative_path: String,
    /// `None` for a file that was imported or added by the user.
    pub provider: Option<String>,
    pub provider_id: Option<String>,
    pub score: Option<i32>,
    pub added_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
