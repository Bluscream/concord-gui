//! Warning before an action Discord's anti-spam checks watch.
//!
//! The wording and the opt-out live in `crate::risk`, shared with the GUI, so
//! both clients warn about the same things in the same words. Only the popup
//! plumbing is here.

use crate::discord::AppCommand;
use crate::risk::RiskKind;

use super::super::DashboardState;
use super::{ModalPopup, RiskWarningState};

impl DashboardState {
    /// Carry out a risky action, asking first unless the user said not to.
    ///
    /// Returns the command when it may go now. `None` means a warning is on
    /// screen and the command is being held, not that it was refused.
    pub fn request_risky(&mut self, kind: RiskKind, command: AppCommand) -> Option<AppCommand> {
        if kind.suppressed(&self.options.warning_options) {
            return Some(command);
        }
        self.popups.confirmation_button = super::ConfirmationButton::default();
        let previous = self.popups.take_modal().map(Box::new);
        self.popups
            .set_modal(ModalPopup::RiskWarning(RiskWarningState {
                kind,
                command,
                dont_ask: false,
                previous,
            }));
        None
    }

    /// Back out, leaving whatever the warning covered on screen.
    pub fn close_risk_warning(&mut self) {
        if let Some(warning) = self.popups.take_risk_warning() {
            self.restore_behind_risk_warning(warning.previous);
        }
    }

    fn restore_behind_risk_warning(&mut self, previous: Option<Box<ModalPopup>>) {
        match previous {
            Some(popup) => self.popups.set_modal(*popup),
            None => self.popups.clear_modal(),
        }
    }

    /// Flip "don't ask again" without deciding anything else.
    ///
    /// Separate from confirming so it can be read, ticked and then cancelled -
    /// deciding not to do this one thing is not the same as wanting the
    /// warning back next time.
    pub fn toggle_risk_dont_ask(&mut self) {
        if let Some(warning) = self.popups.risk_warning_mut() {
            warning.dont_ask = !warning.dont_ask;
        }
    }

    pub(in crate::tui) fn risk_warning(&self) -> Option<&RiskWarningState> {
        self.popups.risk_warning()
    }

    /// Go ahead, and stop asking if that was ticked.
    pub fn confirm_risk_warning(&mut self) -> Option<AppCommand> {
        let warning = self.popups.take_risk_warning()?;
        self.restore_behind_risk_warning(warning.previous);
        if warning.dont_ask {
            warning.kind.suppress(&mut self.options.warning_options);
            self.mark_options_changed();
        }
        Some(warning.command)
    }
}

impl RiskWarningState {
    pub(in crate::tui) fn explanation(&self) -> String {
        self.kind.explanation()
    }

    pub(in crate::tui) fn dont_ask(&self) -> bool {
        self.dont_ask
    }
}
