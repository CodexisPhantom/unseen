//! Download clients and their tags (docs/04-database.md, "Integration
//! tables"). The password arrives and leaves encrypted: this module never
//! sees plain text.

use std::collections::HashMap;

use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DatabaseTransaction, DbErr, EntityTrait,
    QueryFilter, QueryOrder, Set, SqlErr, TransactionTrait,
};

use crate::entities::{download_client_tags, download_clients};

/// An client with the ids of its tags, sorted.
#[derive(Debug, Clone, PartialEq)]
pub struct Client {
    pub model: download_clients::Model,
    pub tags: Vec<i32>,
}

/// What a caller sets on an client.
#[derive(Debug, Clone, PartialEq)]
pub struct Values {
    pub name: String,
    pub kind: String,
    pub url: String,
    pub username: Option<String>,
    /// The encrypted password, as `secret::encrypt` returns it.
    pub password: Option<String>,
    /// The encrypted API key, as `secret::encrypt` returns it.
    pub api_key: Option<String>,
    pub category: String,
    pub remote_path: Option<String>,
    pub local_path: Option<String>,
    pub enabled: bool,
    pub tags: Vec<i32>,
}

#[derive(Debug)]
pub enum ClientError {
    NotFound,
    NameTaken,
    /// A tag id does not exist.
    UnknownTag,
    Db(DbErr),
}

impl From<DbErr> for ClientError {
    fn from(error: DbErr) -> Self {
        match error.sql_err() {
            Some(SqlErr::UniqueConstraintViolation(_)) => Self::NameTaken,
            Some(SqlErr::ForeignKeyConstraintViolation(_)) => Self::UnknownTag,
            _ => Self::Db(error),
        }
    }
}

/// Every client, by name.
pub async fn list(db: &DatabaseConnection) -> Result<Vec<Client>, DbErr> {
    let models = download_clients::Entity::find()
        .order_by_asc(download_clients::Column::Name)
        .all(db)
        .await?;
    let mut tags: HashMap<i32, Vec<i32>> = HashMap::new();
    for link in download_client_tags::Entity::find()
        .order_by_asc(download_client_tags::Column::TagId)
        .all(db)
        .await?
    {
        tags.entry(link.download_client_id)
            .or_default()
            .push(link.tag_id);
    }
    Ok(models
        .into_iter()
        .map(|model| Client {
            tags: tags.remove(&model.id).unwrap_or_default(),
            model,
        })
        .collect())
}

pub async fn find(db: &DatabaseConnection, id: i32) -> Result<Option<Client>, DbErr> {
    let Some(model) = download_clients::Entity::find_by_id(id).one(db).await? else {
        return Ok(None);
    };
    let tags = tags_of(db, id).await?;
    Ok(Some(Client { model, tags }))
}

async fn tags_of(db: &DatabaseConnection, id: i32) -> Result<Vec<i32>, DbErr> {
    Ok(download_client_tags::Entity::find()
        .filter(download_client_tags::Column::DownloadClientId.eq(id))
        .order_by_asc(download_client_tags::Column::TagId)
        .all(db)
        .await?
        .into_iter()
        .map(|link| link.tag_id)
        .collect())
}

async fn set_tags(txn: &DatabaseTransaction, id: i32, tags: &[i32]) -> Result<(), ClientError> {
    download_client_tags::Entity::delete_many()
        .filter(download_client_tags::Column::DownloadClientId.eq(id))
        .exec(txn)
        .await?;
    let mut unique = tags.to_vec();
    unique.sort_unstable();
    unique.dedup();
    for tag_id in unique {
        download_client_tags::ActiveModel {
            download_client_id: Set(id),
            tag_id: Set(tag_id),
        }
        .insert(txn)
        .await?;
    }
    Ok(())
}

