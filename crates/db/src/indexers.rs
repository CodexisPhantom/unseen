//! Indexers and their tags (docs/04-database.md, "Integration tables"). The
//! API key arrives and leaves encrypted: this module never sees plain text.

use std::collections::HashMap;

use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DatabaseTransaction, DbErr, EntityTrait,
    QueryFilter, QueryOrder, Set, SqlErr, TransactionTrait,
};

use crate::entities::{indexer_tags, indexers};

/// An indexer with the ids of its tags, sorted.
#[derive(Debug, Clone, PartialEq)]
pub struct Indexer {
    pub model: indexers::Model,
    pub tags: Vec<i32>,
}

/// What a caller sets on an indexer.
#[derive(Debug, Clone, PartialEq)]
pub struct Values {
    pub name: String,
    pub kind: String,
    pub url: String,
    /// The encrypted key, as `secret::encrypt` returns it.
    pub api_key: String,
    pub categories: serde_json::Value,
    pub priority: i32,
    pub use_proxy: bool,
    pub enabled: bool,
    pub tags: Vec<i32>,
}

#[derive(Debug)]
pub enum IndexerError {
    NotFound,
    NameTaken,
    /// A tag id does not exist.
    UnknownTag,
    Db(DbErr),
}

impl From<DbErr> for IndexerError {
    fn from(error: DbErr) -> Self {
        match error.sql_err() {
            Some(SqlErr::UniqueConstraintViolation(_)) => Self::NameTaken,
            Some(SqlErr::ForeignKeyConstraintViolation(_)) => Self::UnknownTag,
            _ => Self::Db(error),
        }
    }
}

/// Every indexer, by name.
pub async fn list(db: &DatabaseConnection) -> Result<Vec<Indexer>, DbErr> {
    let models = indexers::Entity::find()
        .order_by_asc(indexers::Column::Name)
        .all(db)
        .await?;
    let mut tags: HashMap<i32, Vec<i32>> = HashMap::new();
    for link in indexer_tags::Entity::find()
        .order_by_asc(indexer_tags::Column::TagId)
        .all(db)
        .await?
    {
        tags.entry(link.indexer_id).or_default().push(link.tag_id);
    }
    Ok(models
        .into_iter()
        .map(|model| Indexer {
            tags: tags.remove(&model.id).unwrap_or_default(),
            model,
        })
        .collect())
}

pub async fn find(db: &DatabaseConnection, id: i32) -> Result<Option<Indexer>, DbErr> {
    let Some(model) = indexers::Entity::find_by_id(id).one(db).await? else {
        return Ok(None);
    };
    let tags = tags_of(db, id).await?;
    Ok(Some(Indexer { model, tags }))
}

async fn tags_of(db: &DatabaseConnection, id: i32) -> Result<Vec<i32>, DbErr> {
    Ok(indexer_tags::Entity::find()
        .filter(indexer_tags::Column::IndexerId.eq(id))
        .order_by_asc(indexer_tags::Column::TagId)
        .all(db)
        .await?
        .into_iter()
        .map(|link| link.tag_id)
        .collect())
}

async fn set_tags(txn: &DatabaseTransaction, id: i32, tags: &[i32]) -> Result<(), IndexerError> {
    indexer_tags::Entity::delete_many()
        .filter(indexer_tags::Column::IndexerId.eq(id))
        .exec(txn)
        .await?;
    let mut unique = tags.to_vec();
    unique.sort_unstable();
    unique.dedup();
    for tag_id in unique {
        indexer_tags::ActiveModel {
            indexer_id: Set(id),
            tag_id: Set(tag_id),
        }
        .insert(txn)
        .await?;
    }
    Ok(())
}

pub async fn create(db: &DatabaseConnection, values: Values) -> Result<Indexer, IndexerError> {
    let txn = db.begin().await?;
    let model = indexers::ActiveModel {
        name: Set(values.name),
        kind: Set(values.kind),
        url: Set(values.url),
        api_key: Set(values.api_key),
        categories: Set(values.categories),
        priority: Set(values.priority),
        use_proxy: Set(values.use_proxy),
        enabled: Set(values.enabled),
        created_at: Set(crate::now()),
        ..Default::default()
    }
    .insert(&txn)
    .await?;
    set_tags(&txn, model.id, &values.tags).await?;
    txn.commit().await?;
    find(db, model.id).await?.ok_or(IndexerError::NotFound)
}

/// Replaces every editable field and the tag set.
pub async fn update(
    db: &DatabaseConnection,
    id: i32,
    values: Values,
) -> Result<Indexer, IndexerError> {
    let txn = db.begin().await?;
    let mut indexer: indexers::ActiveModel = indexers::Entity::find_by_id(id)
        .one(&txn)
        .await?
        .ok_or(IndexerError::NotFound)?
        .into();
    indexer.name = Set(values.name);
    indexer.kind = Set(values.kind);
    indexer.url = Set(values.url);
    indexer.api_key = Set(values.api_key);
    indexer.categories = Set(values.categories);
    indexer.priority = Set(values.priority);
    indexer.use_proxy = Set(values.use_proxy);
    indexer.enabled = Set(values.enabled);
    indexer.update(&txn).await?;
    set_tags(&txn, id, &values.tags).await?;
    txn.commit().await?;
    find(db, id).await?.ok_or(IndexerError::NotFound)
}

pub async fn delete(db: &DatabaseConnection, id: i32) -> Result<(), IndexerError> {
    if indexers::Entity::delete_by_id(id)
        .exec(db)
        .await?
        .rows_affected
        == 0
    {
        return Err(IndexerError::NotFound);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::entities::tags;
    use crate::testing::temp_db;

    fn values(name: &str) -> Values {
        Values {
            name: name.into(),
            kind: "torznab".into(),
            url: "http://indexer.test/api".into(),
            api_key: "v1:x:y".into(),
            categories: json!([2000]),
            priority: 25,
            use_proxy: true,
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
            Err(IndexerError::NameTaken)
        ));
        let mut unknown = values("Gamma");
        unknown.tags = vec![999];
        assert!(matches!(
            create(&db, unknown).await,
            Err(IndexerError::UnknownTag)
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
        changed.priority = 10;
        changed.tags = vec![a];
        let updated = update(&db, beta.model.id, changed).await.unwrap();
        assert_eq!((updated.model.priority, updated.tags), (10, vec![a]));
        assert!(matches!(
            update(&db, 999, values("x")).await,
            Err(IndexerError::NotFound)
        ));

        delete(&db, beta.model.id).await.unwrap();
        assert!(matches!(
            delete(&db, beta.model.id).await,
            Err(IndexerError::NotFound)
        ));
        let links = indexer_tags::Entity::find().all(&db).await.unwrap();
        assert!(links.is_empty(), "tag links are deleted with the indexer");
    }
}
