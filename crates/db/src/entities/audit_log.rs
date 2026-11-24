use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "audit_log")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub created_at: String,
    /// Null after the user is deleted; `actor_name` still names them.
    pub actor_user_id: Option<String>,
    pub actor_name: String,
    /// The API key behind the request, if it was not a session.
    pub api_key_id: Option<i32>,
    pub action: String,
    pub target_kind: Option<String>,
    pub target_id: Option<String>,
    /// Changed fields with old and new values. Never a secret.
    pub data: Option<Json>,
    pub ip_address: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
