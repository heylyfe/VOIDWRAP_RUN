//! Greater curses (T-084): run-shaping threats with escape mechanics.

/// Ticks a Wreath cries before its victim dies (12.0 s at 20 Hz).
pub const WREATH_CRY_TICKS: u32 = 240;
/// Ticks of visible crying before the timer starts biting (3.0 s).
pub const WREATH_GRACE_TICKS: u32 = 60;
/// Currency Tithe takes per normal gift collected.
pub const TITHE_PER_GIFT: u32 = 1;
/// Ticks of slow a Tithe victim carries after collecting at zero currency (6.0 s).
pub const TITHE_SLOW_TICKS: u32 = 120;
/// Speed multiplier while a Tithe victim is slowed.
pub const TITHE_SLOW_MUL: f32 = 0.75;
/// Fog multiplier Hush applies on top of the run's other fog mods.
pub const HUSH_FOG_MUL: f32 = 1.6;

/// The three greater curses that carry a runtime mechanic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GreaterCurse {
    Wreath,
    Tithe,
    Hush,
}

impl GreaterCurse {
    /// Every mechanic-bearing greater curse, in content order.
    pub fn all() -> [GreaterCurse; 3] {
        [GreaterCurse::Wreath, GreaterCurse::Tithe, GreaterCurse::Hush]
    }

    /// Map a `design/content.json` curse id to its mechanic; `None` for stat-only curses.
    pub fn from_id(id: &str) -> Option<GreaterCurse> {
        match id {
            "wreath" => Some(GreaterCurse::Wreath),
            "tithe" => Some(GreaterCurse::Tithe),
            "hush" => Some(GreaterCurse::Hush),
            _ => None,
        }
    }

    /// The content id this mechanic is driven by.
    pub fn id(self) -> &'static str {
        match self {
            GreaterCurse::Wreath => "wreath",
            GreaterCurse::Tithe => "tithe",
            GreaterCurse::Hush => "hush",
        }
    }
}

/// One Wreath attached to one player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wreath {
    victim: u32,
    ticks: u32,
}

/// What one Wreath tick did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WreathOutcome {
    Escaped,
    Crying,
    Killed,
}

impl Wreath {
    /// Attach a fresh Wreath to `victim`.
    pub fn attach(victim: u32) -> Wreath {
        Wreath { victim, ticks: 0 }
    }

    /// The player it sits on.
    pub fn victim(&self) -> u32 {
        self.victim
    }

    /// Ticks it has cried so far.
    pub fn ticks(&self) -> u32 {
        self.ticks
    }

    /// True once the grace window is over and the timer is biting (drives the loud telegraph).
    pub fn is_biting(&self) -> bool {
        self.ticks > WREATH_GRACE_TICKS
    }

    /// Advance one tick. `escaped` is true when the victim launched off a jump pad or is gliding
    /// this tick - the level's own geometry is the counter. Escaping ends the Wreath.
    pub fn tick(&mut self, escaped: bool) -> WreathOutcome {
        if escaped {
            return WreathOutcome::Escaped;
        }
        self.ticks += 1;
        if self.ticks >= WREATH_CRY_TICKS {
            WreathOutcome::Killed
        } else {
            WreathOutcome::Crying
        }
    }
}

/// Per-player Tithe slow timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TitheSlow {
    ticks: u32,
}

impl TitheSlow {
    /// Start (or refresh) the slow.
    pub fn apply(&mut self) {
        self.ticks = TITHE_SLOW_TICKS;
    }

    /// Advance one tick.
    pub fn tick(&mut self) {
        self.ticks = self.ticks.saturating_sub(1);
    }

    /// True while the player is slowed.
    pub fn is_slowed(&self) -> bool {
        self.ticks > 0
    }

    /// `TITHE_SLOW_MUL` while slowed, else `1.0`.
    pub fn speed_mul(&self) -> f32 {
        if self.is_slowed() { TITHE_SLOW_MUL } else { 1.0 }
    }
}

