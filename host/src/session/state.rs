use crate::wire::BotMessage;

/// Lifecycle state of a single bot session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// TCP connected, waiting for Identify.
    Connected,
    /// Bot identified, waiting for Ready.
    Identified,
    /// Host sends MatchConfig + StartPreprocessing, waiting for PreprocessingDone.
    Awaiting,
    /// Turn loop — Action accepted.
    Playing,
    /// GameOver sent, winding down.
    Done,
}

impl SessionState {
    /// Whether a bot message of the given type is accepted in this state.
    ///
    /// Messages that are *always* accepted (Pong, Info, RenderCommands) return
    /// `true` in every state except `Done`.
    ///
    /// State-specific messages (Identify, Ready, PreprocessingDone, Action)
    /// are only accepted in the one state where they cause a transition.
    pub fn accepts(&self, msg: BotMessage) -> bool {
        if *self == Self::Done {
            return false;
        }

        // Always accepted in any non-Done state.
        if msg == BotMessage::Pong || msg == BotMessage::Info || msg == BotMessage::RenderCommands {
            return true;
        }

        // State-specific messages — accepted in exactly one state.
        match self {
            Self::Connected => msg == BotMessage::Identify,
            Self::Identified => msg == BotMessage::Ready,
            Self::Awaiting => msg == BotMessage::PreprocessingDone,
            Self::Playing => msg == BotMessage::Action,
            Self::Done => false, // already handled above, but exhaustive
        }
    }

    /// Compute the next state after receiving a valid bot message.
    ///
    /// Returns `None` if the message doesn't trigger a state transition
    /// (e.g., Pong, Info, or Action while Playing).
    pub fn transition(&self, msg: BotMessage) -> Option<SessionState> {
        if *self == Self::Connected && msg == BotMessage::Identify {
            Some(Self::Identified)
        } else if *self == Self::Identified && msg == BotMessage::Ready {
            Some(Self::Awaiting)
        } else if *self == Self::Awaiting && msg == BotMessage::PreprocessingDone {
            Some(Self::Playing)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── accepts() ───────────────────────────────────

    #[test]
    fn connected_accepts_identify() {
        assert!(SessionState::Connected.accepts(BotMessage::Identify));
    }

    #[test]
    fn connected_rejects_ready() {
        assert!(!SessionState::Connected.accepts(BotMessage::Ready));
    }

    #[test]
    fn connected_rejects_action() {
        assert!(!SessionState::Connected.accepts(BotMessage::Action));
    }

    #[test]
    fn identified_accepts_ready() {
        assert!(SessionState::Identified.accepts(BotMessage::Ready));
    }

    #[test]
    fn identified_rejects_identify() {
        assert!(!SessionState::Identified.accepts(BotMessage::Identify));
    }

    #[test]
    fn awaiting_accepts_preprocessing_done() {
        assert!(SessionState::Awaiting.accepts(BotMessage::PreprocessingDone));
    }

    #[test]
    fn awaiting_rejects_action() {
        assert!(!SessionState::Awaiting.accepts(BotMessage::Action));
    }

    #[test]
    fn playing_accepts_action() {
        assert!(SessionState::Playing.accepts(BotMessage::Action));
    }

    #[test]
    fn playing_rejects_identify() {
        assert!(!SessionState::Playing.accepts(BotMessage::Identify));
    }

    // ── Always-accepted messages ────────────────────

    #[test]
    fn pong_accepted_in_all_non_done_states() {
        for state in [
            SessionState::Connected,
            SessionState::Identified,
            SessionState::Awaiting,
            SessionState::Playing,
        ] {
            assert!(
                state.accepts(BotMessage::Pong),
                "Pong rejected in {state:?}"
            );
        }
    }

    #[test]
    fn info_accepted_in_all_non_done_states() {
        for state in [
            SessionState::Connected,
            SessionState::Identified,
            SessionState::Awaiting,
            SessionState::Playing,
        ] {
            assert!(
                state.accepts(BotMessage::Info),
                "Info rejected in {state:?}"
            );
        }
    }

    #[test]
    fn render_commands_accepted_in_all_non_done_states() {
        for state in [
            SessionState::Connected,
            SessionState::Identified,
            SessionState::Awaiting,
            SessionState::Playing,
        ] {
            assert!(
                state.accepts(BotMessage::RenderCommands),
                "RenderCommands rejected in {state:?}"
            );
        }
    }

    // ── Done rejects everything ─────────────────────

    #[test]
    fn done_rejects_all() {
        let all_msgs = [
            BotMessage::Identify,
            BotMessage::Ready,
            BotMessage::PreprocessingDone,
            BotMessage::Action,
            BotMessage::Pong,
            BotMessage::Info,
            BotMessage::RenderCommands,
        ];
        for msg in all_msgs {
            assert!(
                !SessionState::Done.accepts(msg),
                "Done should reject {msg:?}"
            );
        }
    }

    // ── transition() ────────────────────────────────

    #[test]
    fn identify_transitions_to_identified() {
        assert_eq!(
            SessionState::Connected.transition(BotMessage::Identify),
            Some(SessionState::Identified)
        );
    }

    #[test]
    fn ready_transitions_to_awaiting() {
        assert_eq!(
            SessionState::Identified.transition(BotMessage::Ready),
            Some(SessionState::Awaiting)
        );
    }

    #[test]
    fn preprocessing_done_transitions_to_playing() {
        assert_eq!(
            SessionState::Awaiting.transition(BotMessage::PreprocessingDone),
            Some(SessionState::Playing)
        );
    }

    #[test]
    fn action_does_not_transition() {
        assert_eq!(SessionState::Playing.transition(BotMessage::Action), None);
    }

    #[test]
    fn pong_does_not_transition() {
        assert_eq!(SessionState::Connected.transition(BotMessage::Pong), None);
    }
}
