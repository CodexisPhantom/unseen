//! Permission catalog and effective-permission checks.
//!
//! The catalog is fixed in code and mirrors `docs/15-permissions.md`. A test
//! compares the two, so a variant cannot be added without its doc line.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

/// Declares the enum, `Permission::ALL` and `Permission::as_str` from one
/// list, so the three can never disagree.
macro_rules! permissions {
    ($($variant:ident => $name:literal,)+) => {
        /// One entry of the permission catalog, written `resource:action`.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum Permission {
            $(
                #[doc = concat!("`", $name, "`")]
                $variant,
            )+
        }

        impl Permission {
            /// Every permission in the catalog, in the order of the design doc.
            pub const ALL: &'static [Permission] = &[$(Permission::$variant),+];

            /// The `resource:action` form stored in the database.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Permission::$variant => $name,)+
                }
            }
        }
    };
}

/// Invokes `$callback!` with every `Variant => "resource:action"` entry of
/// the permission catalog, in the order of `docs/15-permissions.md`.
#[macro_export]
macro_rules! for_each_permission {
    ($callback:ident) => {
        $callback! {
            LibraryRead => "library:read",
            LibraryAdd => "library:add",
            LibraryEdit => "library:edit",
            LibraryDelete => "library:delete",
            SearchRun => "search:run",
            ReleaseGrab => "release:grab",
            QueueRead => "queue:read",
            QueueManage => "queue:manage",
            ImportRun => "import:run",
            SubtitlesManage => "subtitles:manage",
            IndexersRead => "indexers:read",
            IndexersManage => "indexers:manage",
            ClientsRead => "clients:read",
            ClientsManage => "clients:manage",
            ProfilesRead => "profiles:read",
            ProfilesManage => "profiles:manage",
            SettingsRead => "settings:read",
            SettingsManage => "settings:manage",
            UsersManage => "users:manage",
            RolesManage => "roles:manage",
            ApikeysManage => "apikeys:manage",
            SystemLogs => "system:logs",
            SystemBackup => "system:backup",
            SystemTasks => "system:tasks",
            SystemAudit => "system:audit",
        }
    };
}

for_each_permission!(permissions);

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A string that is not in the permission catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownPermission(pub String);

impl fmt::Display for UnknownPermission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown permission `{}`", self.0)
    }
}

impl std::error::Error for UnknownPermission {}

impl FromStr for Permission {
    type Err = UnknownPermission;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .iter()
            .copied()
            .find(|permission| permission.as_str() == s)
            .ok_or_else(|| UnknownPermission(s.to_owned()))
    }
}

/// Which libraries a permission grant applies to (ADR 0012).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryScope {
    /// Every library on the server, including ones created later.
    All,
    /// Only the listed library ids. An empty set grants no library access.
    Libraries(BTreeSet<i32>),
}

impl LibraryScope {
    /// Applies to every library.
    #[must_use]
    pub const fn all() -> Self {
        Self::All
    }

    /// Applies to no library.
    #[must_use]
    pub fn none() -> Self {
        Self::Libraries(BTreeSet::new())
    }

    /// Builds a scope from `all_libraries` and an optional set of library ids.
    #[must_use]
    pub fn from_parts(all_libraries: bool, library_ids: impl IntoIterator<Item = i32>) -> Self {
        if all_libraries {
            Self::All
        } else {
            Self::Libraries(library_ids.into_iter().collect())
        }
    }

    /// True when this scope covers every library.
    #[must_use]
    pub const fn is_all(&self) -> bool {
        matches!(self, Self::All)
    }

    /// True when `library_id` is within this scope.
    #[must_use]
    pub fn allows(&self, library_id: i32) -> bool {
        match self {
            Self::All => true,
            Self::Libraries(ids) => ids.contains(&library_id),
        }
    }

    /// The restricted set of library ids, or `None` when this scope covers all
    /// libraries.
    #[must_use]
    pub fn library_ids(&self) -> Option<&BTreeSet<i32>> {
        match self {
            Self::All => None,
            Self::Libraries(ids) => Some(ids),
        }
    }

