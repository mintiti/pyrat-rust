use crate::session::SessionId;
use crate::wire::Player;

use super::config::PlayerEntry;

/// Internal state of a player slot.
#[derive(Debug, Clone)]
struct SlotState {
    player: Player,
    agent_id: String,
    claimed_by: Option<SessionId>,
}

/// Tracks which sessions have claimed which player slots.
///
/// Pure synchronous struct — no locks, no Arc. Owned locally by the setup phase.
#[derive(Debug)]
pub(crate) struct PlayerSlots {
    slots: Vec<SlotState>,
}

impl PlayerSlots {
    pub fn new(players: &[PlayerEntry]) -> Self {
        Self {
            slots: players
                .iter()
                .map(|e| SlotState {
                    player: e.player,
                    agent_id: e.agent_id.clone(),
                    claimed_by: None,
                })
                .collect(),
        }
    }

    /// Claim all unclaimed slots matching `agent_id` for the given session.
    ///
    /// Returns the list of players claimed. Empty if no slots match or all
    /// matching slots are already claimed.
    pub fn reserve(&mut self, session_id: SessionId, agent_id: &str) -> Vec<Player> {
        let mut claimed = Vec::new();
        for slot in &mut self.slots {
            if slot.agent_id == agent_id && slot.claimed_by.is_none() {
                slot.claimed_by = Some(session_id);
                claimed.push(slot.player);
            }
        }
        claimed
    }

    /// Free all slots held by the given session.
    pub fn unreserve(&mut self, session_id: SessionId) {
        for slot in &mut self.slots {
            if slot.claimed_by == Some(session_id) {
                slot.claimed_by = None;
            }
        }
    }

    /// Whether every slot has been claimed.
    pub fn all_claimed(&self) -> bool {
        self.slots.iter().all(|s| s.claimed_by.is_some())
    }

    /// Whether both slots share the same agent_id (hivemind mode).
    #[allow(dead_code)] // Used by game loop (chunk 5+).
    pub fn is_hivemind(&self) -> bool {
        if self.slots.len() != 2 {
            return false;
        }
        self.slots[0].agent_id == self.slots[1].agent_id
    }

    /// Return the players assigned to a given session.
    #[allow(dead_code)] // Used by game loop (chunk 5+).
    pub fn players_for_session(&self, session_id: SessionId) -> Vec<Player> {
        self.slots
            .iter()
            .filter(|s| s.claimed_by == Some(session_id))
            .map(|s| s.player)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(pairs: &[(Player, &str)]) -> Vec<PlayerEntry> {
        pairs
            .iter()
            .map(|(p, id)| PlayerEntry {
                player: *p,
                agent_id: id.to_string(),
            })
            .collect()
    }

    #[test]
    fn reserve_normal_two_agents() {
        let players = entries(&[(Player::Rat, "bot-a"), (Player::Python, "bot-b")]);
        let mut slots = PlayerSlots::new(&players);

        let claimed = slots.reserve(SessionId(1), "bot-a");
        assert_eq!(claimed, vec![Player::Rat]);
        assert!(!slots.all_claimed());

        let claimed = slots.reserve(SessionId(2), "bot-b");
        assert_eq!(claimed, vec![Player::Python]);
        assert!(slots.all_claimed());
    }

    #[test]
    fn reserve_hivemind() {
        let players = entries(&[(Player::Rat, "hive"), (Player::Python, "hive")]);
        let mut slots = PlayerSlots::new(&players);
        assert!(slots.is_hivemind());

        let claimed = slots.reserve(SessionId(1), "hive");
        assert_eq!(claimed, vec![Player::Rat, Player::Python]);
        assert!(slots.all_claimed());
    }

    #[test]
    fn reserve_wrong_agent_id() {
        let players = entries(&[(Player::Rat, "bot-a"), (Player::Python, "bot-b")]);
        let mut slots = PlayerSlots::new(&players);

        let claimed = slots.reserve(SessionId(1), "unknown");
        assert!(claimed.is_empty());
        assert!(!slots.all_claimed());
    }

    #[test]
    fn unreserve_frees_slot() {
        let players = entries(&[(Player::Rat, "bot-a"), (Player::Python, "bot-b")]);
        let mut slots = PlayerSlots::new(&players);

        slots.reserve(SessionId(1), "bot-a");
        slots.reserve(SessionId(2), "bot-b");
        assert!(slots.all_claimed());

        slots.unreserve(SessionId(1));
        assert!(!slots.all_claimed());

        // Re-reserve with different session
        let claimed = slots.reserve(SessionId(3), "bot-a");
        assert_eq!(claimed, vec![Player::Rat]);
        assert!(slots.all_claimed());
    }

    #[test]
    fn is_hivemind_detection() {
        let hive = entries(&[(Player::Rat, "same"), (Player::Python, "same")]);
        assert!(PlayerSlots::new(&hive).is_hivemind());

        let diff = entries(&[(Player::Rat, "a"), (Player::Python, "b")]);
        assert!(!PlayerSlots::new(&diff).is_hivemind());
    }

    #[test]
    fn players_for_session_returns_correct_set() {
        let players = entries(&[(Player::Rat, "hive"), (Player::Python, "hive")]);
        let mut slots = PlayerSlots::new(&players);
        slots.reserve(SessionId(1), "hive");

        let ps = slots.players_for_session(SessionId(1));
        assert_eq!(ps, vec![Player::Rat, Player::Python]);

        let ps = slots.players_for_session(SessionId(99));
        assert!(ps.is_empty());
    }
}
