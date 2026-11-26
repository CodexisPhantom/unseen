use api::{AppState, router};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use better_auth::seaorm::sea_orm::{ActiveModelTrait, EntityTrait, Set};
use db::entities::{movies, root_folders};
use db::entities::{role_permissions, roles, user_roles};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

const HOST: &str = "unseen.test";
const ORIGIN: &str = "http://unseen.test";
const PASSWORD: &str = "correct horse";

struct App {
    router: Router,
    state: AppState,
    owner: String,
    _dir: TempDir,
}

impl App {
    async fn new() -> Self {
        let (db, dir) = db::testing::temp_db().await;
        let state = AppState::new(db, "test secret", None).await.unwrap();
        let mut app = Self {
            router: router(state.clone()),
            state,
            owner: String::new(),
            _dir: dir,
        };
        let body = json!({ "name": "Owner", "email": "owner@example.com", "password": PASSWORD });
        let (status, cookie, reply) = app
            .send(Method::POST, "/api/v1/setup", None, Some(body))
            .await;
        assert_eq!(status, StatusCode::CREATED, "{reply}");
        app.owner = cookie.unwrap();
        app
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        cookie: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Option<String>, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, HOST)
            .header(header::ORIGIN, ORIGIN);
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        let body = match body {
            Some(body) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                Body::from(body.to_string())
            }
            None => Body::empty(),
        };
        let response = self
            .router
            .clone()
            .oneshot(request.body(body).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let cookie = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|value| value.to_str().unwrap())
            .find(|cookie| cookie.contains("session_token="))
            .map(|cookie| cookie.split(';').next().unwrap().to_owned());
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (
            status,
            cookie,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn owner(&self, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let (status, _, body) = self.send(method, path, Some(&self.owner), body).await;
        (status, body)
    }

    /// A user who holds a new role with `permissions`. Returns the user's
    /// cookie and the role id.
    async fn member(&self, email: &str, permissions: &[&str]) -> (String, String, i32) {
        self.state.policy.admit_next_signup();
        let body = json!({ "name": "Member", "email": email, "password": PASSWORD });
        let (status, cookie, reply) = self
            .send(Method::POST, "/api/auth/sign-up/email", None, Some(body))
            .await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        let user_id = reply["user"]["id"].as_str().unwrap().to_owned();
        let role = roles::ActiveModel {
            name: Set(format!("Role of {email}")),
            is_owner: Set(false),
            created_at: Set(db::now()),
            updated_at: Set(db::now()),
            ..Default::default()
        }
        .insert(&self.state.db)
        .await
        .unwrap();
        for permission in permissions {
            role_permissions::ActiveModel {
                role_id: Set(role.id),
                permission: Set((*permission).into()),
            }
            .insert(&self.state.db)
            .await
            .unwrap();
        }
        user_roles::ActiveModel {
            user_id: Set(user_id.clone()),
            role_id: Set(role.id),
            all_libraries: Set(true),
        }
        .insert(&self.state.db)
        .await
        .unwrap();
        (user_id, cookie.unwrap(), role.id)
    }
}

const PATH: &str = "/api/v1/root-folders";

fn add(path: &std::path::Path) -> Value {
    json!({ "path": path.to_str().unwrap() })
}

async fn create(app: &App, path: &std::path::Path) -> i64 {
    let (status, reply) = app.owner(Method::POST, PATH, Some(add(path))).await;
    assert_eq!(status, StatusCode::CREATED, "{reply}");
    reply["id"].as_i64().unwrap()
}

#[tokio::test]
async fn adding_a_path_that_does_not_exist_is_rejected() {
    let app = App::new().await;
    let dir = tempfile::tempdir().unwrap();
    let (status, reply) = app
        .owner(Method::POST, PATH, Some(add(&dir.path().join("missing"))))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
    assert_eq!(reply["errors"][0]["field"], "path");
    assert_eq!(reply["errors"][0]["message"], "does not exist");
}

