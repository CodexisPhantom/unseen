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
        #[sea_orm(column_name = "twoFactorEnabled")]
        #[auth(alias = "twoFactorEnabled")]
        pub two_factor_enabled: Option<bool>,
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
pub mod two_factor {
    use super::*;
    #[derive(Clone, Debug, serde::Serialize, DeriveEntityModel, AuthEntity)]
    #[auth(role = "two_factor", native_two_factor)]
    #[sea_orm(table_name = "twoFactor")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub secret: String,
        #[sea_orm(column_name = "backupCodes")]
        #[auth(alias = "backupCodes")]
        pub backup_codes: String,
        #[sea_orm(column_name = "userId")]
        #[auth(alias = "userId")]
        pub user_id: String,
        pub verified: Option<bool>,
        #[sea_orm(column_name = "failedVerificationCount")]
        #[auth(alias = "failedVerificationCount")]
        pub failed_verification_count: Option<i64>,
        #[sea_orm(column_name = "lockedUntil")]
        #[auth(alias = "lockedUntil")]
        pub locked_until: Option<DateTimeUtc>,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
pub mod passkey {
    use super::*;
    #[derive(Clone, Debug, serde::Serialize, DeriveEntityModel, AuthEntity)]
    #[auth(role = "passkey", native_passkey)]
    #[sea_orm(table_name = "passkey")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub name: Option<String>,
        #[sea_orm(column_name = "publicKey")]
        #[auth(alias = "publicKey")]
        pub public_key: String,
        #[sea_orm(column_name = "userId")]
        #[auth(alias = "userId")]
        pub user_id: String,
        #[sea_orm(column_name = "credentialID")]
        #[auth(alias = "credentialID")]
        pub credential_id: String,
        pub counter: i64,
        #[sea_orm(column_name = "deviceType")]
        #[auth(alias = "deviceType")]
        pub device_type: String,
        #[sea_orm(column_name = "backedUp")]
        #[auth(alias = "backedUp")]
        pub backed_up: bool,
        pub transports: Option<String>,
        #[sea_orm(column_name = "createdAt")]
        #[auth(alias = "createdAt")]
        pub created_at: Option<DateTimeUtc>,
        pub aaguid: Option<String>,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
pub type AppPluginSchema = better_auth::seaorm::PluginModels<
    <better_auth::seaorm::PluginModels as better_auth::seaorm::SeaOrmPluginSchema>::ApiKey,
    <better_auth::seaorm::PluginModels as better_auth::seaorm::SeaOrmPluginSchema>::DeviceCode,
    passkey::Model,
    two_factor::Model,
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
                if column.get_column_name().as_str() == "twoFactorEnabled" {
                    column.integer();
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
            table.table(Alias::new("twoFactor"));
            for mut column in schema
                .create_table_from_entity(two_factor::Entity)
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
                if column.get_column_name().as_str() == "secret" {
                    column.text();
                }
                if column.get_column_name().as_str() == "backupCodes" {
                    column.text();
                }
                if column.get_column_name().as_str() == "userId" {
                    if database.get_database_backend() == sea_orm::DbBackend::MySql {
                        column.string_len(36);
                    } else {
                        column.text();
                    }
                }
                if column.get_column_name().as_str() == "verified" {
                    column.integer();
                }
                if column.get_column_name().as_str() == "failedVerificationCount" {
                    column.integer();
                }
                if column.get_column_name().as_str() == "lockedUntil" {
                    column.custom(Alias::new("date"));
                }
                table.col(column);
            }
            table
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_twoFactor_userId")
                        .from(Alias::new("twoFactor"), Alias::new("userId"))
                        .to(Alias::new("user"), Alias::new("id"))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned()
        },
        {
            let mut table = Table::create();
            table.table(Alias::new("passkey"));
            for mut column in schema
                .create_table_from_entity(passkey::Entity)
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
                if column.get_column_name().as_str() == "publicKey" {
                    column.text();
                }
                if column.get_column_name().as_str() == "userId" {
                    if database.get_database_backend() == sea_orm::DbBackend::MySql {
                        column.string_len(36);
                    } else {
                        column.text();
                    }
                }
                if column.get_column_name().as_str() == "credentialID" {
                    column.text();
                }
                if column.get_column_name().as_str() == "counter" {
                    column.integer();
                }
                if column.get_column_name().as_str() == "deviceType" {
                    column.text();
                }
                if column.get_column_name().as_str() == "backedUp" {
                    column.integer();
                }
                if column.get_column_name().as_str() == "transports" {
                    column.text();
                }
                if column.get_column_name().as_str() == "createdAt" {
                    column.custom(Alias::new("date"));
                }
                if column.get_column_name().as_str() == "aaguid" {
                    column.text();
                }
                table.col(column);
            }
            table
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_passkey_userId")
                        .from(Alias::new("passkey"), Alias::new("userId"))
                        .to(Alias::new("user"), Alias::new("id"))
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned()
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
            .name("twoFactor_secret_idx")
            .table(Alias::new("twoFactor"))
            .col(Alias::new("secret"))
            .to_owned(),
        Index::create()
            .name("twoFactor_userId_idx")
            .table(Alias::new("twoFactor"))
            .col(Alias::new("userId"))
            .to_owned(),
        Index::create()
            .name("passkey_userId_idx")
            .table(Alias::new("passkey"))
            .col(Alias::new("userId"))
            .to_owned(),
        Index::create()
            .name("passkey_credentialID_idx")
            .table(Alias::new("passkey"))
            .col(Alias::new("credentialID"))
            .to_owned(),
    ] {
        let _ = database.execute(&statement).await?;
    }
    Ok(())
}
