//! Entities of the app tables (docs/04-database.md). The auth tables are in
//! the generated `auth_schema`.

pub mod audit_log;
pub mod blocklist;
pub mod collections;
pub mod custom_formats;
pub mod download_client_tags;
pub mod download_clients;
pub mod download_episodes;
pub mod downloads;
pub mod episode_file_episodes;
pub mod episode_files;
pub mod episodes;
pub mod indexer_tags;
pub mod indexers;
pub mod language_profiles;
pub mod libraries;
pub mod movie_files;
pub mod movie_profiles;
pub mod movie_tags;
pub mod movies;
pub mod quality_definitions;
pub mod quality_profile_custom_formats;
pub mod quality_profiles;
pub mod role_permissions;
pub mod roles;
pub mod root_folders;
pub mod seasons;
pub mod shows;
pub mod subtitles;
pub mod tags;
pub mod user_permission_libraries;
pub mod user_permissions;
pub mod user_role_libraries;
pub mod user_roles;