pub async fn create(db: &DatabaseConnection, values: Values) -> Result<Client, ClientError> {
    let txn = db.begin().await?;
    let model = download_clients::ActiveModel {
        name: Set(values.name),
        kind: Set(values.kind),
        url: Set(values.url),
        username: Set(values.username),
        password: Set(values.password),
        api_key: Set(values.api_key),
        category: Set(values.category),
        remote_path: Set(values.remote_path),
        local_path: Set(values.local_path),
        enabled: Set(values.enabled),
        created_at: Set(crate::now()),
        ..Default::default()
    }
    .insert(&txn)
    .await?;
    set_tags(&txn, model.id, &values.tags).await?;
    txn.commit().await?;
    find(db, model.id).await?.ok_or(ClientError::NotFound)
}

/// Replaces every editable field and the tag set.
pub async fn update(
    db: &DatabaseConnection,
    id: i32,
    values: Values,
) -> Result<Client, ClientError> {
    let txn = db.begin().await?;
    let mut client: download_clients::ActiveModel = download_clients::Entity::find_by_id(id)
        .one(&txn)
        .await?
        .ok_or(ClientError::NotFound)?
        .into();
    client.name = Set(values.name);
    client.kind = Set(values.kind);
    client.url = Set(values.url);
    client.username = Set(values.username);
    client.password = Set(values.password);
    client.api_key = Set(values.api_key);
    client.category = Set(values.category);
    client.remote_path = Set(values.remote_path);
    client.local_path = Set(values.local_path);
    client.enabled = Set(values.enabled);
    client.update(&txn).await?;
    set_tags(&txn, id, &values.tags).await?;
    txn.commit().await?;
    find(db, id).await?.ok_or(ClientError::NotFound)
}

pub async fn delete(db: &DatabaseConnection, id: i32) -> Result<(), ClientError> {
    if download_clients::Entity::delete_by_id(id)
        .exec(db)
        .await?
        .rows_affected
        == 0
    {
        return Err(ClientError::NotFound);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::tags;
    use crate::testing::temp_db;

    fn values(name: &str) -> Values {
        Values {
            name: name.into(),
            kind: "qbittorrent".into(),
            url: "http://qbittorrent:8080".into(),
            username: Some("admin".into()),
            password: Some("v1:x:y".into()),
            api_key: None,
            category: "radarr".into(),
            remote_path: None,
            local_path: None,
            enabled: true,
            tags: vec![],
        }
    }

    async fn tag(db: &DatabaseConnection, label: &str) -> i32 {
        tags::ActiveModel {
            label: Set(label.into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn create_update_list_and_delete() {
        let (db, _dir) = temp_db().await;
        let (a, b) = (tag(&db, "a").await, tag(&db, "b").await);

        let mut with_tags = values("Beta");
        with_tags.tags = vec![b, a, a];
        let beta = create(&db, with_tags).await.unwrap();
        assert_eq!(beta.tags, [a, b]);
        create(&db, values("Alpha")).await.unwrap();
        assert!(matches!(
            create(&db, values("Alpha")).await,
            Err(ClientError::NameTaken)
        ));
        let mut unknown = values("Gamma");
        unknown.tags = vec![999];
        assert!(matches!(
            create(&db, unknown).await,
            Err(ClientError::UnknownTag)
        ));
        assert!(
            find(&db, beta.model.id).await.unwrap().is_some()
                && list(&db).await.unwrap().len() == 2,
            "a failed create leaves nothing behind"
        );

        let names: Vec<_> = list(&db)
            .await
            .unwrap()
            .into_iter()
            .map(|i| i.model.name)
            .collect();
        assert_eq!(names, ["Alpha", "Beta"]);

        let mut changed = values("Beta");
        changed.category = "movies".into();
        changed.tags = vec![a];
        let updated = update(&db, beta.model.id, changed).await.unwrap();
        assert_eq!(
            (updated.model.category.as_str(), updated.tags),
            ("movies", vec![a])
        );
        assert!(matches!(
            update(&db, 999, values("x")).await,
            Err(ClientError::NotFound)
        ));

        delete(&db, beta.model.id).await.unwrap();
        assert!(matches!(
            delete(&db, beta.model.id).await,
            Err(ClientError::NotFound)
        ));
        let links = download_client_tags::Entity::find().all(&db).await.unwrap();
        assert!(links.is_empty(), "tag links are deleted with the client");
    }
}
