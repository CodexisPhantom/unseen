use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "movie_profiles")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub movie_id: i32,
    pub quality_profile_id: i32,
    pub is_primary: bool,
    pub created_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
