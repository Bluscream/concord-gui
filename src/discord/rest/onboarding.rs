//! Server onboarding: the questions a new member answers before they can talk.
//!
//! Both halves are here - reading what a server asks, and answering it. The
//! answering half is the point: until now the client detected onboarding as a
//! participation restriction and told people to go and finish it in the
//! official app, which is the one thing this client exists not to say.

use serde::Deserialize;
use serde_json::json;

use crate::Result;
use crate::discord::ids::{
    Id,
    marker::{ChannelMarker, GuildMarker, RoleMarker},
};

use super::DiscordRest;

/// One answer someone can pick.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OnboardingOption {
    pub id: u64,
    pub title: String,
    pub description: Option<String>,
    /// What picking it grants. Shown because onboarding is the one place a
    /// client hands out roles on your behalf, and doing that silently would be
    /// the wrong shape.
    pub role_ids: Vec<Id<RoleMarker>>,
    pub channel_ids: Vec<Id<ChannelMarker>>,
}

impl OnboardingOption {
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if let Some(description) = &self.description
            && !description.is_empty()
        {
            parts.push(description.clone());
        }
        if !self.role_ids.is_empty() {
            parts.push(match self.role_ids.len() {
                1 => "gives you 1 role".to_owned(),
                count => format!("gives you {count} roles"),
            });
        }
        if !self.channel_ids.is_empty() {
            parts.push(match self.channel_ids.len() {
                1 => "shows 1 channel".to_owned(),
                count => format!("shows {count} channels"),
            });
        }
        if parts.is_empty() {
            // A choice that does nothing is still a choice Discord requires an
            // answer to, and a blank line reads as a failure to load.
            parts.push("no roles or channels".to_owned());
        }
        parts.join(" - ")
    }
}

/// One question.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OnboardingQuestion {
    pub id: u64,
    pub title: String,
    /// Whether more than one answer may be picked.
    pub single_select: bool,
    /// Whether an answer is required to finish.
    pub required: bool,
    pub options: Vec<OnboardingOption>,
}

impl OnboardingQuestion {
    /// How the question reads under its title.
    pub fn summary(&self) -> String {
        let mut parts = vec![if self.single_select {
            "pick one".to_owned()
        } else {
            "pick any".to_owned()
        }];
        if self.required {
            parts.push("required".to_owned());
        }
        parts.join(" - ")
    }

    /// Whether the answers given satisfy this question.
    pub fn is_answered_by(&self, picked: &[u64]) -> bool {
        if !self.required {
            return true;
        }
        self.options
            .iter()
            .any(|option| picked.contains(&option.id))
    }
}

/// A server's onboarding as it stands.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Onboarding {
    pub enabled: bool,
    pub questions: Vec<OnboardingQuestion>,
}

impl Onboarding {
    /// Which required questions still have no answer.
    ///
    /// Named rather than counted: Discord rejects an incomplete submission
    /// with a message that does not say which question is missing.
    pub fn unanswered(&self, picked: &[u64]) -> Vec<&str> {
        self.questions
            .iter()
            .filter(|question| !question.is_answered_by(picked))
            .map(|question| question.title.as_str())
            .collect()
    }
}

#[derive(Deserialize)]
struct OnboardingBody {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    prompts: Vec<PromptBody>,
}

#[derive(Deserialize)]
struct PromptBody {
    id: Option<String>,
    title: Option<String>,
    #[serde(default)]
    single_select: bool,
    #[serde(default)]
    required: bool,
    #[serde(default)]
    options: Vec<OptionBody>,
}

#[derive(Deserialize)]
struct OptionBody {
    id: Option<String>,
    title: Option<String>,
    description: Option<String>,
    #[serde(default)]
    role_ids: Vec<String>,
    #[serde(default)]
    channel_ids: Vec<String>,
}

fn parse_ids<T>(raw: Vec<String>) -> Vec<Id<T>> {
    raw.into_iter()
        .filter_map(|id| id.parse::<u64>().ok())
        .map(Id::new)
        .collect()
}

impl DiscordRest {
    /// What a server asks new members.
    pub async fn onboarding(&self, guild_id: Id<GuildMarker>) -> Result<Onboarding> {
        let body: OnboardingBody = self
            .send_json(
                self.raw_http.get(format!(
                    "https://discord.com/api/v9/guilds/{}/onboarding",
                    guild_id.get()
                )),
                "onboarding",
            )
            .await?;

        Ok(Onboarding {
            enabled: body.enabled,
            questions: body
                .prompts
                .into_iter()
                .filter_map(|prompt| {
                    // Without an id there is nothing to answer against.
                    let id = prompt.id?.parse::<u64>().ok()?;
                    Some(OnboardingQuestion {
                        id,
                        title: prompt.title.unwrap_or_default(),
                        single_select: prompt.single_select,
                        required: prompt.required,
                        options: prompt
                            .options
                            .into_iter()
                            .filter_map(|option| {
                                Some(OnboardingOption {
                                    id: option.id?.parse::<u64>().ok()?,
                                    title: option.title.unwrap_or_default(),
                                    description: option.description.filter(|text| !text.is_empty()),
                                    role_ids: parse_ids(option.role_ids),
                                    channel_ids: parse_ids(option.channel_ids),
                                })
                            })
                            .collect(),
                    })
                })
                .collect(),
        })
    }

