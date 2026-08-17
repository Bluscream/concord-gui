//! How a server lists itself in discovery.
//!
//! The owner-facing half, next to the browsing half in `discovery.rs`. Shapes
//! and limits are from Discord's documented discovery metadata object rather
//! than guessed: the caps in particular are enforced server-side, and a form
//! that let someone exceed one would fail at the last step with a message that
//! does not say which field was wrong.

use serde::Deserialize;
use serde_json::{Map, Value};

use crate::Result;
use crate::discord::ids::{Id, marker::GuildMarker};

use super::DiscordRest;

/// Discord's caps on discovery metadata.
pub const MAX_DISCOVERY_KEYWORDS: usize = 10;
pub const MAX_DISCOVERY_KEYWORD_CHARS: usize = 30;
/// Subcategories, not counting the primary one.
pub const MAX_DISCOVERY_SUBCATEGORIES: usize = 5;
pub const MAX_DISCOVERY_ABOUT_CHARS: usize = 2400;

/// One category a server can list itself under.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveryCategory {
    pub id: u32,
    pub name: String,
    /// Whether it may be the primary category. Not every category can be, and
    /// offering one that cannot would be a choice Discord then refuses.
    pub is_primary: bool,
}

/// A server's discovery settings.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DiscoveryMetadata {
    pub primary_category_id: Option<u32>,
    pub keywords: Vec<String>,
    /// Whether the server is offered as a source when someone finds one of its
    /// custom emoji elsewhere.
    pub emoji_discoverability_enabled: bool,
    pub about: Option<String>,
    pub category_ids: Vec<u32>,
}

/// Why the metadata cannot be saved yet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiscoveryMetadataProblem {
    TooManyKeywords,
    KeywordTooLong,
    TooManySubcategories,
    AboutTooLong,
}

impl DiscoveryMetadataProblem {
    pub fn message(self) -> String {
        match self {
            Self::TooManyKeywords => {
                format!("At most {MAX_DISCOVERY_KEYWORDS} keywords")
            }
            Self::KeywordTooLong => {
                format!("A keyword is at most {MAX_DISCOVERY_KEYWORD_CHARS} characters")
            }
            Self::TooManySubcategories => {
                format!("At most {MAX_DISCOVERY_SUBCATEGORIES} extra categories")
            }
            Self::AboutTooLong => {
                format!("The description is at most {MAX_DISCOVERY_ABOUT_CHARS} characters")
            }
        }
    }
}

impl DiscoveryMetadata {
    /// Why this cannot be saved, or `None` when it can.
    ///
    /// Checked here rather than left to Discord: it enforces all four, and its
    /// rejection does not say which field was wrong.
    pub fn problem(&self) -> Option<DiscoveryMetadataProblem> {
        if self.keywords.len() > MAX_DISCOVERY_KEYWORDS {
            return Some(DiscoveryMetadataProblem::TooManyKeywords);
        }
        if self
            .keywords
            .iter()
            .any(|keyword| keyword.chars().count() > MAX_DISCOVERY_KEYWORD_CHARS)
        {
            return Some(DiscoveryMetadataProblem::KeywordTooLong);
        }
        if self.category_ids.len() > MAX_DISCOVERY_SUBCATEGORIES {
            return Some(DiscoveryMetadataProblem::TooManySubcategories);
        }
        if self
            .about
            .as_ref()
            .is_some_and(|about| about.chars().count() > MAX_DISCOVERY_ABOUT_CHARS)
        {
            return Some(DiscoveryMetadataProblem::AboutTooLong);
        }
        None
    }

    /// How the panel reads it back.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        parts.push(match self.primary_category_id {
            Some(_) => "listed".to_owned(),
            // Without a primary category Discord will not show the server at
            // all, which is not obvious from the other fields being filled in.
            None => "no primary category - not listed".to_owned(),
        });
        parts.push(match self.keywords.len() {
            0 => "no keywords".to_owned(),
            1 => "1 keyword".to_owned(),
            count => format!("{count} keywords"),
        });
        if self.emoji_discoverability_enabled {
            parts.push("findable by its emoji".to_owned());
        }
        parts.join(" - ")
    }

    fn to_body(&self) -> Value {
        let mut fields = Map::new();
        // Every field is sent: this endpoint replaces rather than merges, so
        // an omitted one is reset to its default rather than left alone.
        fields.insert(
            "primary_category_id".to_owned(),
            self.primary_category_id.map_or(Value::Null, Value::from),
        );
        fields.insert(
            "keywords".to_owned(),
            Value::Array(
                self.keywords
                    .iter()
                    .take(MAX_DISCOVERY_KEYWORDS)
                    .map(|keyword| {
                        Value::from(
                            keyword
                                .chars()
                                .take(MAX_DISCOVERY_KEYWORD_CHARS)
                                .collect::<String>(),
                        )
                    })
                    .collect(),
            ),
        );
        fields.insert(
            "emoji_discoverability_enabled".to_owned(),
            Value::Bool(self.emoji_discoverability_enabled),
        );
        fields.insert(
            "about".to_owned(),
            self.about.as_ref().map_or(Value::Null, |about| {
                Value::from(
                    about
                        .chars()
                        .take(MAX_DISCOVERY_ABOUT_CHARS)
                        .collect::<String>(),
                )
            }),
        );
        fields.insert(
            "category_ids".to_owned(),
            Value::Array(
                self.category_ids
                    .iter()
                    .take(MAX_DISCOVERY_SUBCATEGORIES)
                    .map(|id| Value::from(*id))
                    .collect(),
            ),
        );
        Value::Object(fields)
    }
}

