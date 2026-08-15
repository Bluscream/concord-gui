//! Interface translation.
//!
//! Lives in the core rather than in a front end so both clients translate from
//! the same catalogue: a string added for the GUI is available to the TUI, and
//! a translator has one place to work.
//!
//! Strings are [Fluent](https://projectfluent.org) files under `i18n/`, which
//! is what makes community translation possible - Weblate hosts that format
//! natively, with suggestions, review and voting. See `docs/TRANSLATING.md`.
//! Fluent was chosen over gettext because it handles plurals, gender and
//! per-language grammar without the source language having to anticipate them.
//!
//! The files are embedded at compile time. A translation platform edits the
//! files in the repository, so there is nothing to ship or load at runtime and
//! a broken install cannot leave the interface blank.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{OnceLock, RwLock};

use fluent_bundle::{FluentArgs, FluentResource, FluentValue};
use unic_langid::LanguageIdentifier;

/// Languages the interface is available in.
///
/// Serialised by tag rather than by variant name, so `config.toml` carries
/// `language = "de"` - a locale, which is what someone editing it by hand
/// would expect to write.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "en")]
    English,
    #[serde(rename = "de")]
    German,
}

/// The source language every other one falls back to.
const SOURCE: Language = Language::English;

impl Language {
    /// Parse a locale tag such as `de`, `de-DE` or `de_DE.UTF-8`.
    ///
    /// Only the primary subtag is considered: there is no separate Austrian
    /// or Swiss catalogue, and matching the whole tag would fail to find one.
    pub fn from_locale(tag: &str) -> Option<Self> {
        let primary = tag
            .split(['-', '_', '.'])
            .next()
            .unwrap_or(tag)
            .to_ascii_lowercase();

        Self::ALL
            .iter()
            .copied()
            .find(|language| language.tag() == primary)
    }

    pub const fn tag(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::German => "de",
        }
    }

    /// Its name in itself, which is how language pickers should list it.
    pub const fn endonym(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::German => "Deutsch",
        }
    }

    /// The Fluent source for this language.
    ///
    /// Adding a language means adding a variant, a file, and a line here.
    const fn source(self) -> &'static str {
        match self {
            Self::English => include_str!("../i18n/en.ftl"),
            Self::German => include_str!("../i18n/de.ftl"),
        }
    }

    pub const ALL: &'static [Self] = &[Self::English, Self::German];

    const fn index(self) -> u8 {
        match self {
            Self::English => 0,
            Self::German => 1,
        }
    }

    const fn from_index(index: u8) -> Self {
        match index {
            1 => Self::German,
            _ => Self::English,
        }
    }
}

/// The active language.
///
/// Global because every render path needs it, and threading a locale through
/// every view signature would touch all of both front ends for no benefit.
static ACTIVE: AtomicU8 = AtomicU8::new(0);

pub fn set_language(language: Language) {
    ACTIVE.store(language.index(), Ordering::Relaxed);
}

pub fn language() -> Language {
    Language::from_index(ACTIVE.load(Ordering::Relaxed))
}

/// Pick a language from the system locale, falling back to the source.
pub fn language_from_system() -> Language {
    sys_locale::get_locale()
        .as_deref()
        .and_then(Language::from_locale)
        .unwrap_or_default()
}

// The concurrent bundle rather than the default one: bundles are shared
// across threads here, and the default carries a RefCell that is not Sync.
type Bundle = fluent_bundle::concurrent::FluentBundle<FluentResource>;

/// Parsed bundles, built once per language on first use.
fn bundles() -> &'static RwLock<HashMap<u8, &'static Bundle>> {
    static BUNDLES: OnceLock<RwLock<HashMap<u8, &'static Bundle>>> = OnceLock::new();
    BUNDLES.get_or_init(|| RwLock::new(HashMap::new()))
}

fn bundle(language: Language) -> &'static Bundle {
    if let Some(bundle) = bundles()
        .read()
        .ok()
        .and_then(|cache| cache.get(&language.index()).copied())
    {
        return bundle;
    }

    let identifier: LanguageIdentifier = language
        .tag()
        .parse()
        .unwrap_or_else(|_| "en".parse().expect("en is a valid language tag"));

    let mut built = Bundle::new_concurrent(vec![identifier]);
    // Fluent isolates interpolated values with directional marks by default,
    // which is correct for bidirectional text but shows up as stray characters
    // in a terminal that does not handle them.
    built.set_use_isolating(false);

    // A syntax error yields a partial resource rather than nothing, so the
    // strings that did parse still work. The test below catches the error.
    let resource = FluentResource::try_new(language.source().to_owned())
        .unwrap_or_else(|(resource, _)| resource);
    let _ = built.add_resource(resource);

    // Leaked deliberately: bundles live for the process, and this keeps the
    // lookup free of locks and lifetimes on a path every label takes.
    let leaked: &'static Bundle = Box::leak(Box::new(built));
    if let Ok(mut cache) = bundles().write() {
        cache.insert(language.index(), leaked);
    }
    leaked
}

