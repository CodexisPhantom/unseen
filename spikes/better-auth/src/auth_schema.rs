use better_auth::AuthSchema;
use better_auth::seaorm::sea_orm;
use better_auth::seaorm::sea_orm::entity::prelude::*;
use better_auth::seaorm::sea_orm::{ConnectionTrait, Schema};
use better_auth::seaorm::sea_orm::sea_query::{Alias, Index, Table};
use better_auth::seaorm::sea_orm::sea_query::{ForeignKey, ForeignKeyAction};
use better_auth::seaorm::AuthEntity;
pub mod user {
    use super::*;
    #[derive(Clone, Debug, serde::Serialize, DeriveEntityModel, AuthEntity)]
    #[auth(role = "user")]
    #[sea_orm(table_name = "user")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub name: String,
        pub email: Option<String>,
        #[sea_orm(column_name = "emailVerified")]
        #[auth(alias = "emailVerified")]
        pub email_verified: bool,
        pub image: Option<String>,
        #[sea_orm(column_name = "createdAt")]
        #[auth(alias = "createdAt")]
        pub created_at: DateTimeUtc,
        #[sea_orm(column_name = "updatedAt")]
        #[auth(alias = "updatedAt")]
        pub updated_at: DateTimeUtc,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
pub mod session {
    use super::*;
    #[derive(Clone, Debug, serde::Serialize, DeriveEntityModel, AuthEntity)]
    #[auth(role = "session", row_presence)]
    #[sea_orm(table_name = "session")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        #[sea_orm(column_name = "expiresAt")]
        #[auth(alias = "expiresAt")]
        pub expires_at: DateTimeUtc,
        pub token: String,
        #[sea_orm(column_name = "createdAt")]
        #[auth(alias = "createdAt")]
        pub created_at: DateTimeUtc,
        #[sea_orm(column_name = "updatedAt")]
        #[auth(alias = "updatedAt")]
        pub updated_at: DateTimeUtc,
        #[sea_orm(column_name = "ipAddress")]
        #[auth(alias = "ipAddress")]
        pub ip_address: Option<String>,
        #[sea_orm(column_name = "userAgent")]
        #[auth(alias = "userAgent")]
        pub user_agent: Option<String>,
        #[sea_orm(column_name = "userId")]
        #[auth(alias = "userId")]
        pub user_id: String,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
pub mod account {
    use super::*;
    #[derive(Clone, Debug, serde::Serialize, DeriveEntityModel, AuthEntity)]
    #[auth(role = "account")]
    #[sea_orm(table_name = "account")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        #[sea_orm(column_name = "accountId")]
        #[auth(alias = "accountId")]
        pub account_id: String,
        #[sea_orm(column_name = "providerId")]
        #[auth(alias = "providerId")]
        pub provider_id: String,
        #[sea_orm(column_name = "userId")]
        #[auth(alias = "userId")]
        pub user_id: String,
        #[sea_orm(column_name = "accessToken")]
        #[auth(alias = "accessToken")]
        pub access_token: Option<String>,
        #[sea_orm(column_name = "refreshToken")]
        #[auth(alias = "refreshToken")]
        pub refresh_token: Option<String>,
        #[sea_orm(column_name = "idToken")]
        #[auth(alias = "idToken")]
        pub id_token: Option<String>,
        #[sea_orm(column_name = "accessTokenExpiresAt")]
        #[auth(alias = "accessTokenExpiresAt")]
        pub access_token_expires_at: Option<DateTimeUtc>,
        #[sea_orm(column_name = "refreshTokenExpiresAt")]
        #[auth(alias = "refreshTokenExpiresAt")]
        pub refresh_token_expires_at: Option<DateTimeUtc>,
        pub scope: Option<String>,
        pub password: Option<String>,
        #[sea_orm(column_name = "createdAt")]
        #[auth(alias = "createdAt")]
        pub created_at: DateTimeUtc,
        #[sea_orm(column_name = "updatedAt")]
        #[auth(alias = "updatedAt")]
        pub updated_at: DateTimeUtc,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
pub mod verification {
    use super::*;
    #[derive(Clone, Debug, serde::Serialize, DeriveEntityModel, AuthEntity)]
    #[auth(role = "verification")]
    #[sea_orm(table_name = "verification")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub identifier: String,
        pub value: String,
        #[sea_orm(column_name = "expiresAt")]
        #[auth(alias = "expiresAt")]
        pub expires_at: DateTimeUtc,
        #[sea_orm(column_name = "createdAt")]
        #[auth(alias = "createdAt")]
        pub created_at: DateTimeUtc,
        #[sea_orm(column_name = "updatedAt")]
        #[auth(alias = "updatedAt")]
        pub updated_at: DateTimeUtc,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
pub mod api_key {
    use super::*;
    #[derive(Clone, Debug, serde::Serialize, DeriveEntityModel, AuthEntity)]
    #[auth(role = "api_key")]
    #[sea_orm(table_name = "apikey")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        #[sea_orm(column_name = "configId")]
        #[auth(alias = "configId")]
        pub config_id: String,
        pub name: Option<String>,
        pub start: Option<String>,
        #[sea_orm(column_name = "referenceId")]
        #[auth(alias = "referenceId")]
        pub reference_id: String,
        pub prefix: Option<String>,
        #[sea_orm(column_name = "key")]
        #[auth(alias = "key")]
        pub key_hash: String,
        #[sea_orm(column_name = "refillInterval")]
        #[sea_orm(column_type = "Integer", nullable)]
        #[auth(alias = "refillInterval")]
        pub refill_interval: Option<better_auth::seaorm::SqlNumber>,
        #[sea_orm(column_name = "refillAmount")]
        #[sea_orm(column_type = "Integer", nullable)]
        #[auth(alias = "refillAmount")]
        pub refill_amount: Option<better_auth::seaorm::SqlNumber>,
        #[sea_orm(column_name = "lastRefillAt")]
        #[auth(alias = "lastRefillAt")]
        pub last_refill_at: Option<DateTimeUtc>,
        pub enabled: Option<bool>,
        #[sea_orm(column_name = "rateLimitEnabled")]
        #[auth(alias = "rateLimitEnabled")]
        pub rate_limit_enabled: Option<bool>,
        #[sea_orm(column_name = "rateLimitTimeWindow")]
        #[sea_orm(column_type = "Integer", nullable)]
        #[auth(alias = "rateLimitTimeWindow")]
        pub rate_limit_time_window: Option<better_auth::seaorm::SqlNumber>,
        #[sea_orm(column_name = "rateLimitMax")]
        #[sea_orm(column_type = "Integer", nullable)]
        #[auth(alias = "rateLimitMax")]
        pub rate_limit_max: Option<better_auth::seaorm::SqlNumber>,
        #[sea_orm(column_name = "requestCount")]
        #[sea_orm(column_type = "Integer", nullable)]
        #[auth(alias = "requestCount")]
        pub request_count: Option<better_auth::seaorm::SqlNumber>,
        #[sea_orm(column_type = "Integer", nullable)]
        pub remaining: Option<better_auth::seaorm::SqlNumber>,
        #[sea_orm(column_name = "lastRequest")]
        #[auth(alias = "lastRequest")]
        pub last_request: Option<DateTimeUtc>,
        #[sea_orm(column_name = "expiresAt")]
        #[auth(alias = "expiresAt")]
        pub expires_at: Option<DateTimeUtc>,
        #[sea_orm(column_name = "createdAt")]
        #[auth(alias = "createdAt")]
        pub created_at: DateTimeUtc,
        #[sea_orm(column_name = "updatedAt")]
        #[auth(alias = "updatedAt")]
        pub updated_at: DateTimeUtc,
        pub permissions: Option<String>,
        pub metadata: Option<String>,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
pub type AppPluginSchema = better_auth::seaorm::PluginModels<
    api_key::Model,
    <better_auth::seaorm::PluginModels as better_auth::seaorm::SeaOrmPluginSchema>::DeviceCode,
    <better_auth::seaorm::PluginModels as better_auth::seaorm::SeaOrmPluginSchema>::Passkey,
    <better_auth::seaorm::PluginModels as better_auth::seaorm::SeaOrmPluginSchema>::TwoFactor,
    <better_auth::seaorm::PluginModels as better_auth::seaorm::SeaOrmPluginSchema>::Jwk,
    <better_auth::seaorm::PluginModels as better_auth::seaorm::SeaOrmPluginSchema>::WalletAddress,
    <better_auth::seaorm::PluginModels as better_auth::seaorm::SeaOrmPluginSchema>::RateLimit,
>;
pub struct AppAuthSchema;
impl AuthSchema for AppAuthSchema {
    type User = user::Model;
    type Session = session::Model;
    type Account = account::Model;
    type Verification = verification::Model;
    fn model_declarations() -> &'static [better_auth::schema::ModelDeclaration] {
        &[]
    }
}
/// Create auth tables in an empty database.
///
/// Call this scaffold from the application's initial migration.
/// Use versioned migrations to change existing tables.
pub async fn create_auth_tables(
    database: &impl ConnectionTrait,
) -> Result<(), sea_orm::DbErr> {
    let schema = Schema::new(database.get_database_backend());
    for statement in [
        {
            let mut table = Table::create();
            table.table(Alias::new("user"));
            for mut column in schema
                .create_table_from_entity(user::Entity)
                .get_columns()
                .clone()
            {
                if column.get_column_name().as_str() == "id" {
                    if database.get_database_backend() == sea_orm::DbBackend::MySql {
                        column.string_len(36);
                    } else {
                        column.text();
                    }
                }
                if column.get_column_name().as_str() == "name" {
                    column.text();
                }
                if column.get_column_name().as_str() == "email" {
                    column.text();
                    column.unique_key();
                    column.not_null();
                }
                if column.get_column_name().as_str() == "emailVerified" {
                    column.integer();
                }
                if column.get_column_name().as_str() == "image" {
                    column.text();
                }
                if column.get_column_name().as_str() == "createdAt" {
                    column.custom(Alias::new("date"));
                }
                if column.get_column_name().as_str() == "updatedAt" {
                    column.custom(Alias::new("date"));
                }
                table.col(column);
            }
            table.to_owned()
        },
        {
            let mut table = Table::create();
            table.table(Alias::new("session"));
            for mut column in schema
                .create_table_from_entity(session::Entity)
                .get_columns()
                .clone()
            {
                if column.get_column_name().as_str() == "id" {
                    if database.get_database_backend() == sea_orm::DbBackend::MySql {
                        column.string_len(36);
                    } else {
                        column.text();
                    }
                }
                if column.get_column_name().as_str() == "expiresAt" {
                    column.custom(Alias::new("date"));
                }
                if column.get_column_name().as_str() == "token" {
                    column.text();
                    column.unique_key();
                }
                if column.get_column_name().as_str() == "createdAt" {
                    column.custom(Alias::new("date"));
                }
                if column.get_column_name().as_str() == "updatedAt" {
                    column.custom(Alias::new("date"));
                }
                if column.get_column_name().as_str() == "ipAddress" {
                    column.text();
                }
                if column.get_column_name().as_str() == "userAgent" {
                    column.text();
                }
                if column.get_column_name().as_str() == "userId" {
                    if database.get_database_backend() == sea_orm::DbBackend::MySql {
                        column.string_len(36);
                    } else {
                        column.text();
                    }
                }
                table.col(column);
            }
            table
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_session_userId")
                        .from(Alias::new("session"), Alias::new("userId"))
                        .to(Alias::new("user"), Alias::new("id"))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned()
        },
        {
            let mut table = Table::create();
            table.table(Alias::new("account"));
            for mut column in schema
                .create_table_from_entity(account::Entity)
                .get_columns()
                .clone()
            {
                if column.get_column_name().as_str() == "id" {
                    if database.get_database_backend() == sea_orm::DbBackend::MySql {
                        column.string_len(36);
                    } else {
                        column.text();
                    }
                }
                if column.get_column_name().as_str() == "accountId" {
                    column.text();
                }
                if column.get_column_name().as_str() == "providerId" {
                    column.text();
                }
                if column.get_column_name().as_str() == "userId" {
                    if database.get_database_backend() == sea_orm::DbBackend::MySql {
                        column.string_len(36);
                    } else {
                        column.text();
                    }
                }
                if column.get_column_name().as_str() == "accessToken" {
                    column.text();
                }
                if column.get_column_name().as_str() == "refreshToken" {
                    column.text();
                }
                if column.get_column_name().as_str() == "idToken" {
                    column.text();
                }
                if column.get_column_name().as_str() == "accessTokenExpiresAt" {
                    column.custom(Alias::new("date"));
                }
                if column.get_column_name().as_str() == "refreshTokenExpiresAt" {
                    column.custom(Alias::new("date"));
                }
                if column.get_column_name().as_str() == "scope" {
                    column.text();
                }
                if column.get_column_name().as_str() == "password" {
                    column.text();
                }
                if column.get_column_name().as_str() == "createdAt" {
                    column.custom(Alias::new("date"));
                }
                if column.get_column_name().as_str() == "updatedAt" {
                    column.custom(Alias::new("date"));
                }
                table.col(column);
            }
            table
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_account_userId")
                        .from(Alias::new("account"), Alias::new("userId"))
                        .to(Alias::new("user"), Alias::new("id"))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned()
        },
        {
            let mut table = Table::create();
            table.table(Alias::new("verification"));
            for mut column in schema
                .create_table_from_entity(verification::Entity)
                .get_columns()
                .clone()
            {
                if column.get_column_name().as_str() == "id" {
                    if database.get_database_backend() == sea_orm::DbBackend::MySql {
                        column.string_len(36);
                    } else {
                        column.text();
                    }
                }
                if column.get_column_name().as_str() == "identifier" {
                    column.text();
                }
                if column.get_column_name().as_str() == "value" {
                    column.text();
                }
                if column.get_column_name().as_str() == "expiresAt" {
                    column.custom(Alias::new("date"));
                }
                if column.get_column_name().as_str() == "createdAt" {
                    column.custom(Alias::new("date"));
                }
                if column.get_column_name().as_str() == "updatedAt" {
                    column.custom(Alias::new("date"));
                }
                table.col(column);
            }
            table.to_owned()
        },
        {
            let mut table = Table::create();
            table.table(Alias::new("apikey"));
            for mut column in schema
                .create_table_from_entity(api_key::Entity)
                .get_columns()
                .clone()
            {
                if column.get_column_name().as_str() == "id" {
                    if database.get_database_backend() == sea_orm::DbBackend::MySql {
                        column.string_len(36);
                    } else {
                        column.text();
                    }
                }
                if column.get_column_name().as_str() == "configId" {
                    column.text();
                }
                if column.get_column_name().as_str() == "name" {
                    column.text();
                }
                if column.get_column_name().as_str() == "start" {
                    column.text();
                }
                if column.get_column_name().as_str() == "referenceId" {
                    column.text();
                }
                if column.get_column_name().as_str() == "prefix" {
                    column.text();
                }
                if column.get_column_name().as_str() == "key" {
                    column.text();
                }
                if column.get_column_name().as_str() == "lastRefillAt" {
                    column.custom(Alias::new("date"));
                }
                if column.get_column_name().as_str() == "enabled" {
                    column.integer();
                }
                if column.get_column_name().as_str() == "rateLimitEnabled" {
                    column.integer();
                }
                if column.get_column_name().as_str() == "lastRequest" {
                    column.custom(Alias::new("date"));
                }
                if column.get_column_name().as_str() == "expiresAt" {
                    column.custom(Alias::new("date"));
                }
                if column.get_column_name().as_str() == "createdAt" {
                    column.custom(Alias::new("date"));
                }
                if column.get_column_name().as_str() == "updatedAt" {
                    column.custom(Alias::new("date"));
                }
                if column.get_column_name().as_str() == "permissions" {
                    column.text();
                }
                if column.get_column_name().as_str() == "metadata" {
                    column.text();
                }
                table.col(column);
            }
            table.to_owned()
        },
    ] {
        let _ = database.execute(&statement).await?;
    }
    for statement in [
        Index::create()
            .name("session_userId_idx")
            .table(Alias::new("session"))
            .col(Alias::new("userId"))
            .to_owned(),
        Index::create()
            .name("account_userId_idx")
            .table(Alias::new("account"))
            .col(Alias::new("userId"))
            .to_owned(),
        Index::create()
            .name("verification_identifier_idx")
            .table(Alias::new("verification"))
            .col(Alias::new("identifier"))
            .to_owned(),
        Index::create()
            .name("apikey_configId_idx")
            .table(Alias::new("apikey"))
            .col(Alias::new("configId"))
            .to_owned(),
        Index::create()
            .name("apikey_referenceId_idx")
            .table(Alias::new("apikey"))
            .col(Alias::new("referenceId"))
            .to_owned(),
        Index::create()
            .name("apikey_key_idx")
            .table(Alias::new("apikey"))
            .col(Alias::new("key"))
            .to_owned(),
    ] {
        let _ = database.execute(&statement).await?;
    }
    Ok(())
}
