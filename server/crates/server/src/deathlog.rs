//! Optional NDJSON death log, for diagnosing why a bot (or a player) died.
//!
//! Off unless `VOIDWRAP_DEATH_LOG` names a file path. When it is set, every death
//! appends one JSON object per line:
//!
//! ```json
//! {"tick":812,"level":3,"player":2,"cause":0,"x":4.5,"y":-17.2,"z":-9.0,
//!  "vx":0.0,"vy":-14.8,"vz":0.0,"kill_y":-16.0,"lives":1,"out":false,
//!  "note":"void","seed":7,"ground_dist":0.0,"ground_inset":0.12}
//! ```
//!
//! `cause` is the protocol death constant (`DEATH_VOID` = 0, `DEATH_ENEMY` = 1).
//! `out` is true when the death ended the run for that player (no spare lives left)
//! rather than costing a shared life.
//!
//! `ground_dist` and `ground_inset` are what separate the two ways a player falls,
//! and they are measured at the **last position where the player had ground
//! contact**, not at the death position. That distinction matters: a fall from a
//! platform at y=0 to a kill plane at y=-16 takes about a second, during which a
//! player moving 6 u/s drifts 6 u sideways. Measuring at the death position would
//! report that drift as distance from the platform and make every fall look like an
//! edge fall. Measuring where they left the ground does not.
//!
//! - `ground_dist` is the horizontal distance from that last grounded position to
//!   the nearest platform footprint. It is ~0 whenever the player was standing on
//!   something, which is the normal case.
//! - `ground_inset` is how far *inside* that platform's footprint the position was:
//!   the distance to the nearest of its four sides. A player who walked off the
//!   edge left from `inset` ~0. A player who fell through a seam left from well
//!   inside the platform, so a large `inset` on a void death is the signature of a
//!   collision bug rather than a navigation one.
//!
//! This exists because the playthrough harness could see *that* its bots died but
//! not *where* or *why*: the snapshot carries position, not cause, and the server's
//! `tracing` output is not machine-readable. Adding a field to the snapshot would
//! have meant a protocol change for a diagnostic; a side-channel file does not.
//!
//! The log is a diagnostic, never a gameplay input: nothing reads it back, and a
//! failure to open or write the file is logged once and then ignored.

use std::fs::File;
use std::io::Write;
use std::sync::{Mutex, OnceLock};

use protocol::Vec3;

/// The open log file, or `None` when `VOIDWRAP_DEATH_LOG` is unset.
fn sink() -> &'static Option<Mutex<File>> {
    static SINK: OnceLock<Option<Mutex<File>>> = OnceLock::new();
    SINK.get_or_init(|| {
        let path = std::env::var("VOIDWRAP_DEATH_LOG").ok().filter(|p| !p.is_empty())?;
        match File::create(&path) {
            Ok(file) => {
                tracing::info!(path = %path, "death log enabled");
                Some(Mutex::new(file))
            }
            Err(err) => {
                tracing::warn!(path = %path, %err, "death log could not be opened; deaths will not be logged");
                None
            }
        }
    })
}

/// Open the log now, if it is configured.
///
/// Called once at startup so the file exists even when nothing dies: a harness can
/// then tell "the log is on and there were no deaths" from "the log was never on",
/// which a lazily-created file cannot express.
pub fn init() {
    let _ = sink();
}

/// One death, as written to the log.
pub struct Death<'a> {
    pub tick: u32,
    pub level: u32,
    pub player: u32,
    pub cause: u8,
    pub pos: Vec3,
    pub vel: Vec3,
    pub kill_y: f32,
    pub lives: u32,
    /// True when this death ended the player's run rather than costing a shared life.
    pub out: bool,
    /// Free-form tag for the caller's own context (e.g. "void", "enemy", "spare").
    pub note: &'a str,
    /// The run seed, so a death can be replayed against the same level.
    pub seed: u32,
    /// Horizontal distance from the last grounded position to the nearest platform footprint.
    pub ground_dist: f32,
    /// How far inside that platform's footprint the last grounded position was.
    pub ground_inset: f32,
}

/// Append one death to the log. A no-op when the log is disabled.
pub fn record(d: Death<'_>) {
    let Some(sink) = sink() else { return };
    let Ok(mut file) = sink.lock() else { return };
    let line = format!(
        concat!(
            "{{\"tick\":{},\"level\":{},\"player\":{},\"cause\":{},\"x\":{:.3},\"y\":{:.3},\"z\":{:.3},",
            "\"vx\":{:.3},\"vy\":{:.3},\"vz\":{:.3},\"kill_y\":{:.3},\"lives\":{},\"out\":{},",
            "\"note\":\"{}\",\"seed\":{},\"ground_dist\":{:.3},\"ground_inset\":{:.3}}}\n"
        ),
        d.tick,
        d.level,
        d.player,
        d.cause,
        d.pos.x,
        d.pos.y,
        d.pos.z,
        d.vel.x,
        d.vel.y,
        d.vel.z,
        d.kill_y,
        d.lives,
        d.out,
        d.note,
        d.seed,
        d.ground_dist,
        d.ground_inset,
    );
    // A diagnostic must never take the tick loop down.
    if let Err(err) = file.write_all(line.as_bytes()) {
        tracing::warn!(%err, "death log write failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_is_a_noop_when_disabled() {
        // No `VOIDWRAP_DEATH_LOG` in the test environment: this must not panic and
        // must not create a file.
        record(Death {
            tick: 1,
            level: 1,
            player: 1,
            cause: 0,
            pos: Vec3::ZERO,
            vel: Vec3::ZERO,
            kill_y: -16.0,
            lives: 0,
            out: true,
            note: "test",
            seed: 1,
            ground_dist: 0.0,
            ground_inset: 0.0,
        });
    }
}
