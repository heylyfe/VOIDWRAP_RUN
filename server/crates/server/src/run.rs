//! Run state: the current level, the team's shared gift progress, and the enemies.
//! Pure game rules on top of `sim`; no I/O, so it is fully unit-testable.

use protocol::{DEATH_ENEMY, DEATH_VOID, EnemyView, LevelData, Vec3};
use sim::levels;
use sim::rules::{self, Enemy};
use sim::world::World;

pub struct Run {
    pub level: LevelData,
    pub world: World,
    /// Gift ids collected on the current level (shared by the whole team).
    pub gifts_taken: Vec<u16>,
    /// Gifts collected over the whole run.
    pub gifts_collected: u32,
    pub enemies: Vec<Enemy>,
}

impl Run {
    /// A fresh run at level 1.
    #[cfg(test)]
    pub fn start() -> Self {
        Self::at_level(1, 0)
    }

    /// A fresh run starting at level `n` (dev/QA aid; clamped to >= 1).
    pub fn start_at(n: u32) -> Self {
        Self::at_level(n.max(1), 0)
    }

    fn at_level(n: u32, gifts_collected: u32) -> Self {
        let level = levels::level(n);
        let world = World::from_level(&level);
        let enemies = levels::enemy_spawns(n)
            .into_iter()
            .zip(1u32..)
            .map(|((kind, pos), id)| rules::spawn_enemy(id, kind, pos))
            .collect();
        Run {
            level,
            world,
            gifts_taken: Vec::new(),
            gifts_collected,
            enemies,
        }
    }

    pub fn number(&self) -> u32 {
        self.level.level
    }

    /// Advance to the next level, keeping the run's gift total.
    pub fn advance(&mut self) {
        *self = Self::at_level(self.number().saturating_add(1), self.gifts_collected);
    }

    pub fn all_gifts_taken(&self) -> bool {
        self.gifts_taken.len() >= self.level.gifts.len()
    }

    /// Collect every untaken gift the player at `feet` touches.
    pub fn collect_gifts(&mut self, feet: &Vec3) {
        for gift in &self.level.gifts {
            if !self.gifts_taken.contains(&gift.id) && rules::gift_touched(feet, gift) {
                self.gifts_taken.push(gift.id);
                self.gifts_collected = self.gifts_collected.saturating_add(1);
            }
        }
    }

    /// `targets` are the centres of living players.
    pub fn step_enemies(&mut self, targets: &[Vec3]) {
        for e in &mut self.enemies {
            rules::step_enemy(e, targets);
        }
    }

    /// The death cause for a living player standing at `feet`, if something kills them this tick.
    pub fn hazard(&self, feet: &Vec3) -> Option<u8> {
        if feet.y < self.world.kill_y {
            Some(DEATH_VOID)
        } else if self
            .enemies
            .iter()
            .any(|e| e.active_in == 0 && rules::enemy_touches(e, feet))
        {
            // Dormant enemies (spawn grace period) are harmless: grace is a fairness window.
            Some(DEATH_ENEMY)
        } else {
            None
        }
    }

    /// The beacon only takes you onward once every gift on the level is collected.
    pub fn beacon_reached(&self, feet: &Vec3) -> bool {
        self.all_gifts_taken() && rules::beacon_reached(feet, &self.level.beacon)
    }

    pub fn enemy_views(&self) -> Vec<EnemyView> {
        self.enemies
            .iter()
            .map(|e| EnemyView {
                id: e.id,
                kind: e.kind,
                pos: e.pos.clone(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collecting_every_gift_opens_the_beacon_and_advance_keeps_the_total() {
        let mut run = Run::start();
        assert!(!run.beacon_reached(&run.level.beacon.clone()));
        let gifts = run.level.gifts.clone();
        for g in &gifts {
            run.collect_gifts(&Vec3::new(g.pos.x, g.pos.y - 0.9, g.pos.z));
        }
        // Touching the same gift twice does not double count.
        run.collect_gifts(&Vec3::new(gifts[0].pos.x, gifts[0].pos.y - 0.9, gifts[0].pos.z));
        assert_eq!(run.gifts_collected, 5);
        assert!(run.all_gifts_taken());
        assert!(run.beacon_reached(&run.level.beacon.clone()));
        run.advance();
        assert_eq!(run.number(), 2);
        assert_eq!(run.gifts_collected, 5);
        assert!(run.gifts_taken.is_empty());
    }

    #[test]
    fn void_and_enemies_kill() {
        let run = Run::start();
        let below = Vec3::new(0.0, run.world.kill_y - 1.0, 0.0);
        assert_eq!(run.hazard(&below), Some(DEATH_VOID));
        let e = run.enemies.first().expect("level 1 has a chaser").pos.clone();
        let touching = Vec3::new(e.x, e.y - 0.9, e.z);
        assert_eq!(run.hazard(&touching), None, "dormant enemies are harmless");
        let mut run = run;
        run.enemies[0].active_in = 0;
        assert_eq!(run.hazard(&touching), Some(DEATH_ENEMY));
        assert_eq!(run.hazard(&run.level.spawn.clone()), None);
    }
}