#[tokio::test]
async fn a_file_a_relative_path_and_dot_dot_are_rejected() {
    let app = App::new().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("movie.mkv");
    std::fs::write(&file, b"x").unwrap();
    let up = format!("{}/..", dir.path().to_str().unwrap());
    for (path, message) in [
        (file.to_str().unwrap(), "is not a directory"),
        ("movies", "must be an absolute path"),
        ("", "must be an absolute path"),
        (up.as_str(), "must not contain `..`"),
    ] {
        let (status, reply) = app
            .owner(Method::POST, PATH, Some(json!({ "path": path })))
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{path}: {reply}");
        assert_eq!(reply["errors"][0]["message"], message, "{path}");
    }
}

#[tokio::test]
async fn a_nested_folder_and_a_duplicate_are_rejected() {
    let app = App::new().await;
    let dir = tempfile::tempdir().unwrap();
    let movies = dir.path().join("movies");
    let inner = movies.join("4k");
    std::fs::create_dir_all(&inner).unwrap();
    let sibling = dir.path().join("movies2");
    std::fs::create_dir(&sibling).unwrap();
    create(&app, &movies).await;

    // Inside an existing root folder.
    let (status, reply) = app.owner(Method::POST, PATH, Some(add(&inner))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
    assert!(
        reply["errors"][0]["message"]
            .as_str()
            .unwrap()
            .starts_with("is inside")
    );
    // Around an existing root folder.
    let (status, reply) = app.owner(Method::POST, PATH, Some(add(dir.path()))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
    assert!(
        reply["errors"][0]["message"]
            .as_str()
            .unwrap()
            .starts_with("contains")
    );
    // The same folder, with a trailing separator.
    let again = format!("{}/", movies.to_str().unwrap());
    let (status, reply) = app
        .owner(Method::POST, PATH, Some(json!({ "path": again })))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{reply}");
    assert_eq!(reply["code"], "path_taken");
    // A name that only starts like an existing folder is fine.
    create(&app, &sibling).await;
}

#[tokio::test]
async fn the_list_shows_free_space_and_unmapped_folders() {
    let app = App::new().await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("Dune (2021)")).unwrap();
    std::fs::create_dir(dir.path().join("Stray")).unwrap();
    std::fs::write(dir.path().join("notes.txt"), b"x").unwrap();
    let id = create(&app, dir.path()).await;
    // The write probe leaves nothing behind.
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3);
    add_movie(&app, id as i32, "Dune (2021)").await;

    let (status, reply) = app.owner(Method::GET, PATH, None).await;
    assert_eq!(status, StatusCode::OK);
    let folder = &reply[0];
    assert_eq!(folder["id"], id);
    assert_eq!(folder["accessible"], true);
    assert!(folder["freeSpace"].as_u64().unwrap() > 0);
    assert_eq!(folder["unmappedFolders"], json!(["Stray"]));

    // A folder that vanished is listed as inaccessible, without free space.
    drop(dir);
    let (_, reply) = app.owner(Method::GET, PATH, None).await;
    assert_eq!(reply[0]["accessible"], false);
    assert_eq!(reply[0]["freeSpace"], Value::Null);
    assert_eq!(reply[0]["unmappedFolders"], json!([]));
}

async fn add_movie(app: &App, root: i32, folder: &str) {
    movies::ActiveModel {
        tmdb_id: Set(1),
        title: Set("Movie".into()),
        sort_title: Set("movie".into()),
        year: Set(2020),
        status: Set("released".into()),
        root_folder_id: Set(root),
        folder_name: Set(folder.into()),
        monitored: Set(true),
        minimum_availability: Set("released".into()),
        added_at: Set(db::now()),
        updated_at: Set(db::now()),
        ..Default::default()
    }
    .insert(&app.state.db)
    .await
    .unwrap();
}