    /// Answer the questions and finish onboarding.
    ///
    /// `prompts_seen` and `responses_seen` are Discord's own bookkeeping: it
    /// wants to know which questions were shown, not only which were answered,
    /// and refuses the submission without them.
    pub async fn submit_onboarding(
        &self,
        guild_id: Id<GuildMarker>,
        onboarding: &Onboarding,
        picked: &[u64],
    ) -> Result<()> {
        let seen: serde_json::Map<String, serde_json::Value> = onboarding
            .questions
            .iter()
            .map(|question| (question.id.to_string(), json!(true)))
            .collect();
        let options_seen: serde_json::Map<String, serde_json::Value> = onboarding
            .questions
            .iter()
            .flat_map(|question| question.options.iter())
            .map(|option| (option.id.to_string(), json!(true)))
            .collect();

        self.send_unit(
            self.raw_http
                .post(format!(
                    "https://discord.com/api/v9/guilds/{}/onboarding-responses",
                    guild_id.get()
                ))
                .json(&json!({
                    "onboarding_responses": picked
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>(),
                    "onboarding_prompts_seen": seen,
                    "onboarding_responses_seen": options_seen,
                })),
            "onboarding responses",
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn question(required: bool, option_ids: &[u64]) -> OnboardingQuestion {
        OnboardingQuestion {
            id: 1,
            title: "What brings you here?".to_owned(),
            single_select: true,
            required,
            options: option_ids
                .iter()
                .map(|id| OnboardingOption {
                    id: *id,
                    title: format!("Option {id}"),
                    description: None,
                    role_ids: Vec::new(),
                    channel_ids: Vec::new(),
                })
                .collect(),
        }
    }

    #[test]
    fn an_optional_question_needs_no_answer() {
        // Discord allows them, and treating one as required would block a
        // submission that is actually complete.
        assert!(question(false, &[10, 11]).is_answered_by(&[]));
    }

    #[test]
    fn a_required_question_needs_one_of_its_own_options() {
        let question = question(true, &[10, 11]);

        assert!(!question.is_answered_by(&[]));
        assert!(question.is_answered_by(&[11]));
        // An id from another question does not answer this one.
        assert!(!question.is_answered_by(&[99]));
    }

    #[test]
    fn the_unanswered_questions_are_named_not_counted() {
        // Discord rejects an incomplete submission with a message that does
        // not say which question is missing, so the client has to.
        let onboarding = Onboarding {
            enabled: true,
            questions: vec![
                question(true, &[10]),
                OnboardingQuestion {
                    id: 2,
                    title: "Which topics?".to_owned(),
                    ..question(true, &[20])
                },
            ],
        };

        assert_eq!(onboarding.unanswered(&[]).len(), 2);
        assert_eq!(onboarding.unanswered(&[10]), vec!["Which topics?"]);
        assert!(onboarding.unanswered(&[10, 20]).is_empty());
    }

    #[test]
    fn an_option_says_what_it_grants() {
        // Onboarding is the one place a client hands out roles on your behalf,
        // and doing that silently would be the wrong shape.
        let option = OnboardingOption {
            id: 1,
            title: "Gamer".to_owned(),
            description: None,
            role_ids: vec![Id::new(7)],
            channel_ids: vec![Id::new(8), Id::new(9)],
        };
        let summary = option.summary();

        assert!(summary.contains("1 role"));
        assert!(summary.contains("2 channels"));
    }

    #[test]
    fn an_option_that_grants_nothing_says_so_rather_than_nothing() {
        // Discord allows it, and a blank line reads as a failure to load.
        let option = OnboardingOption {
            id: 1,
            title: "Just looking".to_owned(),
            description: None,
            role_ids: Vec::new(),
            channel_ids: Vec::new(),
        };
        assert!(option.summary().contains("no roles or channels"));
    }

    #[test]
    fn a_question_says_whether_one_answer_or_several() {
        // Picking a second answer on a single-select question replaces the
        // first, which is surprising unless the row said so.
        assert!(question(true, &[10]).summary().contains("pick one"));
        assert!(
            OnboardingQuestion {
                single_select: false,
                ..question(false, &[10])
            }
            .summary()
            .contains("pick any")
        );
    }
}

/// One line of the onboarding form.
///
/// Questions and answers in one list because that is how the form reads, but
/// each row says which it is - a header is not selectable, and an index that
/// did not distinguish them would let enter "pick" a question title.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OnboardingRow {
    Question {
        title: String,
        summary: String,
        /// Whether it still needs an answer, so the form can say which one is
        /// holding up the submission.
        unanswered: bool,
    },
    Option {
        id: u64,
        title: String,
        summary: String,
        picked: bool,
    },
}

impl OnboardingRow {
    /// The option this row picks, if it is an option at all.
    pub const fn option_id(&self) -> Option<u64> {
        match self {
            Self::Option { id, .. } => Some(*id),
            Self::Question { .. } => None,
        }
    }
}

impl Onboarding {
    /// The form as a flat list of rows.
    pub fn rows(&self, picked: &[u64]) -> Vec<OnboardingRow> {
        let mut rows = Vec::new();
        for question in &self.questions {
            rows.push(OnboardingRow::Question {
                title: question.title.clone(),
                summary: question.summary(),
                unanswered: !question.is_answered_by(picked),
            });
            for option in &question.options {
                rows.push(OnboardingRow::Option {
                    id: option.id,
                    title: option.title.clone(),
                    summary: option.summary(),
                    picked: picked.contains(&option.id),
                });
            }
        }
        rows
    }

