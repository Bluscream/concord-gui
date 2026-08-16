//! A string that must not end up in a log.
//!
//! `AppCommand` derives `Debug`, so a bare `String` password field would be
//! printed in full by any `{:?}` of a command - today's code, or code written
//! later by someone who has no reason to suspect one variant carries a
//! credential. This makes that impossible rather than merely unlikely.

/// A credential. Prints as `[redacted]` however it is formatted.
#[derive(Clone, Eq, PartialEq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The real value. Named to be conspicuous at the call site: reaching for
    /// this should look deliberate in a review.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("[redacted]")
    }
}

impl std::fmt::Display for Secret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("[redacted]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neither_formatter_reveals_the_value() {
        // Both, because a Display impl that leaked would undo the Debug one -
        // and `{}` is the easier of the two to reach for by accident.
        let secret = Secret::new("hunter2");

        assert_eq!(format!("{secret:?}"), "[redacted]");
        assert_eq!(format!("{secret}"), "[redacted]");
        assert!(!format!("{secret:?} {secret}").contains("hunter2"));
    }

    #[test]
    fn a_command_carrying_one_does_not_print_it() {
        // The actual risk: `{:?}` on a whole command, which is what a debug
        // log or a test failure message does.
        let command = crate::discord::AppCommand::RevokeAuthSessions {
            id_hashes: vec!["abc".to_owned()],
            password: Secret::new("hunter2"),
        };

        assert!(!format!("{command:?}").contains("hunter2"));
    }

    #[test]
    fn the_value_survives_for_the_request_that_needs_it() {
        assert_eq!(Secret::new("hunter2").expose(), "hunter2");
    }
}
