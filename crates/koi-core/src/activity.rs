//! What an agent is doing, as reported through its status file or guessed
//! from its terminal title.

/// What an agent is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    /// Running a turn.
    Working,
    /// Finished and waiting for the next prompt.
    Idle,
    /// Blocked until the user answers, for example a permission prompt.
    Waiting,
}

impl Activity {
    /// The word integrations write to the status file.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::Idle => "idle",
            Self::Waiting => "waiting",
        }
    }

    /// Parses status file contents; `None` for an empty or unknown value,
    /// which happens while the file is being rewritten.
    pub fn parse(contents: &str) -> Option<Self> {
        [Self::Working, Self::Idle, Self::Waiting]
            .into_iter()
            .find(|activity| activity.as_str() == contents.trim())
    }

    /// The activity of a program status report (OSC 7501) in `state`;
    /// `None` for `clear`, sent when the program stops, and unknown states.
    pub fn from_program_state(state: &str) -> Option<Self> {
        match state {
            "working" => Some(Self::Working),
            "blocked" => Some(Self::Waiting),
            "idle" | "done" | "error" => Some(Self::Idle),
            _ => None,
        }
    }

    /// Guesses the activity of an agent that does not report it, from its
    /// terminal title: many command-line tools put a spinner at the start of
    /// the title while they work. A title without one means idle only after
    /// a spinner was seen; before that, the title says nothing.
    pub fn guess_from_title(title: &str, previous: Option<Self>) -> Option<Self> {
        let spinning = title.trim_start().chars().next().is_some_and(is_spinner);
        match (spinning, previous) {
            (true, _) => Some(Self::Working),
            (false, Some(Self::Working)) => Some(Self::Idle),
            (false, previous) => previous,
        }
    }
}

/// Characters used by common terminal spinners: braille dots and circle
/// quarters.
fn is_spinner(c: char) -> bool {
    matches!(
        c,
        '\u{2801}'..='\u{28ff}' | '◐' | '◓' | '◑' | '◒' | '◴' | '◷' | '◶' | '◵'
    )
}

/// Why a change of activity deserves the user's attention.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attention {
    /// The agent is blocked on the user.
    Waiting,
    /// The agent finished a turn.
    Finished,
}

impl Attention {
    /// Whether going from `previous` to `current` should be brought to the
    /// user's attention.
    pub fn of_change(previous: Option<Activity>, current: Activity) -> Option<Self> {
        match (previous, current) {
            (Some(Activity::Waiting), Activity::Waiting) => None,
            (_, Activity::Waiting) => Some(Self::Waiting),
            (Some(Activity::Working), Activity::Idle) => Some(Self::Finished),
            _ => None,
        }
    }
}

/// Whether a session has news the user has not looked at once its activity
/// went from `previous` to `current`. A finished turn or a question counts
/// unless the user is `watching`; starting a new turn clears it, as the
/// user has answered.
pub fn unseen_after_change(
    unseen: bool,
    previous: Option<Activity>,
    current: Activity,
    watching: bool,
) -> bool {
    if current == Activity::Working {
        return false;
    }
    unseen || (!watching && Attention::of_change(previous, current).is_some())
}

/// An agent started by hand in a terminal whose own agent returned to the
/// shell, or that agent exiting again. `reentered` is the agent started so
/// far and `found` the one running now. Returns the new agent, `Some(None)`
/// meaning back to the shell, or `None` when nothing changed.
pub fn reentry_change<'a>(
    shell: bool,
    reentered: Option<&str>,
    found: Option<&'a str>,
) -> Option<Option<&'a str>> {
    (shell || reentered.is_some())
        .then_some(found)
        .filter(|&found| found != reentered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spinners_in_titles_mean_working() {
        assert_eq!(
            Activity::guess_from_title("⠋ Building", None),
            Some(Activity::Working)
        );
        assert_eq!(
            Activity::guess_from_title(" ◐ sync", None),
            Some(Activity::Working)
        );
        assert_eq!(
            Activity::guess_from_title("my-agent", Some(Activity::Working)),
            Some(Activity::Idle)
        );
    }

    #[test]
    fn plain_titles_say_nothing_before_a_spinner() {
        assert_eq!(Activity::guess_from_title("~/code/app", None), None);
        assert_eq!(
            Activity::guess_from_title("app", Some(Activity::Idle)),
            Some(Activity::Idle)
        );
        // An empty braille cell is a blank, not a spinner frame.
        assert_eq!(Activity::guess_from_title("\u{2800} x", None), None);
    }

    #[test]
    fn attention_on_waiting_and_on_finishing() {
        use Activity::*;
        assert_eq!(
            Attention::of_change(Some(Working), Waiting),
            Some(Attention::Waiting)
        );
        assert_eq!(
            Attention::of_change(Some(Idle), Waiting),
            Some(Attention::Waiting)
        );
        assert_eq!(Attention::of_change(Some(Waiting), Waiting), None);
        assert_eq!(
            Attention::of_change(Some(Working), Idle),
            Some(Attention::Finished)
        );
        assert_eq!(Attention::of_change(Some(Waiting), Idle), None);
        assert_eq!(Attention::of_change(Some(Idle), Idle), None);
        assert_eq!(Attention::of_change(Some(Idle), Working), None);
    }

    #[test]
    fn unseen_until_looked_at_or_answered() {
        use Activity::*;
        assert!(unseen_after_change(false, Some(Working), Idle, false));
        assert!(unseen_after_change(false, Some(Working), Waiting, false));
        assert!(!unseen_after_change(false, Some(Working), Idle, true));
        assert!(!unseen_after_change(false, Some(Idle), Idle, false));
        assert!(unseen_after_change(true, Some(Waiting), Idle, true));
        assert!(!unseen_after_change(true, Some(Waiting), Working, false));
    }

    #[test]
    fn maps_program_states() {
        assert_eq!(
            Activity::from_program_state("working"),
            Some(Activity::Working)
        );
        assert_eq!(
            Activity::from_program_state("blocked"),
            Some(Activity::Waiting)
        );
        for state in ["idle", "done", "error"] {
            assert_eq!(Activity::from_program_state(state), Some(Activity::Idle));
        }
        assert_eq!(Activity::from_program_state("clear"), None);
    }

    #[test]
    fn parses_status_words() {
        assert_eq!(Activity::parse("working"), Some(Activity::Working));
        assert_eq!(Activity::parse("idle\n"), Some(Activity::Idle));
        assert_eq!(Activity::parse("waiting"), Some(Activity::Waiting));
        assert_eq!(Activity::parse(""), None);
        assert_eq!(Activity::parse("busy"), None);
    }

    #[test]
    fn agents_started_by_hand_are_followed_until_they_exit() {
        // The session's own agent is still running.
        assert_eq!(reentry_change(false, None, Some("pi")), None);
        assert_eq!(reentry_change(true, None, None), None);
        assert_eq!(
            reentry_change(true, None, Some("opencode")),
            Some(Some("opencode"))
        );
        assert_eq!(
            reentry_change(false, Some("opencode"), Some("opencode")),
            None
        );
        assert_eq!(reentry_change(false, Some("opencode"), None), Some(None));
        assert_eq!(
            reentry_change(false, Some("opencode"), Some("pi")),
            Some(Some("pi"))
        );
    }
}
