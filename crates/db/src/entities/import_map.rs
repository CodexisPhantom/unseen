use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "import_map")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub source_id: i32,
    #[sea_orm(primary_key, auto_increment = false)]
    pub resource: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub source_resource_id: i32,
    pub local_id: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
