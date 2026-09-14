//! The role list an operator may put on an account or an invitation, checked
//! against the vocabulary this build actually has.
//!
//! ## Why a check here, when the store already accepts any text
//!
//! [`AccountsDb::set_roles`](super::AccountsDb::set_roles) and
//! [`AccountsDb::create_invite`](super::AccountsDb::create_invite) store what
//! they are given — any comma-free non-empty string up to the column's length.
//! That is deliberate and stays: the store records roles, it does not decide
//! what they mean, and a hub sending a role this build has never heard of must
//! not be silently "corrected" (see `op_editor_core::access`, which KEEPS an
//! unrecognised role precisely so the mismatch is visible).
//!
//! An operator typing a role into this product's own account list is a
//! different situation, and it is the one this module answers. Here the
//! vocabulary is not somebody else's: [`ProductRole`] is the product's own
//! seven roles, and an administrator choosing from them can simply be told
//! when they typed something that is not one. The cost of not telling them is
//! concrete — `RoleSet::from_wire` grants NOTHING to a role it does not
//! recognise (fail closed, correctly), so a single typo in an invitation
//! produces an account whose role is a word no route, no badge and no colour
//! will ever act on, and nothing in the product would ever say so.
//!
//! ## Why the canonical spelling is what gets stored
//!
//! [`ProductRole::from_wire`] folds case, separators and aliases, so `UX/UI`,
//! `ux-ui` and `ux ui` are one role. Storing what the operator typed would put
//! three spellings of one role in one column, and every reader that compared
//! role strings would then have to know all three. [`ProductRole::as_wire`] is
//! the one spelling this build writes, which is what the first-admin
//! bootstrap already does ([`FIRST_ADMIN_ROLE`](super::FIRST_ADMIN_ROLE)).

use op_editor_core::access::ProductRole;

/// A role this build does not have, as the caller wrote it.
///
/// Its own type rather than a `String` because both front doors need it to say
/// the same thing: the route renders it as a `400`, and `op admin invite`
/// prints it before writing anything. Carries the original spelling — the
/// caller has to be able to see the typo, and `from_wire`'s normalisation is
/// exactly what would hide it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownRole {
    /// The role exactly as the caller wrote it.
    pub raw: String,
}

impl std::fmt::Display for UnknownRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "`{}` is not a role this build has; the roles are: {}",
            self.raw,
            known_roles()
        )
    }
}

impl std::error::Error for UnknownRole {}

/// Every role this build has, in the operator's order, wire spellings, comma
/// separated — what a refusal has to print for the person to be able to fix
/// their input without going to look it up.
pub fn known_roles() -> String {
    ProductRole::ALL
        .iter()
        .map(|role| role.as_wire())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The canonical wire spellings of `roles`, or the first one this build does
/// not have.
///
/// Order is the caller's and repeats collapse, because the column is a tag
/// list: `["admin", "admin"]` is one grant, and storing it twice would make
/// the row read as if it said something it does not.
///
/// An empty list is accepted and means "no roles" — the account or the
/// invitation confers nothing beyond the view-only floor every verified
/// account has. Refusing it would make a guest invitation, which is a real
/// thing to hand out, impossible to express.
pub fn canonical_roles(roles: &[String]) -> Result<Vec<String>, UnknownRole> {
    let mut canonical: Vec<String> = Vec::new();
    for raw in roles {
        let role = ProductRole::from_wire(raw).map_err(|_| UnknownRole { raw: raw.clone() })?;
        let wire = role.as_wire().to_string();
        if !canonical.contains(&wire) {
            canonical.push(wire);
        }
    }
    Ok(canonical)
}

/// The role names in a comma-separated string, as a caller writes them on a
/// command line.
///
/// Blank entries are dropped rather than refused, so `--roles qa,` is one role
/// and not a typo the caller has to hunt for; `--roles ""` is no roles.
pub fn split_roles(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|role| !role.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
#[path = "accounts_roles_tests.rs"]
mod tests;
