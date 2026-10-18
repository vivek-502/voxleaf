//! Opaque identifiers.
//!
//! The web app used Prisma `cuid()` for rows and `nanoid` elsewhere. Neither
//! format is part of any contract — ids only have to be unique, URL-safe and
//! unreadable as a hint of sequence — so one generator is enough here. These
//! are prefixed (`bk_`, `ch_`, …) purely to make logs and bug reports
//! self-describing when someone pastes one.

use nanoid::nanoid;

const ALPHABET: &[char] = &[
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9',
    'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm',
    'n', 'o', 'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z',
    'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M',
    'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z',
];

fn generate() -> String {
    nanoid!(21, ALPHABET)
}

/// A prefixed id as a plain string, for call sites that build rows dynamically.
pub fn new_id(prefix: &str) -> String {
    format!("{prefix}{}", generate())
}

macro_rules! id_type {
    ($name:ident, $prefix:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn new() -> Self {
                Self(format!("{}{}", $prefix, generate()))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_string())
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                String::deserialize(deserializer).map(Self)
            }
        }
    };
}

id_type!(BookId, "bk_");
id_type!(ChapterId, "ch_");
id_type!(LibraryId, "lb_");
id_type!(AudioId, "au_");
id_type!(BookmarkId, "bm_");
id_type!(EpubId, "ep_");

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ids_are_unique_and_prefixed() {
        let mut seen = HashSet::new();
        for _ in 0..200 {
            let id = ChapterId::new();
            assert!(id.as_str().starts_with("ch_"));
            assert_eq!(id.as_str().len(), 24);
            assert!(seen.insert(id.as_str().to_string()), "duplicate id generated");
        }
    }

    #[test]
    fn ids_round_trip_through_json() {
        let original = BookId::new();
        let json = serde_json::to_string(&original).unwrap();
        let parsed: BookId = serde_json::from_str(&json).unwrap();
        assert_eq!(original, parsed);
    }
}