/// Currency Tithe takes for `gifts` normal gifts collected.
pub fn tithe_cost(gifts: u32) -> u32 {
    gifts.saturating_mul(TITHE_PER_GIFT)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-6;

    #[test]
    fn escape_by_jump_pad_ends_the_wreath() {
        let mut wreath = Wreath::attach(7);
        // A jump-pad launch this tick counts as an escape (`PlayerSense::jump_launched`).
        assert_eq!(wreath.tick(true), WreathOutcome::Escaped);
        assert_eq!(wreath.ticks(), 0, "escaping leaves the tick counter unchanged");
    }

    #[test]
    fn escape_by_glider_ends_the_wreath() {
        let mut wreath = Wreath::attach(9);
        for _ in 0..WREATH_GRACE_TICKS {
            assert_eq!(wreath.tick(false), WreathOutcome::Crying);
        }
        // Gliding this tick counts as an escape (`PlayerSense::gliding`).
        assert_eq!(wreath.tick(true), WreathOutcome::Escaped);
        assert_eq!(
            wreath.ticks(),
            WREATH_GRACE_TICKS,
            "escaping leaves the tick counter unchanged"
        );
    }

    #[test]
    fn kill_at_exactly_wreath_cry_ticks() {
        let mut wreath = Wreath::attach(1);
        for tick in 1..WREATH_CRY_TICKS {
            assert_eq!(wreath.tick(false), WreathOutcome::Crying, "tick {tick}");
        }
        assert_eq!(wreath.tick(false), WreathOutcome::Killed);
        assert_eq!(wreath.ticks(), WREATH_CRY_TICKS);
    }

    #[test]
    fn still_crying_one_tick_before_the_kill() {
        let mut wreath = Wreath::attach(2);
        for _ in 0..(WREATH_CRY_TICKS - 1) {
            assert_eq!(wreath.tick(false), WreathOutcome::Crying);
        }
        assert_eq!(wreath.ticks(), WREATH_CRY_TICKS - 1);
        assert!(wreath.is_biting(), "past grace the timer is already biting");
    }

    #[test]
    fn quiet_during_grace() {
        let mut wreath = Wreath::attach(3);
        assert!(!wreath.is_biting());
        for _ in 0..WREATH_GRACE_TICKS {
            wreath.tick(false);
        }
        assert_eq!(wreath.ticks(), WREATH_GRACE_TICKS);
        assert!(!wreath.is_biting(), "grace is not over at exactly WREATH_GRACE_TICKS");
        wreath.tick(false);
        assert_eq!(wreath.ticks(), WREATH_GRACE_TICKS + 1);
        assert!(wreath.is_biting(), "the timer bites once grace is over");
    }

    #[test]
    fn tithe_cost_scales_and_saturates() {
        assert_eq!(tithe_cost(0), 0);
        assert_eq!(tithe_cost(5), 5);
        assert_eq!(tithe_cost(u32::MAX), u32::MAX, "saturates instead of overflowing");
    }

    #[test]
    fn tithe_slow_applies_expires_and_sets_speed_mul() {
        let mut slow = TitheSlow::default();
        assert!(!slow.is_slowed());
        assert!((slow.speed_mul() - 1.0).abs() < EPS);
        slow.apply();
        assert!(slow.is_slowed());
        assert!((slow.speed_mul() - TITHE_SLOW_MUL).abs() < EPS);
        for _ in 0..TITHE_SLOW_TICKS {
            slow.tick();
        }
        assert!(!slow.is_slowed());
        assert!((slow.speed_mul() - 1.0).abs() < EPS);
    }

    #[test]
    fn tithe_slow_apply_refreshes_the_timer() {
        let mut slow = TitheSlow::default();
        slow.apply();
        for _ in 0..30 {
            slow.tick();
        }
        slow.apply();
        for _ in 0..(TITHE_SLOW_TICKS - 1) {
            slow.tick();
        }
        assert!(slow.is_slowed(), "the refreshed slow outlasts the original window");
        assert!((slow.speed_mul() - TITHE_SLOW_MUL).abs() < EPS);
    }

    #[test]
    fn from_id_round_trips_all() {
        assert_eq!(GreaterCurse::all().len(), 3);
        for curse in GreaterCurse::all() {
            assert_eq!(GreaterCurse::from_id(curse.id()), Some(curse));
        }
    }

    #[test]
    fn from_id_rejects_stat_only_greater_curses() {
        assert_eq!(GreaterCurse::from_id("leaden_sky"), None);
        assert_eq!(GreaterCurse::from_id("the_hollow_choir"), None);
        assert_eq!(GreaterCurse::from_id("world_of_teeth"), None);
        assert_eq!(GreaterCurse::from_id(""), None);
    }

    #[test]
    fn hush_fog_mul_matches_the_content_fog_mod() {
        assert!((HUSH_FOG_MUL - 1.6).abs() < EPS);
    }
}