    /// Widens `self` to include every library covered by `other`.
    pub fn union_with(&mut self, other: &Self) {
        match (&mut *self, other) {
            (Self::All, _) => {}
            (_, Self::All) => *self = Self::All,
            (Self::Libraries(ours), Self::Libraries(theirs)) => {
                ours.extend(theirs.iter().copied());
            }
        }
    }

    /// True when `self` covers every library covered by `other`.
    #[must_use]
    pub fn contains_scope(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::All, _) => true,
            (Self::Libraries(_), Self::All) => false,
            (Self::Libraries(ours), Self::Libraries(theirs)) => theirs.is_subset(ours),
        }
    }
}

/// What one user may do: the union of their roles and direct grants, with the
/// library scope of each permission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectivePermissions {
    is_owner: bool,
    permissions: BTreeMap<Permission, LibraryScope>,
}

impl EffectivePermissions {
    /// Builds the effective set from unscoped (all-library) permission strings.
    ///
    /// An Owner gets every permission in the catalog for all libraries. Strings
    /// that are not in the catalog are ignored.
    pub fn new<I>(is_owner: bool, permissions: I) -> Self
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        Self::from_scoped(
            is_owner,
            permissions
                .into_iter()
                .map(|permission| (permission, LibraryScope::All)),
        )
    }

    /// Builds the effective set from scoped grants. When a permission is
    /// granted by several roles or direct grants, its scope is the union of
    /// their scopes. An Owner gets every permission for all libraries.
    pub fn from_scoped<I, S>(is_owner: bool, grants: I) -> Self
    where
        I: IntoIterator<Item = (S, LibraryScope)>,
        S: AsRef<str>,
    {
        let permissions = if is_owner {
            Permission::ALL
                .iter()
                .copied()
                .map(|permission| (permission, LibraryScope::All))
                .collect()
        } else {
            let mut map: BTreeMap<Permission, LibraryScope> = BTreeMap::new();
            for (name, scope) in grants {
                let Ok(permission) = name.as_ref().parse::<Permission>() else {
                    continue;
                };
                map.entry(permission)
                    .and_modify(|existing| existing.union_with(&scope))
                    .or_insert(scope);
            }
            map
        };
        Self {
            is_owner,
            permissions,
        }
    }

    /// Whether the user holds the Owner role.
    #[must_use]
    pub fn is_owner(&self) -> bool {
        self.is_owner
    }

    /// Whether the user holds `permission` (for at least one scope, possibly
    /// restricted to specific libraries).
    #[must_use]
    pub fn has(&self, permission: Permission) -> bool {
        self.permissions.contains_key(&permission)
    }

    /// The library scope of `permission`, or `None` when the user does not hold
    /// it at all.
    #[must_use]
    pub fn scope(&self, permission: Permission) -> Option<&LibraryScope> {
        self.permissions.get(&permission)
    }

    /// Whether the user holds `permission` for `library_id`.
    #[must_use]
    pub fn allows_in_library(&self, permission: Permission, library_id: i32) -> bool {
        self.permissions
            .get(&permission)
            .is_some_and(|scope| scope.allows(library_id))
    }

    /// Whether the user holds `permission` across all libraries.
    #[must_use]
    pub fn has_all_libraries(&self, permission: Permission) -> bool {
        self.permissions
            .get(&permission)
            .is_some_and(LibraryScope::is_all)
    }

    /// The held permissions, in catalog order.
    pub fn iter(&self) -> impl Iterator<Item = Permission> + '_ {
        self.permissions.keys().copied()
    }

    /// The held permissions with their effective library scope, in catalog
    /// order.
    pub fn scoped_iter(&self) -> impl Iterator<Item = (Permission, &LibraryScope)> + '_ {
        self.permissions
            .iter()
            .map(|(permission, scope)| (*permission, scope))
    }
}

/// Whether `actor` may hand out or take away all of `permissions`, as a
/// direct grant, as the content of a role being edited, or as a role being
/// assigned or removed. The actor must hold every one of them.
///
/// This does not cover the Owner role itself: only an Owner may grant it,
/// which callers check with [`EffectivePermissions::is_owner`].
pub fn can_grant<I>(actor: &EffectivePermissions, permissions: I) -> bool
where
    I: IntoIterator<Item = Permission>,
{
    permissions
        .into_iter()
        .all(|permission| actor.has(permission))
}

