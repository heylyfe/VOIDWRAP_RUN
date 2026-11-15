//! Radar modules (T-086): what a player is allowed to see, and how often.
//!
//! Pure deterministic logic with no server dependencies: safe to compile to
//! wasm for client-side prediction. The constants are the balance knobs; the
//! cooldowns tick down in the sim's fixed 20 Hz step, never on wall-clock time.

use protocol::Vec3;

/// Range of the gift arrow, in units.
pub const GIFT_ARROW_RANGE: f32 = 60.0;
/// Ticks between enemy-radar pings (10.0 s at 20 Hz).
pub const ENEMY_RADAR_COOLDOWN_TICKS: u32 = 200;
/// Ticks a ping stays visible (2.0 s at 20 Hz).
pub const ENEMY_RADAR_PING_TICKS: u32 = 40;
/// Radius of an enemy-radar ping, in units.
pub const ENEMY_RADAR_RADIUS: f32 = 30.0;
/// Radius of the ground-hazard overlay, in units.
pub const HAZARD_RADAR_RADIUS: f32 = 18.0;

/// The three purchasable modules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadarModule {
    /// Compass arrow to the nearest uncollected gift.
    GiftArrow,
    /// Short positional pulse over the run's permanents.
    EnemyRadar,
    /// Overlay of nearby ground hazards.
    HazardRadar,
}

impl RadarModule {
    /// Every module, in content order.
    pub fn all() -> [RadarModule; 3] {
        [
            RadarModule::GiftArrow,
            RadarModule::EnemyRadar,
            RadarModule::HazardRadar,
        ]
    }

    /// Map a shop item id to its module.
    pub fn from_id(id: &str) -> Option<RadarModule> {
        match id {
            "radar_gift_arrow" => Some(RadarModule::GiftArrow),
            "radar_enemy" => Some(RadarModule::EnemyRadar),
            "radar_hazard" => Some(RadarModule::HazardRadar),
            _ => None,
        }
    }

    /// The shop item id that grants this module.
    pub fn id(self) -> &'static str {
        match self {
            RadarModule::GiftArrow => "radar_gift_arrow",
            RadarModule::EnemyRadar => "radar_enemy",
            RadarModule::HazardRadar => "radar_hazard",
        }
    }

    /// Registry enemy ids this module counters (never empty).
    pub fn counters(self) -> &'static [&'static str] {
        match self {
            RadarModule::GiftArrow => &["lure", "static"],
            RadarModule::EnemyRadar => &["static", "overlook", "kite"],
            RadarModule::HazardRadar => &["coil", "lure"],
        }
    }
}

/// A player's owned modules and the enemy-radar cooldown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RadarState {
    /// One slot per [`RadarModule::all`] entry, `true` once granted.
    owned: [bool; 3],
    /// Ticks until the next enemy ping is allowed.
    cooldown: u32,
    /// Ticks the current ping stays visible (0 = no ping).
    ping: u32,
}

impl RadarState {
    /// Grant a module (idempotent).
    pub fn grant(&mut self, module: RadarModule) {
        self.owned[module as usize] = true;
    }

    /// True when the module is owned.
    pub fn has(&self, module: RadarModule) -> bool {
        self.owned[module as usize]
    }

    /// Advance one tick: the cooldown and the ping window both count down.
    pub fn tick(&mut self) {
        if self.cooldown > 0 {
            self.cooldown -= 1;
        }
        if self.ping > 0 {
            self.ping -= 1;
        }
    }

    /// Start a ping if the enemy radar is owned and off cooldown. Returns true when a ping started.
    pub fn fire(&mut self) -> bool {
        if !self.has(RadarModule::EnemyRadar) || self.cooldown > 0 {
            return false;
        }
        self.cooldown = ENEMY_RADAR_COOLDOWN_TICKS;
        self.ping = ENEMY_RADAR_PING_TICKS;
        true
    }

    /// True while a ping is visible.
    pub fn ping_active(&self) -> bool {
        self.ping > 0
    }

    /// Ticks until the next ping is allowed (0 when ready).
    pub fn cooldown_left(&self) -> u32 {
        self.cooldown
    }
}