#[tokio::test]
async fn a_root_folder_with_movies_cannot_be_deleted() {
    let app = App::new().await;
    let dir = tempfile::tempdir().unwrap();
    let id = create(&app, dir.path()).await;
    add_movie(&app, id as i32, "Movie (2020)").await;

    let (status, reply) = app
        .owner(Method::DELETE, &format!("{PATH}/{id}"), None)
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{reply}");
    assert_eq!(reply["code"], "root_folder_in_use");
    assert!(
        root_folders::Entity::find_by_id(id as i32)
            .one(&app.state.db)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn delete_removes_the_row_and_keeps_the_folder_on_disk() {
    let app = App::new().await;
    let dir = tempfile::tempdir().unwrap();
    let id = create(&app, dir.path()).await;
    let path = format!("{PATH}/{id}");

    let (status, _) = app.owner(Method::DELETE, &path, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(dir.path().is_dir());
    let (status, _) = app.owner(Method::DELETE, &path, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, reply) = app.owner(Method::GET, PATH, None).await;
    assert_eq!(reply, json!([]));
}

#[tokio::test]
async fn create_and_delete_are_audited() {
    let app = App::new().await;
    let dir = tempfile::tempdir().unwrap();
    let id = create(&app, dir.path()).await;
    app.owner(Method::DELETE, &format!("{PATH}/{id}"), None)
        .await;

    let (status, reply) = app
        .owner(
            Method::GET,
            "/api/v1/audit-log?targetKind=root_folder",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    let mut actions: Vec<_> = reply["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["action"].as_str().unwrap())
        .collect();
    actions.sort_unstable();
    assert_eq!(actions, ["root_folder.created", "root_folder.deleted"]);
}

#[tokio::test]
async fn the_browser_lists_sub_folders_and_the_parent() {
    let app = App::new().await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("b")).unwrap();
    std::fs::create_dir(dir.path().join("A")).unwrap();
    std::fs::write(dir.path().join("file.txt"), b"x").unwrap();
    let path = dir.path().to_str().unwrap();

    let (status, reply) = app
        .owner(
            Method::GET,
            &format!("/api/v1/filesystem?path={path}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    let names: Vec<_> = reply["folders"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["A", "b"]);
    assert_eq!(
        reply["parent"],
        dir.path().parent().unwrap().to_str().unwrap()
    );

    let (status, reply) = app.owner(Method::GET, "/api/v1/filesystem", None).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert!(reply["parent"].is_null() || reply["parent"] == "");
    let missing = dir.path().join("missing");
    let (status, _) = app
        .owner(
            Method::GET,
            &format!("/api/v1/filesystem?path={}", missing.to_str().unwrap()),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = app
        .owner(
            Method::GET,
            &format!("/api/v1/filesystem?path={path}/file.txt"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn read_and_manage_are_separate_permissions() {
    let app = App::new().await;
    let dir = tempfile::tempdir().unwrap();
    let (_, reader, _) = app.member("read@example.com", &["settings:read"]).await;
    let (_, nobody, _) = app.member("none@example.com", &[]).await;
    let (_, manager, _) = app.member("manage@example.com", &["settings:manage"]).await;

    let (status, _, _) = app.send(Method::GET, PATH, Some(&reader), None).await;
    assert_eq!(status, StatusCode::OK);
    for (method, path, body) in [
        (Method::POST, PATH, Some(add(dir.path()))),
        (Method::DELETE, "/api/v1/root-folders/1", None),
        (Method::GET, "/api/v1/filesystem", None),
    ] {
        let (status, _, _) = app.send(method.clone(), path, Some(&reader), body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}");
    }
    let (status, _, _) = app.send(Method::GET, PATH, Some(&nobody), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _, _) = app
        .send(Method::GET, "/api/v1/filesystem", Some(&manager), None)
        .await;
    assert_eq!(status, StatusCode::OK);
}
