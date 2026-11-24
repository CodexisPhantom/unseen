//! The audit log: append-only (docs/11-security.md, "Audit log"). This module
//! only inserts and reads; nothing updates or deletes a row.

use sea_orm::prelude::Json;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, Select, Set,
};

use crate::entities::audit_log;

/// One event to record. `data` must never hold a secret value.
#[derive(Debug, Default, Clone)]
pub struct NewEntry {
    pub actor_user_id: Option<String>,
    /// Copied into the row: it stays readable when the user is deleted.
    pub actor_name: String,
    pub api_key_id: Option<i32>,
    pub action: String,
    pub target_kind: Option<String>,
    pub target_id: Option<String>,
    pub data: Option<Json>,
    pub ip_address: Option<String>,
}

pub async fn record(db: &DatabaseConnection, entry: NewEntry) -> Result<(), DbErr> {
    audit_log::ActiveModel {
        created_at: Set(crate::now()),
        actor_user_id: Set(entry.actor_user_id),
        actor_name: Set(entry.actor_name),
        api_key_id: Set(entry.api_key_id),
        action: Set(entry.action),
        target_kind: Set(entry.target_kind),
        target_id: Set(entry.target_id),
        data: Set(entry.data),
        ip_address: Set(entry.ip_address),
        ..Default::default()
    }
    .insert(db)
    .await
    .map(|_| ())
}

/// Every field narrows the result; `None` does not.
#[derive(Debug, Default, Clone)]
pub struct Filter {
    pub actor_user_id: Option<String>,
    pub action: Option<String>,
    pub target_kind: Option<String>,
    pub target_id: Option<String>,
    /// Inclusive, in the stored timestamp format.
    pub from: Option<String>,
    /// Exclusive.
    pub to: Option<String>,
}

impl Filter {
    fn apply(&self, mut query: Select<audit_log::Entity>) -> Select<audit_log::Entity> {
        use audit_log::Column;
        if let Some(actor) = &self.actor_user_id {
            query = query.filter(Column::ActorUserId.eq(actor.as_str()));
        }
        if let Some(action) = &self.action {
            query = query.filter(Column::Action.eq(action.as_str()));
        }
        if let Some(kind) = &self.target_kind {
            query = query.filter(Column::TargetKind.eq(kind.as_str()));
        }
        if let Some(id) = &self.target_id {
            query = query.filter(Column::TargetId.eq(id.as_str()));
        }
        if let Some(from) = &self.from {
            query = query.filter(Column::CreatedAt.gte(from.as_str()));
        }
        if let Some(to) = &self.to {
            query = query.filter(Column::CreatedAt.lt(to.as_str()));
        }
        query
    }
}

/// One page of the matching rows, newest first, and the number of matches.
/// `page` starts at 1.
pub async fn list(
    db: &DatabaseConnection,
    filter: &Filter,
    page: u64,
    page_size: u64,
) -> Result<(Vec<audit_log::Model>, u64), DbErr> {
    let query = filter
        .apply(audit_log::Entity::find())
        .order_by_desc(audit_log::Column::Id);
    let paginator = query.paginate(db, page_size);
    let total = paginator.num_items().await?;
    let items = paginator.fetch_page(page.saturating_sub(1)).await?;
    Ok((items, total))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::testing::{insert_user, temp_db};

    fn entry(actor: &str, action: &str, target: &str) -> NewEntry {
        NewEntry {
            actor_user_id: Some(actor.into()),
            actor_name: actor.into(),
            action: action.into(),
            target_kind: Some("role".into()),
            target_id: Some(target.into()),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn rows_are_listed_newest_first_and_filtered() {
        let (db, _dir) = temp_db().await;
        insert_user(&db, "u1").await;
        insert_user(&db, "u2").await;
        record(&db, entry("u1", "role.created", "1")).await.unwrap();
        record(&db, entry("u2", "role.updated", "1")).await.unwrap();
        record(&db, entry("u1", "role.updated", "2")).await.unwrap();

        let (all, total) = list(&db, &Filter::default(), 1, 50).await.unwrap();
        assert_eq!(total, 3);
        assert_eq!(all[0].target_id.as_deref(), Some("2"));

        let by_actor = Filter {
            actor_user_id: Some("u1".into()),
            ..Default::default()
        };
        assert_eq!(list(&db, &by_actor, 1, 50).await.unwrap().1, 2);
        let by_action = Filter {
            action: Some("role.updated".into()),
            target_id: Some("1".into()),
            ..Default::default()
        };
        let (rows, total) = list(&db, &by_action, 1, 50).await.unwrap();
        assert_eq!((rows.len(), total), (1, 1));
        let future = Filter {
            from: Some("9999-01-01T00:00:00.000Z".into()),
            ..Default::default()
        };
        assert_eq!(list(&db, &future, 1, 50).await.unwrap().1, 0);
        let past = Filter {
            to: Some("2000-01-01T00:00:00.000Z".into()),
            ..Default::default()
        };
        assert_eq!(list(&db, &past, 1, 50).await.unwrap().1, 0);

        // Pages of two: the second holds the oldest row.
        let (second, total) = list(&db, &Filter::default(), 2, 2).await.unwrap();
        assert_eq!((second.len(), total), (1, 3));
    }

    #[tokio::test]
    async fn a_row_stays_readable_after_its_actor_is_deleted() {
        let (db, _dir) = temp_db().await;
        insert_user(&db, "gone").await;
        let mut new = entry("gone", "user.updated", "x");
        new.data = Some(json!({ "name": { "old": "a", "new": "b" } }));
        record(&db, new).await.unwrap();

        crate::users::delete(&db, "gone").await.unwrap();

        let (rows, _) = list(&db, &Filter::default(), 1, 50).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].actor_user_id, None);
        assert_eq!(rows[0].actor_name, "gone");
        assert_eq!(
            rows[0].data,
            Some(json!({ "name": { "old": "a", "new": "b" } }))
        );
    }
}
