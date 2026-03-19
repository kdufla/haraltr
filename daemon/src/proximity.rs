use common::config::{DisconnectActionConfig, ProximityConfig};
use tracing::{debug, info};

#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    Near { consecutive_far: u32 },
    Far { consecutive_near: u32 },
    Disconnected,
}

#[derive(Debug)]
pub enum Reading {
    Rpl(f64),
    ConnectionLost,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    Lock,
    Unlock,
    None,
}

impl From<DisconnectActionConfig> for Action {
    fn from(config: DisconnectActionConfig) -> Self {
        match config {
            DisconnectActionConfig::Lock => Action::Lock,
            DisconnectActionConfig::Unlock => Action::Unlock,
            DisconnectActionConfig::None => Action::None,
        }
    }
}

#[derive(Debug)]
pub struct State {
    phase: Phase,
    disconnect_action: Action,
    rpl_threshold: f64,
    lock_count: u32,
    unlock_count: u32,
}

impl State {
    pub fn new(config: &ProximityConfig) -> Self {
        Self {
            phase: Phase::Near { consecutive_far: 0 },
            disconnect_action: config.disconnect_action.into(),
            rpl_threshold: config.rpl_threshold,
            lock_count: config.lock_count,
            unlock_count: config.unlock_count,
        }
    }

    pub fn is_disconnected(&self) -> bool {
        matches!(self.phase, Phase::Disconnected)
    }

    pub fn transition(&mut self, reading: Reading) -> Action {
        match reading {
            Reading::Rpl(rpl) => {
                let is_far = rpl >= self.rpl_threshold;
                debug!(rpl, is_far, ?self.phase, "evaluating reading");
                self.handle_rpl(rpl, is_far)
            }
            Reading::ConnectionLost => {
                info!(previous = ?self.phase, "connection lost");
                let already_disconnected = matches!(self.phase, Phase::Disconnected);
                self.phase = Phase::Disconnected;
                if already_disconnected {
                    Action::None
                } else {
                    self.disconnect_action
                }
            }
        }
    }