/// The nearest gift to `from` within [`GIFT_ARROW_RANGE`], with its distance.
///
/// Only `+ - * / sqrt` are used, so the result is bit-identical native and wasm.
pub fn nearest_gift(from: Vec3, gifts: &[Vec3]) -> Option<(Vec3, f32)> {
    let range_sq = GIFT_ARROW_RANGE * GIFT_ARROW_RANGE;
    let mut best: Option<usize> = None;
    let mut best_d2 = range_sq;
    for (i, gift) in gifts.iter().enumerate() {
        let dx = gift.x - from.x;
        let dy = gift.y - from.y;
        let dz = gift.z - from.z;
        let d2 = dx * dx + dy * dy + dz * dz;
        if d2 <= best_d2 {
            best_d2 = d2;
            best = Some(i);
        }
    }
    best.map(|i| {
        let pos = gifts[i];
        (pos, best_d2.sqrt())
    })
}

/// Indices of `points` within `radius` of `from`, in input order.
pub fn within_radius(from: Vec3, points: &[Vec3], radius: f32) -> Vec<usize> {
    let radius_sq = radius * radius;
    let mut out = Vec::new();
    for (i, point) in points.iter().enumerate() {
        let dx = point.x - from.x;
        let dy = point.y - from.y;
        let dz = point.z - from.z;
        if dx * dx + dy * dy + dz * dz <= radius_sq {
            out.push(i);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3 { x, y, z }
    }

    #[test]
    fn ids_round_trip_and_reject_unknown() {
        for module in RadarModule::all() {
            assert_eq!(RadarModule::from_id(module.id()), Some(module));
        }
        assert_eq!(RadarModule::from_id("radar_nope"), None);
    }

    #[test]
    fn counters_are_never_empty() {
        for module in RadarModule::all() {
            assert!(!module.counters().is_empty());
        }
    }

    #[test]
    fn grant_is_idempotent_and_has_reflects_it() {
        let mut state = RadarState::default();
        assert!(!state.has(RadarModule::GiftArrow));
        state.grant(RadarModule::GiftArrow);
        state.grant(RadarModule::GiftArrow);
        assert!(state.has(RadarModule::GiftArrow));
        assert!(!state.has(RadarModule::EnemyRadar));
        assert!(!state.has(RadarModule::HazardRadar));
    }

    #[test]
    fn fire_requires_the_module() {
        let mut state = RadarState::default();
        assert!(!state.fire());
        assert!(!state.ping_active());
    }

    #[test]
    fn fire_refuses_while_cooling_down_and_succeeds_when_ready() {
        let mut state = RadarState::default();
        state.grant(RadarModule::EnemyRadar);
        assert!(state.fire());
        assert!(!state.fire());
        for _ in 0..ENEMY_RADAR_COOLDOWN_TICKS {
            state.tick();
        }
        assert_eq!(state.cooldown_left(), 0);
        assert!(state.fire());
    }

    #[test]
    fn cooldown_counts_down_to_zero() {
        let mut state = RadarState::default();
        state.grant(RadarModule::EnemyRadar);
        assert_eq!(state.cooldown_left(), 0);
        assert!(state.fire());
        for remaining in (0..ENEMY_RADAR_COOLDOWN_TICKS).rev() {
            assert_eq!(state.cooldown_left(), remaining + 1);
            state.tick();
        }
        assert_eq!(state.cooldown_left(), 0);
    }

    #[test]
    fn ping_is_active_for_exactly_ping_ticks() {
        let mut state = RadarState::default();
        state.grant(RadarModule::EnemyRadar);
        assert!(state.fire());
        for _ in 0..ENEMY_RADAR_PING_TICKS {
            assert!(state.ping_active());
            state.tick();
        }
        assert!(!state.ping_active());
    }

    #[test]
    fn nearest_gift_picks_closest_inside_range() {
        let from = v(0.0, 0.0, 0.0);
        assert!(nearest_gift(from, &[]).is_none());
        let far = v(GIFT_ARROW_RANGE + 1.0, 0.0, 0.0);
        assert!(nearest_gift(from, &[far]).is_none());
        let near = v(10.0, 0.0, 0.0);
        let mid = v(20.0, 0.0, 0.0);
        let (pos, dist) = nearest_gift(from, &[mid, far, near]).expect("gift inside range");
        assert!((pos.x - near.x).abs() < 1e-5);
        assert!((pos.y - near.y).abs() < 1e-5);
        assert!((pos.z - near.z).abs() < 1e-5);
        assert!((dist - 10.0).abs() < 1e-4);
    }

    #[test]
    fn within_radius_filters_and_keeps_order() {
        let from = v(0.0, 0.0, 0.0);
        let points = vec![v(5.0, 0.0, 0.0), v(50.0, 0.0, 0.0), v(0.0, 3.0, 4.0)];
        let hits = within_radius(from, &points, 10.0);
        assert_eq!(hits, vec![0, 2]);
        assert!(within_radius(from, &points, 0.0).is_empty());
    }
}