#[derive(Deserialize)]
struct MetadataBody {
    primary_category_id: Option<u32>,
    keywords: Option<Vec<String>>,
    #[serde(default)]
    emoji_discoverability_enabled: bool,
    about: Option<String>,
    #[serde(default)]
    category_ids: Vec<u32>,
}

#[derive(Deserialize)]
struct CategoryBody {
    id: Option<u32>,
    name: Option<String>,
    #[serde(default)]
    is_primary: bool,
}

impl DiscordRest {
    pub async fn discovery_metadata(&self, guild_id: Id<GuildMarker>) -> Result<DiscoveryMetadata> {
        let body: MetadataBody = self
            .send_json(
                self.raw_http.get(format!(
                    "https://discord.com/api/v9/guilds/{}/discovery-metadata",
                    guild_id.get()
                )),
                "discovery metadata",
            )
            .await?;

        Ok(DiscoveryMetadata {
            primary_category_id: body.primary_category_id,
            // Null and absent both mean none; the field is nullable.
            keywords: body.keywords.unwrap_or_default(),
            emoji_discoverability_enabled: body.emoji_discoverability_enabled,
            about: body.about.filter(|text| !text.is_empty()),
            category_ids: body.category_ids,
        })
    }

    pub async fn modify_discovery_metadata(
        &self,
        guild_id: Id<GuildMarker>,
        metadata: &DiscoveryMetadata,
    ) -> Result<()> {
        self.send_unit(
            self.raw_http
                .patch(format!(
                    "https://discord.com/api/v9/guilds/{}/discovery-metadata",
                    guild_id.get()
                ))
                .json(&metadata.to_body()),
            "discovery metadata",
        )
        .await
    }

    /// Every category a server can list itself under.
    pub async fn discovery_categories(&self) -> Result<Vec<DiscoveryCategory>> {
        let categories: Vec<CategoryBody> = self
            .send_json(
                self.raw_http
                    .get("https://discord.com/api/v9/discovery/categories"),
                "discovery categories",
            )
            .await?;

        Ok(categories
            .into_iter()
            .filter_map(|category| {
                // Without an id there is nothing to select.
                Some(DiscoveryCategory {
                    id: category.id?,
                    name: category.name.unwrap_or_default(),
                    is_primary: category.is_primary,
                })
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata() -> DiscoveryMetadata {
        DiscoveryMetadata {
            primary_category_id: Some(49),
            keywords: vec!["rust".to_owned()],
            emoji_discoverability_enabled: true,
            about: Some("We talk about Rust".to_owned()),
            category_ids: vec![48],
        }
    }

    #[test]
    fn a_complete_metadata_is_accepted() {
        assert_eq!(metadata().problem(), None);
    }

    #[test]
    fn every_documented_cap_is_refused_before_the_round_trip() {
        // Discord enforces all four and its rejection does not say which field
        // was wrong, so a form that relied on it would be unfixable by anyone
        // who did not already know the limits.
        let too_many = DiscoveryMetadata {
            keywords: vec!["a".to_owned(); MAX_DISCOVERY_KEYWORDS + 1],
            ..metadata()
        };
        assert_eq!(
            too_many.problem(),
            Some(DiscoveryMetadataProblem::TooManyKeywords)
        );

        let too_long = DiscoveryMetadata {
            keywords: vec!["a".repeat(MAX_DISCOVERY_KEYWORD_CHARS + 1)],
            ..metadata()
        };
        assert_eq!(
            too_long.problem(),
            Some(DiscoveryMetadataProblem::KeywordTooLong)
        );

        let too_many_categories = DiscoveryMetadata {
            category_ids: vec![1; MAX_DISCOVERY_SUBCATEGORIES + 1],
            ..metadata()
        };
        assert_eq!(
            too_many_categories.problem(),
            Some(DiscoveryMetadataProblem::TooManySubcategories)
        );

        let long_about = DiscoveryMetadata {
            about: Some("a".repeat(MAX_DISCOVERY_ABOUT_CHARS + 1)),
            ..metadata()
        };
        assert_eq!(
            long_about.problem(),
            Some(DiscoveryMetadataProblem::AboutTooLong)
        );
    }

    #[test]
    fn caps_are_counted_in_characters_not_bytes() {
        // A description of multi-byte characters would otherwise be refused
        // while being well within Discord's limit.
        let ok = DiscoveryMetadata {
            about: Some("é".repeat(MAX_DISCOVERY_ABOUT_CHARS)),
            ..metadata()
        };
        assert_eq!(ok.problem(), None);
    }

    #[test]
    fn every_field_is_sent_because_this_endpoint_replaces() {
        // Discord's own note: omitting a field sets it to default rather than
        // leaving it alone, so a partial body would quietly clear the rest.
        let body = DiscoveryMetadata::default().to_body();

        for field in [
            "primary_category_id",
            "keywords",
            "emoji_discoverability_enabled",
            "about",
            "category_ids",
        ] {
            assert!(body.get(field).is_some(), "{field} was omitted");
        }
    }

    #[test]
    fn no_primary_category_is_sent_as_null_and_says_the_server_is_not_listed() {
        // Without one Discord will not show the server at all, which the other
        // fields being filled in does not reveal.
        let unlisted = DiscoveryMetadata {
            primary_category_id: None,
            ..metadata()
        };

        assert_eq!(unlisted.to_body()["primary_category_id"], Value::Null);
        assert!(unlisted.summary().contains("not listed"));
        assert!(!metadata().summary().contains("not listed"));
    }
}
