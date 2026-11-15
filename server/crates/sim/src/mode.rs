//! Difficulty modes (T-085): the run-shaping knobs chosen in the lobby.
//!
//! Pure data and pure functions: no server dependency, no I/O, no wall-clock time and
//! no transcendental float functions, so this module compiles unchanged for
//! `wasm32-unknown-unknown` (see the crate header in `lib.rs`).

/// The four modes. `Standard` is the default and changes nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DifficultyMode {
    /// Normal pools, a pedestal vote every level. The default.
    #[default]
    Standard,
    /// Harder pools: one extra permanent copy allowed per enemy, enemies offered one
    /// level earlier, denser fog.
    Extreme,
    /// A shop every level, including level 1.
    Chaos,
    /// No vote: the server picks a pedestal option itself each level.
    Random,
}

impl DifficultyMode {
    /// Every mode, in content order.
    pub fn all() -> [DifficultyMode; 4] {
        [
            DifficultyMode::Standard,
            DifficultyMode::Extreme,
            DifficultyMode::Chaos,
            DifficultyMode::Random,
        ]
    }

    /// Map a lobby id (`"standard"`, `"extreme"`, `"chaos"`, `"random"`) to its mode.
    pub fn from_id(id: &str) -> Option<DifficultyMode> {
        DifficultyMode::all().iter().copied().find(|m| m.id() == id)
    }

    /// The lobby id this mode is chosen by.
    pub fn id(self) -> &'static str {
        match self {
            DifficultyMode::Standard => "standard",
            DifficultyMode::Extreme => "extreme",
            DifficultyMode::Chaos => "chaos",
            DifficultyMode::Random => "random",
        }
    }

    /// Extra permanent copies allowed per enemy (EXTREME +1, else 0).
    pub fn cap_bonus(self) -> u32 {
        match self {
            DifficultyMode::Extreme => 1,
            DifficultyMode::Standard | DifficultyMode::Chaos | DifficultyMode::Random => 0,
        }
    }

    /// Shift applied to every enemy's `min_level` (EXTREME -1, else 0). Never below 1.
    pub fn min_level_shift(self) -> i32 {
        match self {
            DifficultyMode::Extreme => -1,
            DifficultyMode::Standard | DifficultyMode::Chaos | DifficultyMode::Random => 0,
        }
    }

    /// Fog multiplier on top of the run's curse mods (EXTREME 1.25, else 1.0).
    pub fn fog_mul(self) -> f32 {
        match self {
            DifficultyMode::Extreme => 1.25,
            DifficultyMode::Standard | DifficultyMode::Chaos | DifficultyMode::Random => 1.0,
        }
    }

    /// True when the lobby shop opens every level, including level 1 (CHAOS).
    pub fn shop_every_level(self) -> bool {
        matches!(self, DifficultyMode::Chaos)
    }

    /// True when the pedestal vote is replaced by a server pick (RANDOM).
    pub fn forces_random_pick(self) -> bool {
        matches!(self, DifficultyMode::Random)
    }

    /// The suffix appended to the matchmaker `phase` string: `""` for STANDARD, else `":extreme"`,
    /// `":chaos"`, `":random"`.
    pub fn phase_suffix(self) -> &'static str {
        match self {
            DifficultyMode::Standard => "",
            DifficultyMode::Extreme => ":extreme",
            DifficultyMode::Chaos => ":chaos",
            DifficultyMode::Random => ":random",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DifficultyMode;

    #[test]
    fn all_is_content_order() {
        assert_eq!(
            DifficultyMode::all(),
            [
                DifficultyMode::Standard,
                DifficultyMode::Extreme,
                DifficultyMode::Chaos,
                DifficultyMode::Random,
            ]
        );
    }

    #[test]
    fn from_id_round_trips_all() {
        for mode in DifficultyMode::all() {
            assert_eq!(DifficultyMode::from_id(mode.id()), Some(mode));
        }
    }

    #[test]
    fn from_id_rejects_unknown() {
        assert!(DifficultyMode::from_id("nightmare").is_none());
    }

    #[test]
    fn default_is_standard() {
        assert_eq!(DifficultyMode::default(), DifficultyMode::Standard);
    }

    #[test]
    fn cap_bonus_per_mode() {
        assert_eq!(DifficultyMode::Standard.cap_bonus(), 0);
        assert_eq!(DifficultyMode::Extreme.cap_bonus(), 1);
        assert_eq!(DifficultyMode::Chaos.cap_bonus(), 0);
        assert_eq!(DifficultyMode::Random.cap_bonus(), 0);
    }

    #[test]
    fn min_level_shift_per_mode() {
        assert_eq!(DifficultyMode::Standard.min_level_shift(), 0);
        assert_eq!(DifficultyMode::Extreme.min_level_shift(), -1);
        assert_eq!(DifficultyMode::Chaos.min_level_shift(), 0);
        assert_eq!(DifficultyMode::Random.min_level_shift(), 0);
    }

    #[test]
    fn fog_mul_per_mode() {
        assert_eq!(DifficultyMode::Standard.fog_mul(), 1.0);
        assert_eq!(DifficultyMode::Extreme.fog_mul(), 1.25);
        assert_eq!(DifficultyMode::Chaos.fog_mul(), 1.0);
        assert_eq!(DifficultyMode::Random.fog_mul(), 1.0);
    }

    #[test]
    fn shop_every_level_per_mode() {
        assert!(!DifficultyMode::Standard.shop_every_level());
        assert!(!DifficultyMode::Extreme.shop_every_level());
        assert!(DifficultyMode::Chaos.shop_every_level());
        assert!(!DifficultyMode::Random.shop_every_level());
    }

    #[test]
    fn forces_random_pick_per_mode() {
        assert!(!DifficultyMode::Standard.forces_random_pick());
        assert!(!DifficultyMode::Extreme.forces_random_pick());
        assert!(!DifficultyMode::Chaos.forces_random_pick());
        assert!(DifficultyMode::Random.forces_random_pick());
    }

    #[test]
    fn phase_suffix_per_mode() {
        assert_eq!(DifficultyMode::Standard.phase_suffix(), "");
        assert_eq!(DifficultyMode::Extreme.phase_suffix(), ":extreme");
        assert_eq!(DifficultyMode::Chaos.phase_suffix(), ":chaos");
        assert_eq!(DifficultyMode::Random.phase_suffix(), ":random");
    }
}
