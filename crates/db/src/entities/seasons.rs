use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "seasons")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub show_id: i32,
    #[sea_orm(primary_key, auto_increment = false)]
    pub season_number: i32,
    pub monitored: bool,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
