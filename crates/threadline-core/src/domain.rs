use std::error::Error;
use std::fmt::{self, Display};
use std::num::NonZeroU64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvalidIdentity {
    Zero,
    InvalidHost,
    InvalidRepositoryName,
}

impl Display for InvalidIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Zero => "Identity must be nonzero",
            Self::InvalidHost => "Invalid GitHub host name",
            Self::InvalidRepositoryName => "Expected a GitHub owner and repository name",
        })
    }
}

impl Error for InvalidIdentity {}

macro_rules! numeric_identity {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
        pub struct $name(NonZeroU64);

        impl $name {
            pub const fn new(value: NonZeroU64) -> Self {
                Self(value)
            }

            pub const fn get(self) -> u64 {
                self.0.get()
            }
        }

        impl TryFrom<u64> for $name {
            type Error = InvalidIdentity;

            fn try_from(value: u64) -> Result<Self, Self::Error> {
                NonZeroU64::new(value)
                    .map(Self)
                    .ok_or(InvalidIdentity::Zero)
            }
        }
    };
}

numeric_identity!(AccountProfileId);
numeric_identity!(RepositoryApiId);
numeric_identity!(ItemNumber);
numeric_identity!(IssueApiId);
numeric_identity!(TabInstanceId);

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct HostName(String);

impl HostName {
    pub fn new(name: &str) -> Result<Self, InvalidIdentity> {
        if name.len() > 253 || !name.split('.').all(valid_dns_label) {
            return Err(InvalidIdentity::InvalidHost);
        }
        Ok(Self(name.to_ascii_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn valid_dns_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 63
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct RepositoryKey {
    pub host: HostName,
    pub profile: AccountProfileId,
    pub id: RepositoryApiId,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ItemKey {
    pub repository: RepositoryKey,
    pub number: ItemNumber,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ItemKind {
    Issue,
    PullRequest,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryName {
    owner: String,
    name: String,
}

impl RepositoryName {
    pub fn new(owner: &str, name: &str) -> Result<Self, InvalidIdentity> {
        let valid_owner = valid_dns_label(owner);
        let valid_name = !name.is_empty()
            && !matches!(name, "." | "..")
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
        if !valid_owner || !valid_name {
            return Err(InvalidIdentity::InvalidRepositoryName);
        }
        Ok(Self {
            owner: owner.to_owned(),
            name: name.to_owned(),
        })
    }

    pub fn owner(&self) -> &str {
        &self.owner
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Revisions are compared for equality. A timestamp is a fallback, not a total order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Revision {
    EntityTag(String),
    UpdatedAt(String),
    Synthetic(u64),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ViewSelection {
    pub tab: TabInstanceId,
    pub generation: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_number_is_scoped_to_repository_and_account() {
        let key = ItemKey {
            repository: RepositoryKey {
                host: HostName::new("GitHub.COM").unwrap(),
                profile: 1.try_into().unwrap(),
                id: 8.try_into().unwrap(),
            },
            number: 42.try_into().unwrap(),
        };
        assert_eq!(key.repository.host, HostName::new("github.com").unwrap());
        let mut other = key.clone();
        other.repository.id = 9.try_into().unwrap();
        assert_ne!(key, other);
        other.repository.id = key.repository.id;
        other.repository.profile = 2.try_into().unwrap();
        assert_ne!(key, other);
        other.repository.profile = key.repository.profile;
        other.repository.host = HostName::new("github.example").unwrap();
        assert_ne!(key, other);
        assert_eq!(ItemNumber::try_from(0), Err(InvalidIdentity::Zero));
    }

    #[test]
    fn remote_route_components_cannot_become_paths_or_authorities() {
        for invalid in [
            "",
            "https://github.com",
            ".github.com",
            "a..com",
            "github.com@host",
        ] {
            assert_eq!(HostName::new(invalid), Err(InvalidIdentity::InvalidHost));
        }
        for invalid in ["", "..", "repo/other", "%2e%2e", "repo?query", "repo#part"] {
            assert_eq!(
                RepositoryName::new("owner", invalid),
                Err(InvalidIdentity::InvalidRepositoryName)
            );
        }
        assert!(RepositoryName::new("octo-team", "native.client_2").is_ok());
    }
}