/// Translate a key into the active language.
///
/// An unknown key returns itself. That is deliberate: it makes the mistake
/// visible in the interface rather than showing an empty label, and the keys
/// are written to be readable if one ever escapes.
pub fn translate(key: &str) -> String {
    translate_args(key, None)
}

/// Translate a key, substituting named arguments.
///
/// Counts belong here rather than being formatted into the string by the
/// caller: only the translation knows how its language inflects around them,
/// which is the whole reason for choosing Fluent.
pub fn translate_args(key: &str, args: Option<&FluentArgs<'_>>) -> String {
    if let Some(text) = lookup(language(), key, args) {
        return text;
    }
    // A language with a missing key falls back to the source rather than
    // showing the key, so a partial translation stays usable - which is the
    // normal state of a language being worked on, not an edge case.
    if language() != SOURCE
        && let Some(text) = lookup(SOURCE, key, args)
    {
        return text;
    }
    key.to_owned()
}

fn lookup(language: Language, key: &str, args: Option<&FluentArgs<'_>>) -> Option<String> {
    let bundle = bundle(language);
    let message = bundle.get_message(key)?;
    let pattern = message.value()?;

    // Formatting errors leave placeholders in the output rather than failing,
    // which is preferable to a blank label.
    let mut errors = Vec::new();
    let text = bundle.format_pattern(pattern, args, &mut errors);
    Some(text.into_owned())
}

/// Translate with text arguments.
///
/// Front ends use this rather than building Fluent types themselves, so
/// `fluent_bundle` stays an implementation detail of this module.
pub fn translate_text(key: &str, pairs: &[(&str, &str)]) -> String {
    let mut args = FluentArgs::new();
    for (name, value) in pairs {
        args.set(*name, FluentValue::from(*value));
    }
    translate_args(key, Some(&args))
}

/// Build arguments for [`translate_args`].
pub fn args<'a>(pairs: &[(&'a str, i64)]) -> FluentArgs<'a> {
    let mut built = FluentArgs::new();
    for (name, value) in pairs {
        built.set(*name, FluentValue::from(*value));
    }
    built
}

/// Shorthand for [`translate`].
#[macro_export]
macro_rules! t {
    ($key:expr) => {
        $crate::i18n::translate($key)
    };
    ($key:expr, $($name:expr => $value:expr),+ $(,)?) => {
        $crate::i18n::translate_args($key, Some(&$crate::i18n::args(&[$(($name, $value)),+])))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_tags_resolve_to_a_language() {
        assert_eq!(Language::from_locale("de"), Some(Language::German));
        assert_eq!(Language::from_locale("de-DE"), Some(Language::German));
        assert_eq!(Language::from_locale("de_AT.UTF-8"), Some(Language::German));
        assert_eq!(Language::from_locale("en-GB"), Some(Language::English));

        // An unsupported language is not an error - it falls back to English.
        assert_eq!(Language::from_locale("fr"), None);
        assert_eq!(Language::from_locale(""), None);
    }

    #[test]
    fn every_language_file_parses() {
        // A syntax error would otherwise surface as every string in that
        // language silently falling back to English, which looks like a
        // missing translation rather than a broken file.
        for language in Language::ALL {
            assert!(
                FluentResource::try_new(language.source().to_owned()).is_ok(),
                "{}.ftl has Fluent syntax errors",
                language.tag()
            );
        }
    }

    #[test]
    fn every_translated_key_exists_in_the_source() {
        // A key only a translation has is dead weight, and usually a typo that
        // leaves the English string showing with no obvious cause.
        let source = Language::English.source();
        for language in Language::ALL.iter().filter(|l| **l != Language::English) {
            for line in language.source().lines() {
                let Some((key, _)) = line.split_once(" = ") else {
                    continue;
                };
                let key = key.trim();
                if key.starts_with('#') || key.is_empty() {
                    continue;
                }
                assert!(
                    source.contains(&format!("{key} = ")),
                    "{}.ftl has key {key}, which the source does not",
                    language.tag()
                );
            }
        }
    }

    #[test]
    fn translations_resolve_in_each_language() {
        set_language(Language::German);
        assert_eq!(translate("presence-idle"), "Abwesend");
        set_language(Language::English);
        assert_eq!(translate("presence-idle"), "Idle");
    }

    #[test]
    fn an_unknown_key_returns_itself_rather_than_nothing() {
        // Visible in the interface beats a blank label: a missing string
        // should look like a bug, not like an empty control.
        assert_eq!(translate("does-not-exist"), "does-not-exist");
    }
}
