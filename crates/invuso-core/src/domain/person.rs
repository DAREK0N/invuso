use std::fmt;

/// Identifier of a `Person` (idee.md 4.1).
///
/// Ordered, because its order is the stable tie-breaker wherever a rounding
/// cent or a settlement has to go to "the first" person (idee.md 8.4).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PersonId(pub String);

impl PersonId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PersonId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for PersonId {
    fn from(id: &str) -> Self {
        Self::new(id)
    }
}

/// Someone who can pay or owe (idee.md 4.1). Created globally, usable in
/// any number of groups.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Person {
    pub id: PersonId,
    pub name: String,
    /// Avatar color as a design-token name, e.g. `"cerulean"`.
    pub color: String,
    pub avatar_path: Option<String>,
    /// Exactly one person is the user ("Ich").
    pub is_me: bool,
    pub note: Option<String>,
}
