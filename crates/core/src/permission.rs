//! Permission catalog and effective-permission checks.
//!
//! The catalog is fixed in code and mirrors `docs/15-permissions.md`. A test
//! compares the two, so a variant cannot be added without its doc line.

use std::collections::BTreeSet;
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

permissions! {
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

/// What one user may do: the union of their roles and direct grants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectivePermissions {
    is_owner: bool,
    permissions: BTreeSet<Permission>,
}

impl EffectivePermissions {
    /// Builds the effective set from what the database stores for a user:
    /// whether they hold the Owner role, and the permission strings of all
    /// their roles and direct grants, in any order and with duplicates.
    ///
    /// An Owner gets every permission in the catalog. Strings that are not
    /// in the catalog are ignored.
    pub fn new<I>(is_owner: bool, permissions: I) -> Self
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        let permissions = if is_owner {
            Permission::ALL.iter().copied().collect()
        } else {
            permissions
                .into_iter()
                .filter_map(|permission| permission.as_ref().parse().ok())
                .collect()
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

    /// Whether the user holds `permission`.
    #[must_use]
    pub fn has(&self, permission: Permission) -> bool {
        self.permissions.contains(&permission)
    }

    /// The held permissions, in catalog order.
    pub fn iter(&self) -> impl Iterator<Item = Permission> + '_ {
        self.permissions.iter().copied()
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
