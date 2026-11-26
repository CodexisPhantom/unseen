use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "quality_profile_custom_formats")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub quality_profile_id: i32,
    #[sea_orm(primary_key, auto_increment = false)]
    pub custom_format_id: i32,
    pub score: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
