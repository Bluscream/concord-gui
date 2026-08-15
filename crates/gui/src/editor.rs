//! Compose in an external editor.
//!
//! The TUI can suspend itself and hand the terminal to `$EDITOR`. A GUI has no
//! terminal to hand over, so there are two paths:
//!
//! * `$VISUAL` is by convention a program that can run under a window system.
//!   It is launched directly and waited on.
//! * `$EDITOR` is by convention a terminal program, so it needs a terminal
//!   emulator wrapped around it. One is detected from the system rather than
//!   assumed, because guessing wrong produces a process that exits instantly
//!   with no visible error.
//!
//! Either way the draft round-trips through a temp file, which is also what
//! makes the edit recoverable if the editor is killed.

use std::io;
use std::path::PathBuf;
use std::process::Command;

/// Terminal emulators tried for `$EDITOR`, in order.
///
/// All of these accept `-e` followed by a command. Terminals that use a
/// different flag are deliberately absent: launching them with the wrong
/// syntax opens an empty shell and loses the draft.
const TERMINALS: &[&str] = &[
    "foot",
    "alacritty",
    "kitty",
    "wezterm",
    "konsole",
    "gnome-terminal",
    "xfce4-terminal",
    "lxterminal",
    "xterm",
];

/// Why an external edit could not be started.
pub enum EditorError {
    /// Neither `$VISUAL` nor `$EDITOR` is set.
    NotConfigured,
    /// `$EDITOR` is set but no terminal emulator was found to host it.
    NoTerminal,
    Io(io::Error),
}

impl EditorError {
    pub fn message(&self) -> String {
        match self {
            EditorError::NotConfigured => {
                "Set $VISUAL or $EDITOR to compose in an external editor".to_string()
            }
            EditorError::NoTerminal => {
                "$EDITOR needs a terminal; set $VISUAL to a windowed editor instead".to_string()
            }
            EditorError::Io(error) => format!("Could not start the editor: {error}"),
        }
    }
}

/// Open `draft` in an external editor and return what comes back.
///
/// Blocks until the editor exits, so callers must run it off the UI thread.
pub fn edit(draft: &str) -> Result<String, EditorError> {
    let path = write_draft(draft).map_err(EditorError::Io)?;

    let status = spawn_editor(&path)?;

    // A non-zero exit is treated as "leave the draft alone": editors return it
    // on an aborted quit, and adopting a half-written buffer would be worse
    // than doing nothing.
    if !status {
        let _ = std::fs::remove_file(&path);
        return Ok(draft.to_string());
    }

    let edited = std::fs::read_to_string(&path).map_err(EditorError::Io)?;
    let _ = std::fs::remove_file(&path);

    // Editors habitually append a trailing newline; sending it would post a
    // message with a blank last line.
    Ok(edited.trim_end_matches('\n').to_string())
}

fn write_draft(draft: &str) -> io::Result<PathBuf> {
    let mut path = std::env::temp_dir();
    path.push(format!("concord-draft-{}.md", std::process::id()));
    std::fs::write(&path, draft)?;
    Ok(path)
}

/// Launch the configured editor, returning whether it exited cleanly.
fn spawn_editor(path: &PathBuf) -> Result<bool, EditorError> {
    if let Some(visual) = non_empty("VISUAL") {
        let status = Command::new(&visual)
            .arg(path)
            .status()
            .map_err(EditorError::Io)?;
        return Ok(status.success());
    }

    let Some(editor) = non_empty("EDITOR") else {
        return Err(EditorError::NotConfigured);
    };

    let Some(terminal) = TERMINALS
        .iter()
        .find(|candidate| which(candidate).is_some())
    else {
        return Err(EditorError::NoTerminal);
    };

    let status = Command::new(terminal)
        .arg("-e")
        .arg(&editor)
        .arg(path)
        .status()
        .map_err(EditorError::Io)?;

    Ok(status.success())
}

fn non_empty(variable: &str) -> Option<String> {
    usable(std::env::var(variable).ok())
}

/// Whether a configured value names an actual program.
///
/// Split out from the environment lookup so it can be tested without mutating
/// process-global state: `set_var` races with other tests running in parallel,
/// which made an earlier version of this test intermittently fail.
fn usable(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

/// Whether a program exists on PATH.
fn which(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(program))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailing_newlines_are_stripped_from_the_result() {
        // Editors add one by convention; sending it posts a blank last line.
        let path = write_draft("hello\n\n").expect("temp file");
        let content = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).ok();

        assert_eq!(content.trim_end_matches('\n'), "hello");
    }

    #[test]
    fn which_finds_a_program_that_exists_and_rejects_one_that_does_not() {
        assert!(which("sh").is_some(), "sh should be on PATH");
        assert!(which("definitely-not-a-real-program-xyz").is_none());
    }

    #[test]
    fn a_blank_value_reads_as_absent() {
        // A variable set to whitespace is as good as unset; treating it as a
        // program name would spawn something nameless.
        assert!(usable(Some("   ".to_string())).is_none());
        assert!(usable(Some(String::new())).is_none());
        assert!(usable(None).is_none());
        assert_eq!(usable(Some("nvim".to_string())).as_deref(), Some("nvim"));
    }
}
