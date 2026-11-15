//! Tripmines (T-087): arm, fire once, then reopen the route.
//!
//! A tripmine is a ground hazard with three phases: it arms over a short window (the visible
//! telegraph), fires once when a player is inside its radius, then disarms for a while so the
//! route reopens behind whoever set it off. Pure logic: no server deps, no wall-clock time, no
//! transcendental float math, so it is safe to compile into the wasm prediction build.

/// Radius that triggers a tripmine, in units.
pub const TRIPMINE_RADIUS: f32 = 1.6;
/// Ticks a fresh tripmine spends arming (2.0 s at 20 Hz) - the visible telegraph.
pub const TRIPMINE_ARM_TICKS: u32 = 40;
/// Ticks a tripmine stays disarmed after firing (6.0 s at 20 Hz).
pub const TRIPMINE_DISARM_TICKS: u32 = 120;

/// What a tripmine is doing right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TripminePhase {
    /// Winding up: the telegraph is visible and the mine cannot fire yet.
    #[default]
    Arming,
    /// Live: the next successful `trigger` fires it.
    Armed,
    /// Spent: the route is open again until the mine finishes re-arming.
    Disarmed,
}

/// One tripmine's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tripmine {
    /// What the mine is doing right now.
    phase: TripminePhase,
    /// Ticks left in the current phase; always 0 while `Armed`.
    ticks: u32,
}

impl Default for Tripmine {
    /// Same as [`Tripmine::new`]: a fresh mine, arming.
    fn default() -> Tripmine {
        Tripmine::new()
    }
}

impl Tripmine {
    /// A fresh tripmine, arming.
    pub fn new() -> Tripmine {
        Tripmine {
            phase: TripminePhase::Arming,
            ticks: TRIPMINE_ARM_TICKS,
        }
    }

    /// What it is doing right now.
    pub fn phase(&self) -> TripminePhase {
        self.phase
    }

    /// Ticks left in the current phase (0 when armed and waiting).
    pub fn ticks_left(&self) -> u32 {
        self.ticks
    }

    /// True only while armed: the phase in which it can fire.
    pub fn is_armed(&self) -> bool {
        self.phase == TripminePhase::Armed
    }

    /// Advance one tick: Arming -> Armed, Disarmed -> Arming.
    pub fn tick(&mut self) {
        match self.phase {
            TripminePhase::Arming => {
                self.ticks = self.ticks.saturating_sub(1);
                if self.ticks == 0 {
                    self.phase = TripminePhase::Armed;
                }
            }
            // An armed mine waits for a trigger; ticking does not change it.
            TripminePhase::Armed => {}
            TripminePhase::Disarmed => {
                self.ticks = self.ticks.saturating_sub(1);
                if self.ticks == 0 {
                    self.phase = TripminePhase::Arming;
                    self.ticks = TRIPMINE_ARM_TICKS;
                }
            }
        }
    }

    /// Fire if armed. Returns true when it actually fired (and is now disarmed).
    pub fn trigger(&mut self) -> bool {
        if self.phase != TripminePhase::Armed {
            return false;
        }
        self.phase = TripminePhase::Disarmed;
        self.ticks = TRIPMINE_DISARM_TICKS;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run `ticks` simulation ticks on `mine`.
    fn advance(mine: &mut Tripmine, ticks: u32) {
        for _ in 0..ticks {
            mine.tick();
        }
    }

    #[test]
    fn fresh_mine_is_arming_and_not_armed() {
        let mine = Tripmine::new();
        assert_eq!(mine.phase(), TripminePhase::Arming);
        assert!(!mine.is_armed());
        assert_eq!(mine.ticks_left(), TRIPMINE_ARM_TICKS);
    }

    #[test]
    fn default_is_a_fresh_mine() {
        assert_eq!(Tripmine::default(), Tripmine::new());
    }

    #[test]
    fn arms_after_exactly_arm_ticks() {
        let mut mine = Tripmine::new();
        advance(&mut mine, TRIPMINE_ARM_TICKS - 1);
        assert_eq!(mine.phase(), TripminePhase::Arming);
        assert!(!mine.is_armed());
        assert_eq!(mine.ticks_left(), 1);

        mine.tick();
        assert_eq!(mine.phase(), TripminePhase::Armed);
        assert!(mine.is_armed());
        assert_eq!(mine.ticks_left(), 0);
    }

    #[test]
    fn armed_mine_stays_armed_while_ticking() {
        let mut mine = Tripmine::new();
        advance(&mut mine, TRIPMINE_ARM_TICKS);
        advance(&mut mine, TRIPMINE_DISARM_TICKS * 2);
        assert_eq!(mine.phase(), TripminePhase::Armed);
        assert!(mine.is_armed());
        assert_eq!(mine.ticks_left(), 0);
    }

    #[test]
    fn trigger_while_arming_returns_false_and_changes_nothing() {
        let mut mine = Tripmine::new();
        advance(&mut mine, TRIPMINE_ARM_TICKS / 2);
        let before = mine;
        assert!(!mine.trigger());
        assert_eq!(mine, before);
    }

    #[test]
    fn trigger_while_armed_fires_and_disarms() {
        let mut mine = Tripmine::new();
        advance(&mut mine, TRIPMINE_ARM_TICKS);
        assert!(mine.trigger());
        assert_eq!(mine.phase(), TripminePhase::Disarmed);
        assert!(!mine.is_armed());
        assert_eq!(mine.ticks_left(), TRIPMINE_DISARM_TICKS);
    }

    #[test]
    fn trigger_while_disarmed_returns_false_and_changes_nothing() {
        let mut mine = Tripmine::new();
        advance(&mut mine, TRIPMINE_ARM_TICKS);
        assert!(mine.trigger());
        let before = mine;
        assert!(!mine.trigger());
        assert_eq!(mine, before);
    }

    #[test]
    fn rearms_after_disarm_ticks() {
        let mut mine = Tripmine::new();
        advance(&mut mine, TRIPMINE_ARM_TICKS);
        assert!(mine.trigger());

        advance(&mut mine, TRIPMINE_DISARM_TICKS - 1);
        assert_eq!(mine.phase(), TripminePhase::Disarmed);
        assert_eq!(mine.ticks_left(), 1);

        mine.tick();
        assert_eq!(mine.phase(), TripminePhase::Arming);
        assert_eq!(mine.ticks_left(), TRIPMINE_ARM_TICKS);

        // ...and it arms again after the full telegraph.
        advance(&mut mine, TRIPMINE_ARM_TICKS);
        assert!(mine.is_armed());
    }

    #[test]
    fn second_trigger_while_disarmed_returns_false() {
        let mut mine = Tripmine::new();
        advance(&mut mine, TRIPMINE_ARM_TICKS);
        assert!(mine.trigger());
        assert!(!mine.trigger());
        assert_eq!(mine.phase(), TripminePhase::Disarmed);
        assert_eq!(mine.ticks_left(), TRIPMINE_DISARM_TICKS);
    }
}
