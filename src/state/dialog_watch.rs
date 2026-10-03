//! Waiting acceleration: Claude Code paints a permission dialog instantly
//! but emits its Notification hook — our waiting signal — ~6s later
//! (measured 2026-10-03). While the sidebar is open, its 1s refresh can
//! look at running panes' screen bottom and flip them to Waiting as soon
//! as the dialog is actually visible. The hook's later write lands on the
//! same value (idempotent), so this only moves the transition earlier for
//! every @pane_status reader (sidebar rows, picker, agent-server).

use crate::tmux;

/// Live confirm-hint shapes observed on real dialogs: tool permission
/// "Esc to cancel · Tab to amend", notice types "Enter to confirm ·
/// Esc to cancel", banner "y to continue · n to cancel". A hint alone is
/// not enough — a numbered option (or the y/n pair) must share the same
/// bottom window so quoted transcript text cannot trip the classifier.
pub(crate) fn screen_shows_dialog(bottom: &str) -> bool {
    let numbered = bottom.lines().any(|l| {
        let t = l.trim_start_matches([' ', '❯', '>']);
        let mut cs = t.chars();
        matches!(cs.next(), Some('1'..='9'))
            && matches!(cs.next(), Some('.' | ')'))
            && cs.next().map_or(false, |c| c == ' ')
    });
    let yn = bottom.contains("y to continue") && bottom.contains("n to cancel");
    ((bottom.contains("Esc to cancel") || bottom.contains("Enter to confirm")) && numbered) || yn
}

/// Bottom ~10 rendered lines of the pane — the only place a LIVE dialog
/// sits; answered ones scroll up out of this window.
pub(crate) fn pane_shows_dialog(pane_id: &str) -> bool {
    match tmux::run_tmux_capture(&["capture-pane", "-p", "-S", "-10", "-t", pane_id]) {
        Ok(out) => screen_shows_dialog(&out),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::screen_shows_dialog;

    #[test]
    fn tool_permission_dialog_is_detected() {
        let bottom = "
 Do you want to proceed?
 ❯ 1. Yes
   2. Yes, allow reading from /etc from this project
   3. Yes, and switch to auto mode · auto mode handles these prompts for you
   4. No
 Esc to cancel · Tab to amend
";
        assert!(screen_shows_dialog(bottom));
    }

    #[test]
    fn notice_dialog_is_detected() {
        let bottom = "
 ❯ 1. Yes
   2. Not now
   3. Don't show again
 Enter to confirm · Esc to cancel
";
        assert!(screen_shows_dialog(bottom));
    }

    #[test]
    fn yn_banner_is_detected() {
        let bottom = "
 However, this session isn't eligible.
 y to continue · n to cancel
";
        assert!(screen_shows_dialog(bottom));
    }

    #[test]
    fn working_widget_is_not_a_dialog() {
        let bottom = "
· Dilly-dallying… (2m 9s · ↓ 7.4k tokens)
────────────────────────────────
❯
────────────────────────────────
  proj git:(main*) | [glm-5.3[1m]] █░░░░░ 10% | 1 CLAUDE.md
  ⏵⏵ auto mode on (shift+tab to cycle) · ← 3 agents
";
        assert!(!screen_shows_dialog(bottom));
    }

    #[test]
    fn hint_without_numbered_options_is_ignored() {
        let bottom = "
 some transcript line mentioning Esc to cancel in prose
 another plain line
";
        assert!(!screen_shows_dialog(bottom));
    }
}