/// Whether `actor` may hand out or take away `permission` with the given
/// `scope`. The actor must hold `permission` for every library in `scope`.
#[must_use]
pub fn can_grant_scoped(
    actor: &EffectivePermissions,
    permission: Permission,
    scope: &LibraryScope,
) -> bool {
    actor
        .scope(permission)
        .is_some_and(|actor_scope| actor_scope.contains_scope(scope))
}

/// Whether `actor` may change `target`'s account, roles or password as far
/// as the Owner rule goes: nobody but an Owner can change an Owner.
///
/// The route still requires `users:manage`, and each permission that changes
/// still goes through [`can_grant`].
#[must_use]
pub fn can_modify_user(actor: &EffectivePermissions, target: &EffectivePermissions) -> bool {
    actor.is_owner || !target.is_owner
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: [&str; 0] = [];

    #[test]
    fn display_and_parse_round_trip() {
        for &permission in Permission::ALL {
            assert_eq!(permission.to_string().parse(), Ok(permission));
        }
        assert_eq!(Permission::LibraryRead.to_string(), "library:read");
    }

    #[test]
    fn parse_rejects_unknown_strings() {
        let error = "library:fly".parse::<Permission>().unwrap_err();
        assert_eq!(error, UnknownPermission("library:fly".to_owned()));
        assert_eq!(error.to_string(), "unknown permission `library:fly`");
        assert!("Library:Read".parse::<Permission>().is_err());
        assert!("".parse::<Permission>().is_err());
    }

    #[test]
    fn effective_set_is_the_union_of_roles_and_grants() {
        let member = ["library:read", "queue:read"];
        let requester = ["library:read", "library:add"];
        let grants = ["queue:manage"];
        let user = EffectivePermissions::new(false, member.iter().chain(&requester).chain(&grants));

        assert!(!user.is_owner());
        assert_eq!(
            user.iter().collect::<Vec<_>>(),
            [
                Permission::LibraryRead,
                Permission::LibraryAdd,
                Permission::QueueRead,
                Permission::QueueManage,
            ]
        );
        assert!(!user.has(Permission::LibraryDelete));
    }

    #[test]
    fn builds_from_a_set_of_owned_strings() {
        let stored: BTreeSet<String> = ["search:run".to_owned()].into();
        let user = EffectivePermissions::new(false, &stored);
        assert!(user.has(Permission::SearchRun));
    }

    #[test]
    fn direct_grant_alone_gives_its_permission() {
        let user = EffectivePermissions::new(false, ["queue:manage"]);
        assert_eq!(user.iter().collect::<Vec<_>>(), [Permission::QueueManage]);
    }

    #[test]
    fn user_without_role_or_grant_has_nothing() {
        let user = EffectivePermissions::new(false, NONE);
        assert_eq!(user.iter().count(), 0);
    }

    #[test]
    fn owner_has_every_permission() {
        let owner = EffectivePermissions::new(true, NONE);
        assert!(owner.is_owner());
        assert!(owner.iter().eq(Permission::ALL.iter().copied()));
    }

    #[test]
    fn unknown_strings_are_ignored() {
        let user = EffectivePermissions::new(false, ["library:read", "requests:approve", "", "*"]);
        assert_eq!(user.iter().collect::<Vec<_>>(), [Permission::LibraryRead]);
    }

    #[test]
    fn can_grant_only_what_the_actor_holds() {
        let actor = EffectivePermissions::new(false, ["users:manage", "library:read"]);
        assert!(can_grant(&actor, [Permission::LibraryRead]));
        assert!(can_grant(&actor, []));
        assert!(!can_grant(
            &actor,
            [Permission::LibraryRead, Permission::LibraryDelete]
        ));

        let owner = EffectivePermissions::new(true, NONE);
        assert!(can_grant(&owner, Permission::ALL.iter().copied()));
        // A role is checked the same way, through the permissions it holds.
        assert!(can_grant(&owner, actor.iter()));
    }

    #[test]
    fn only_an_owner_can_modify_an_owner() {
        let owner = EffectivePermissions::new(true, NONE);
        // Every permission, as with the Admin template, is still not Owner.
        let admin = EffectivePermissions::new(false, Permission::ALL.iter().map(|p| p.as_str()));
        let member = EffectivePermissions::new(false, ["library:read"]);

        assert!(can_modify_user(&owner, &owner));
        assert!(can_modify_user(&owner, &admin));
        assert!(can_modify_user(&admin, &member));
        assert!(can_modify_user(&member, &admin));
        assert!(!can_modify_user(&admin, &owner));
        assert!(!can_modify_user(&member, &owner));
    }

    #[test]
    fn scoped_permissions_unite_and_enforce_escalation() {
        let kids = LibraryScope::Libraries([2].into());
        let movies = LibraryScope::Libraries([1].into());
        let both = LibraryScope::Libraries([1, 2].into());

        let user = EffectivePermissions::from_scoped(
            false,
            [
                ("library:read", kids.clone()),
                ("library:read", movies.clone()),
                ("library:add", kids.clone()),
                ("queue:read", LibraryScope::All),
                ("queue:read", kids.clone()),
                ("library:delete", LibraryScope::none()),
            ],
        );

        assert_eq!(user.scope(Permission::LibraryRead), Some(&both));
        assert!(user.allows_in_library(Permission::LibraryRead, 1));
        assert!(user.allows_in_library(Permission::LibraryRead, 2));
        assert!(!user.allows_in_library(Permission::LibraryRead, 3));
        assert!(!user.has_all_libraries(Permission::LibraryRead));

        assert!(user.allows_in_library(Permission::LibraryAdd, 2));
        assert!(!user.allows_in_library(Permission::LibraryAdd, 1));

        assert_eq!(user.scope(Permission::QueueRead), Some(&LibraryScope::All));
        assert!(user.has_all_libraries(Permission::QueueRead));
        assert!(user.allows_in_library(Permission::QueueRead, 999));

        // Empty scope holds the permission entry, but allows no library.
        assert!(user.has(Permission::LibraryDelete));
        assert!(!user.allows_in_library(Permission::LibraryDelete, 1));

        // Escalation: actor can only grant scopes within their own scope.
        assert!(can_grant_scoped(&user, Permission::LibraryRead, &kids));
        assert!(can_grant_scoped(&user, Permission::LibraryRead, &both));
        assert!(!can_grant_scoped(
            &user,
            Permission::LibraryRead,
            &LibraryScope::All
        ));
        assert!(!can_grant_scoped(
            &user,
            Permission::LibraryRead,
            &LibraryScope::Libraries([2, 3].into())
        ));
        assert!(can_grant_scoped(
            &user,
            Permission::QueueRead,
            &LibraryScope::All
        ));
        assert!(can_grant_scoped(&user, Permission::QueueRead, &kids));
        assert!(!can_grant_scoped(&user, Permission::LibraryEdit, &kids));

        // Owner holds every permission across all libraries.
        let owner = EffectivePermissions::from_scoped(true, Vec::<(&str, LibraryScope)>::new());
        assert!(owner.has_all_libraries(Permission::LibraryRead));
        assert!(can_grant_scoped(
            &owner,
            Permission::LibraryDelete,
            &LibraryScope::All
        ));
    }

    /// The first column of the table under "## Permission catalog".
    fn documented_catalog() -> Vec<String> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/15-permissions.md");
        let doc = std::fs::read_to_string(path).expect("docs/15-permissions.md is readable");
        doc.lines()
            .skip_while(|line| line.trim() != "## Permission catalog")
            .skip(1)
            .take_while(|line| !line.starts_with("## "))
            .filter_map(|line| line.strip_prefix("| `")?.split('`').next())
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn enum_matches_the_catalog_in_the_design_doc() {
        let in_code: Vec<String> = Permission::ALL.iter().map(ToString::to_string).collect();
        assert_eq!(
            in_code,
            documented_catalog(),
            "Permission enum (left) and docs/15-permissions.md catalog (right) differ"
        );
    }
}