    /// Pick or unpick an option, returning the new set of answers.
    ///
    /// A single-select question replaces its previous answer rather than
    /// adding to it, which is Discord's rule and is why this cannot be a
    /// simple toggle in each client.
    pub fn toggled(&self, picked: &[u64], option_id: u64) -> Vec<u64> {
        let Some(question) = self
            .questions
            .iter()
            .find(|question| question.options.iter().any(|option| option.id == option_id))
        else {
            return picked.to_vec();
        };

        if picked.contains(&option_id) {
            return picked
                .iter()
                .copied()
                .filter(|id| *id != option_id)
                .collect();
        }

        let mut next: Vec<u64> = if question.single_select {
            // Drop the other answers to this question, and only to this one.
            picked
                .iter()
                .copied()
                .filter(|id| !question.options.iter().any(|option| option.id == *id))
                .collect()
        } else {
            picked.to_vec()
        };
        next.push(option_id);
        next
    }
}

#[cfg(test)]
mod row_tests {
    use super::*;

    fn onboarding(single_select: bool) -> Onboarding {
        Onboarding {
            enabled: true,
            questions: vec![
                OnboardingQuestion {
                    id: 1,
                    title: "What brings you here?".to_owned(),
                    single_select,
                    required: true,
                    options: [10, 11]
                        .into_iter()
                        .map(|id| OnboardingOption {
                            id,
                            title: format!("Option {id}"),
                            description: None,
                            role_ids: Vec::new(),
                            channel_ids: Vec::new(),
                        })
                        .collect(),
                },
                OnboardingQuestion {
                    id: 2,
                    title: "Which topics?".to_owned(),
                    single_select: false,
                    required: false,
                    options: vec![OnboardingOption {
                        id: 20,
                        title: "Rust".to_owned(),
                        description: None,
                        role_ids: Vec::new(),
                        channel_ids: Vec::new(),
                    }],
                },
            ],
        }
    }

    #[test]
    fn a_question_row_is_not_an_option_row() {
        // An index that did not distinguish them would let enter "pick" a
        // question title, which answers nothing and looks like it worked.
        let rows = onboarding(true).rows(&[]);
        assert!(rows[0].option_id().is_none());
        assert_eq!(rows[1].option_id(), Some(10));
    }

    #[test]
    fn a_single_select_question_replaces_its_answer() {
        // Discord's rule. Adding to it instead would submit two answers to a
        // question that takes one, which it rejects.
        let onboarding = onboarding(true);
        let picked = onboarding.toggled(&[10], 11);
        assert_eq!(picked, vec![11]);
    }

    #[test]
    fn a_multi_select_question_keeps_both() {
        let onboarding = onboarding(false);
        let mut picked = onboarding.toggled(&[10], 11);
        picked.sort_unstable();
        assert_eq!(picked, vec![10, 11]);
    }

    #[test]
    fn replacing_one_answer_leaves_other_questions_alone() {
        // The filter must be scoped to the question being answered, or
        // answering one would silently clear the rest of the form.
        let onboarding = onboarding(true);
        let mut picked = onboarding.toggled(&[10, 20], 11);
        picked.sort_unstable();
        assert_eq!(picked, vec![11, 20]);
    }

    #[test]
    fn picking_the_same_option_again_unpicks_it() {
        let onboarding = onboarding(true);
        assert!(onboarding.toggled(&[10], 10).is_empty());
    }

    #[test]
    fn an_unknown_option_changes_nothing() {
        let onboarding = onboarding(true);
        assert_eq!(onboarding.toggled(&[10], 999), vec![10]);
    }

    #[test]
    fn a_question_row_says_when_it_still_needs_an_answer() {
        let onboarding = onboarding(true);
        let OnboardingRow::Question { unanswered, .. } = &onboarding.rows(&[])[0] else {
            panic!("first row should be a question");
        };
        assert!(*unanswered);

        let OnboardingRow::Question { unanswered, .. } = &onboarding.rows(&[10])[0] else {
            panic!("first row should be a question");
        };
        assert!(!*unanswered);
    }
}
