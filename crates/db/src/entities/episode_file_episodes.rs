use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "episode_file_episodes")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub episode_file_id: i32,
    #[sea_orm(primary_key, auto_increment = false)]
    pub episode_id: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