    fn handle_rpl(&mut self, rpl: f64, is_far: bool) -> Action {
        match &mut self.phase {
            Phase::Near { consecutive_far } => {
                if is_far {
                    *consecutive_far += 1;
                    debug!(
                        consecutive_far = *consecutive_far,
                        required = self.lock_count,
                        "far reading while near"
                    );
                    if *consecutive_far >= self.lock_count {
                        info!(rpl, "near -> far, locking");
                        self.phase = Phase::Far {
                            consecutive_near: 0,
                        };
                        Action::Lock
                    } else {
                        Action::None
                    }
                } else {
                    *consecutive_far = 0;
                    Action::None
                }
            }
            Phase::Far { consecutive_near } => {
                if !is_far {
                    *consecutive_near += 1;
                    debug!(
                        consecutive_near = *consecutive_near,
                        required = self.unlock_count,
                        "near reading while far"
                    );
                    if *consecutive_near >= self.unlock_count {
                        info!(rpl, "far -> near, unlocking");
                        self.phase = Phase::Near { consecutive_far: 0 };
                        Action::Unlock
                    } else {
                        Action::None
                    }
                } else {
                    *consecutive_near = 0;
                    Action::None
                }
            }
            Phase::Disconnected => {
                info!(rpl, is_far, "connection restored");
                if is_far {
                    self.phase = Phase::Far {
                        consecutive_near: 0,
                    };
                } else {
                    self.phase = Phase::Far {
                        consecutive_near: 1,
                    };
                }
                Action::None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RPL_THRESHOLD: f64 = 15.0;
    const LOCK_COUNT: u32 = 5;
    const UNLOCK_COUNT: u32 = 5;
    const FAR_RPL: f64 = RPL_THRESHOLD + 1.0;
    const NEAR_RPL: f64 = RPL_THRESHOLD - 1.0;

    fn test_config(disconnect_action: DisconnectActionConfig) -> ProximityConfig {
        ProximityConfig {
            rpl_threshold: 15.0,
            lock_count: 5,
            unlock_count: 5,
            disconnect_action,
            ..ProximityConfig::default()
        }
    }

    fn new_lock_on_disconnect() -> State {
        State::new(&test_config(DisconnectActionConfig::Lock))
    }

    fn new_far_state(disconnect_action: DisconnectActionConfig) -> State {
        let mut s = State::new(&test_config(disconnect_action));
        s.phase = Phase::Far {
            consecutive_near: 0,
        };
        s
    }

    fn new_disconnected_state() -> State {
        let mut s = State::new(&test_config(DisconnectActionConfig::Lock));
        s.phase = Phase::Disconnected;
        s
    }

    #[test]
    fn stays_near_on_near_readings() {
        let mut s = new_lock_on_disconnect();
        for _ in 0..10 {
            assert_eq!(s.transition(Reading::Rpl(NEAR_RPL)), Action::None);
        }
        assert!(matches!(s.phase, Phase::Near { consecutive_far: 0 }));
    }

    #[test]
    fn locks_after_consecutive_far_readings() {
        let mut s = new_lock_on_disconnect();
        for _ in 0..LOCK_COUNT - 1 {
            assert_eq!(s.transition(Reading::Rpl(FAR_RPL)), Action::None);
        }
        assert_eq!(s.transition(Reading::Rpl(FAR_RPL)), Action::Lock);
        assert!(matches!(s.phase, Phase::Far { .. }));
    }

    #[test]
    fn far_counter_resets_on_near_reading() {
        let mut s = new_lock_on_disconnect();
        for _ in 0..LOCK_COUNT - 1 {
            s.transition(Reading::Rpl(FAR_RPL));
        }
        s.transition(Reading::Rpl(NEAR_RPL)); // reset
        for _ in 0..LOCK_COUNT - 1 {
            assert_eq!(s.transition(Reading::Rpl(FAR_RPL)), Action::None);
        }
        assert_eq!(s.transition(Reading::Rpl(FAR_RPL)), Action::Lock);
    }

    #[test]
    fn unlocks_after_consecutive_near_readings() {
        let mut s = new_far_state(DisconnectActionConfig::Lock);
        for _ in 0..UNLOCK_COUNT - 1 {
            assert_eq!(s.transition(Reading::Rpl(NEAR_RPL)), Action::None);
        }
        assert_eq!(s.transition(Reading::Rpl(NEAR_RPL)), Action::Unlock);
        assert!(matches!(s.phase, Phase::Near { .. }));
    }

    #[test]
    fn connection_lost_from_near_locks() {
        let mut s = new_lock_on_disconnect();
        assert_eq!(s.transition(Reading::ConnectionLost), Action::Lock);
        assert!(s.is_disconnected());
    }

    #[test]
    fn connection_lost_from_far_no_duplicate_action() {
        let mut s = new_far_state(DisconnectActionConfig::Lock);
        assert_eq!(s.transition(Reading::ConnectionLost), Action::Lock);
        assert_eq!(s.transition(Reading::ConnectionLost), Action::None);
    }

    #[test]
    fn connection_lost_nothing_action() {
        let mut s = State::new(&test_config(DisconnectActionConfig::None));
        assert_eq!(s.transition(Reading::ConnectionLost), Action::None);
        assert!(s.is_disconnected());
    }

    #[test]
    fn connection_lost_unlock_action() {
        let mut s = State::new(&test_config(DisconnectActionConfig::Unlock));
        assert_eq!(s.transition(Reading::ConnectionLost), Action::Unlock);
        assert!(s.is_disconnected());
    }

    #[test]
    fn reconnect_does_not_immediately_unlock() {
        let mut s = new_disconnected_state();
        assert_eq!(s.transition(Reading::Rpl(5.0)), Action::None);
        assert!(matches!(
            s.phase,
            Phase::Far {
                consecutive_near: 1
            }
        ));
    }

    #[test]
    fn reconnect_far_stays_far() {
        let mut s = new_disconnected_state();
        assert_eq!(s.transition(Reading::Rpl(20.0)), Action::None);
        assert!(matches!(
            s.phase,
            Phase::Far {
                consecutive_near: 0
            }
        ));
    }
}
