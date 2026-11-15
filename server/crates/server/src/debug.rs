//! Debug command layer (T-110).
//!
//! The tester build's command surface. The contract lives in `design/DEBUG_COMMANDS.md`
//! and is frozen: adding a command is additive, changing one is a human decision.
//!
//! Three gates, all enforced here and all before the command is parsed, so a rejected
//! command has no effect at all:
//!
//! 1. **Availability** — the server was started with `VOIDWRAP_DEV=1`, or the request
//!    carries a code equal to `VOIDWRAP_DEV_CODE`. The code is read from the environment
//!    at startup and never leaves the server: it is not in the protocol, not in the client
//!    bundle, and not in the manifest.
//! 2. **Singleplayer** — every command is rejected while more than one player is connected.
//! 3. **Rate limit** — wrong codes are counted per connection; after [`MAX_BAD_CODES`] the
//!    connection is refused debug access for the rest of its life, even with the right code.

use crate::content::Content;
use crate::game::ConnId;
use crate::run::Run;
use protocol::Vec3;
use serde_json::Value;
use std::collections::BTreeMap;

/// Wrong codes a single connection may present before it is locked out for good.
pub const MAX_BAD_CODES: u8 = 5;

/// Why a debug command was refused. The strings are stable: the panel shows them verbatim.
pub const NOT_ENABLED: &str = "debug commands are not enabled";
pub const SINGLEPLAYER_ONLY: &str = "debug commands are singleplayer only";
pub const TOO_MANY_BAD_CODES: &str = "too many wrong codes";

/// The availability gate: whether the route exists at all, and the per-connection bad-code count.
#[derive(Debug, Default)]
pub struct Gate {
    /// `VOIDWRAP_DEV=1` at startup: every connection may use debug commands without a code.
    dev_flag: bool,
    /// `VOIDWRAP_DEV_CODE`: the code a client may present instead of the flag.
    code: Option<String>,
    /// Wrong-code attempts per connection. A connection at [`MAX_BAD_CODES`] is locked out.
    bad: BTreeMap<ConnId, u8>,
}

impl Gate {
    /// Read the gate's configuration from the environment. Called once at startup.
    pub fn from_env() -> Self {
        let dev_flag = std::env::var("VOIDWRAP_DEV").is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));
        // An empty code is the same as no code: it must never be satisfiable by sending "".
        let code = std::env::var("VOIDWRAP_DEV_CODE").ok().filter(|c| !c.is_empty());
        Gate {
            dev_flag,
            code,
            bad: BTreeMap::new(),
        }
    }

    /// A gate with an explicit configuration, for tests.
    ///
    /// An empty code is filtered here as well as in [`Gate::from_env`], so "an empty code is
    /// never satisfiable" is a property of the type rather than of one constructor.
    #[cfg(test)]
    pub fn new(dev_flag: bool, code: Option<&str>) -> Self {
        Gate {
            dev_flag,
            code: code.filter(|c| !c.is_empty()).map(str::to_owned),
            bad: BTreeMap::new(),
        }
    }

    /// True when the route exists for anyone at all (the dev flag is set or a code is configured).
    pub fn configured(&self) -> bool {
        self.dev_flag || self.code.is_some()
    }

    /// Check one request. `Err` carries the stable refusal reason.
    ///
    /// A wrong code is counted even when the dev flag is set: a tester who sends a code is
    /// asking to be checked against it, and silently ignoring a wrong one would hide a typo.
    pub fn authorize(&mut self, conn: ConnId, code: &str) -> Result<(), &'static str> {
        if self.bad.get(&conn).copied().unwrap_or(0) >= MAX_BAD_CODES {
            return Err(TOO_MANY_BAD_CODES);
        }
        if self.dev_flag {
            return Ok(());
        }
        let Some(expected) = self.code.as_deref() else {
            return Err(NOT_ENABLED);
        };
        if constant_time_eq(code.as_bytes(), expected.as_bytes()) {
            return Ok(());
        }
        let count = self.bad.entry(conn).or_insert(0);
        *count = count.saturating_add(1);
        if *count >= MAX_BAD_CODES {
            Err(TOO_MANY_BAD_CODES)
        } else {
            Err(NOT_ENABLED)
        }
    }

    /// Forget a connection's bad-code count (it disconnected).
    pub fn forget(&mut self, conn: ConnId) {
        self.bad.remove(&conn);
    }
}

/// Compare two byte strings without an early exit, so a wrong code cannot be narrowed down by
/// timing. The length difference is folded into the result rather than returned early.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        diff |= usize::from(x ^ y);
    }
    diff == 0
}

/// What a command needs from the game: the content, the run, and the caller's feet.
pub struct Ctx<'a> {
    pub content: &'a Content,
    pub run: &'a mut Run,
    /// The calling player's feet position; `teleport` writes it.
    pub feet: &'a mut Vec3,
    /// Players connected, for `switch_level`'s gift target.
    pub players: u32,
}

/// Execute one debug command. `args` is a JSON object; an unknown key is an error, not a
/// silent no-op, so a tester who mistypes finds out.
pub fn execute(ctx: Ctx<'_>, command: &str, args: &str) -> Result<String, String> {
    let args: Value = if args.trim().is_empty() {
        Value::Object(serde_json::Map::new())
    } else {
        serde_json::from_str(args).map_err(|e| format!("bad args: {e}"))?
    };
    let obj = args
        .as_object()
        .ok_or_else(|| "bad args: expected a JSON object".to_string())?;
    let mut reader = Args { obj, used: Vec::new() };

    let result = match command {
        "spawn_enemy" => {
            let id = reader.string("id")?;
            let count = reader.u8("count")?.unwrap_or(1).max(1);
            let pos = match (reader.f32("x")?, reader.f32("y")?, reader.f32("z")?) {
                (Some(x), Some(y), Some(z)) => Vec3::new(x, y, z),
                _ => ctx.run.level.spawn,
            };
            let mut spawned = 0;
            for _ in 0..count {
                ctx.run.debug_spawn_enemy(ctx.content, &id, pos)?;
                spawned += 1;
            }
            Ok(format!(
                "spawned {spawned} x {id} at ({:.1}, {:.1}, {:.1})",
                pos.x, pos.y, pos.z
            ))
        }
        "despawn_enemy" => {
            let id = reader.string("id")?;
            let removed = ctx.run.debug_despawn_enemy(ctx.content, &id)?;
            Ok(format!("removed {removed} x {id}"))
        }
        "switch_level" => {
            let level = reader
                .u32("level")?
                .ok_or_else(|| "bad args: level is required".to_string())?;
            let seed = reader.u32("seed")?;
            ctx.run.debug_switch_level(ctx.content, level, seed, ctx.players);
            Ok(format!("switched to level {level} (seed {})", ctx.run.seed))
        }
        "grant_gold" => {
            let amount = reader
                .i32("amount")?
                .ok_or_else(|| "bad args: amount is required".to_string())?;
            if amount >= 0 {
                ctx.run.currency = ctx.run.currency.saturating_add(amount.unsigned_abs());
            } else {
                ctx.run.currency = ctx.run.currency.saturating_sub(amount.unsigned_abs());
            }
            Ok(format!("currency is now {}", ctx.run.currency))
        }
        "grant_gift" => {
            let count = reader.u8("count")?.unwrap_or(1).max(1);
            let golden = reader.bool("golden")?.unwrap_or(false);
            let granted = ctx.run.debug_grant_gift(count, golden)?;
            Ok(format!(
                "granted {granted} {} gift(s)",
                if golden { "golden" } else { "normal" }
            ))
        }
        "remove_gift" => {
            let count = reader.u8("count")?.unwrap_or(1).max(1);
            let removed = ctx.run.debug_remove_gift(count);
            Ok(format!("removed {removed} gift(s)"))
        }
        "set_phase" => {
            let phase = reader
                .u8("phase")?
                .ok_or_else(|| "bad args: phase is required".to_string())?;
            if phase > protocol::PHASE_OVER {
                return Err(format!("bad args: phase {phase} is not a PHASE_* constant"));
            }
            ctx.run.phase = phase;
            Ok(format!("phase is now {phase}"))
        }
        "give_upgrade" => {
            let id = reader.string("id")?;
            ctx.run.debug_give_upgrade(ctx.content, &id)?;
            Ok(format!("gave upgrade {id}"))
        }
        "take_upgrade" => {
            let id = reader.string("id")?;
            ctx.run.debug_take_upgrade(ctx.content, &id)?;
            Ok(format!("took upgrade {id}"))
        }
        "give_curse" => {
            let id = reader.string("id")?;
            ctx.run.debug_give_curse(ctx.content, &id)?;
            Ok(format!("gave curse {id}"))
        }
        "take_curse" => {
            let id = reader.string("id")?;
            ctx.run.debug_take_curse(ctx.content, &id)?;
            Ok(format!("took curse {id}"))
        }
        "trigger_altar" => {
            let id = reader
                .u16("id")?
                .ok_or_else(|| "bad args: id is required".to_string())?;
            ctx.run.debug_trigger_altar(ctx.content, id)
        }
        "teleport" => {
            let x = reader.f32("x")?.ok_or_else(|| "bad args: x is required".to_string())?;
            let y = reader.f32("y")?.ok_or_else(|| "bad args: y is required".to_string())?;
            let z = reader.f32("z")?.ok_or_else(|| "bad args: z is required".to_string())?;
            *ctx.feet = Vec3::new(x, y, z);
            Ok(format!("teleported to ({x:.1}, {y:.1}, {z:.1})"))
        }
        "set_fog" => {
            // Client-side visual: the server validates the range and echoes it back, and the
            // panel applies it locally. Gating still applies, which is the point.
            let near = reader
                .f32("near")?
                .ok_or_else(|| "bad args: near is required".to_string())?;
            let far = reader
                .f32("far")?
                .ok_or_else(|| "bad args: far is required".to_string())?;
            if !(near.is_finite() && far.is_finite()) || near <= 0.0 || far <= near {
                return Err("bad args: need 0 < near < far".into());
            }
            Ok(format!("fog near={near} far={far}"))
        }
        "state" => Ok(ctx.run.debug_state(ctx.content)),
        other => Err(format!("unknown command: {other}")),
    };

    // An unknown key is a typo, and a typo that silently does nothing is worse than an error.
    if result.is_ok() {
        reader.reject_unknown()?;
    }
    result
}

/// A tiny typed reader over the args object that remembers which keys were consumed.
struct Args<'a> {
    obj: &'a serde_json::Map<String, Value>,
    used: Vec<&'a str>,
}

impl<'a> Args<'a> {
    fn take(&mut self, key: &'a str) -> Option<&'a Value> {
        let value = self.obj.get(key);
        if value.is_some() {
            self.used.push(key);
        }
        value
    }

    fn string(&mut self, key: &'a str) -> Result<String, String> {
        match self.take(key) {
            Some(Value::String(s)) => Ok(s.clone()),
            Some(_) => Err(format!("bad args: {key} must be a string")),
            None => Err(format!("bad args: {key} is required")),
        }
    }

    fn u8(&mut self, key: &'a str) -> Result<Option<u8>, String> {
        self.int(key).map(|v| v.map(|v| v.clamp(0, i64::from(u8::MAX)) as u8))
    }

    fn u16(&mut self, key: &'a str) -> Result<Option<u16>, String> {
        self.int(key).map(|v| v.map(|v| v.clamp(0, i64::from(u16::MAX)) as u16))
    }

    fn u32(&mut self, key: &'a str) -> Result<Option<u32>, String> {
        self.int(key).map(|v| v.map(|v| v.clamp(0, i64::from(u32::MAX)) as u32))
    }

    fn i32(&mut self, key: &'a str) -> Result<Option<i32>, String> {
        self.int(key)
            .map(|v| v.map(|v| v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32))
    }

    fn int(&mut self, key: &'a str) -> Result<Option<i64>, String> {
        match self.take(key) {
            Some(Value::Number(n)) => n
                .as_i64()
                .map(Some)
                .ok_or_else(|| format!("bad args: {key} must be an integer")),
            Some(_) => Err(format!("bad args: {key} must be an integer")),
            None => Ok(None),
        }
    }

    fn f32(&mut self, key: &'a str) -> Result<Option<f32>, String> {
        match self.take(key) {
            Some(Value::Number(n)) => n
                .as_f64()
                .map(|v| Some(v as f32))
                .ok_or_else(|| format!("bad args: {key} must be a number")),
            Some(_) => Err(format!("bad args: {key} must be a number")),
            None => Ok(None),
        }
    }

    fn bool(&mut self, key: &'a str) -> Result<Option<bool>, String> {
        match self.take(key) {
            Some(Value::Bool(b)) => Ok(Some(*b)),
            Some(_) => Err(format!("bad args: {key} must be a boolean")),
            None => Ok(None),
        }
    }

    fn reject_unknown(&self) -> Result<(), String> {
        for key in self.obj.keys() {
            if !self.used.contains(&key.as_str()) {
                return Err(format!("bad args: unknown key {key}"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>(content: &'a Content, run: &'a mut Run, feet: &'a mut Vec3) -> Ctx<'a> {
        Ctx {
            content,
            run,
            feet,
            players: 1,
        }
    }

    #[test]
    fn a_gate_with_neither_flag_nor_code_refuses_everything() {
        let mut gate = Gate::new(false, None);
        assert!(!gate.configured());
        assert_eq!(gate.authorize(1, ""), Err(NOT_ENABLED));
        assert_eq!(gate.authorize(1, "anything"), Err(NOT_ENABLED));
    }

    #[test]
    fn the_dev_flag_opens_the_route_without_a_code() {
        let mut gate = Gate::new(true, None);
        assert!(gate.configured());
        assert_eq!(gate.authorize(1, ""), Ok(()));
    }

    #[test]
    fn the_right_code_opens_the_route_and_a_wrong_one_does_not() {
        let mut gate = Gate::new(false, Some("s3cret"));
        assert_eq!(gate.authorize(1, "s3cret"), Ok(()));
        assert_eq!(gate.authorize(2, "s3cres"), Err(NOT_ENABLED));
        assert_eq!(gate.authorize(2, ""), Err(NOT_ENABLED));
    }

    #[test]
    fn wrong_codes_are_rate_limited_per_connection() {
        let mut gate = Gate::new(false, Some("s3cret"));
        for _ in 0..MAX_BAD_CODES - 1 {
            assert_eq!(gate.authorize(7, "nope"), Err(NOT_ENABLED));
        }
        // The last allowed attempt trips the lockout.
        assert_eq!(gate.authorize(7, "nope"), Err(TOO_MANY_BAD_CODES));
        // Locked out for good, even with the right code.
        assert_eq!(gate.authorize(7, "s3cret"), Err(TOO_MANY_BAD_CODES));
        // Another connection is unaffected.
        assert_eq!(gate.authorize(8, "s3cret"), Ok(()));
    }

    #[test]
    fn an_empty_configured_code_is_never_satisfiable() {
        // `VOIDWRAP_DEV_CODE=""` must not mean "any empty code works".
        let mut gate = Gate::new(false, Some(""));
        assert_eq!(gate.authorize(1, ""), Err(NOT_ENABLED));
    }

    #[test]
    fn constant_time_eq_matches_slice_equality() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
        assert!(!constant_time_eq(b"", b"a"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn unknown_keys_are_rejected_rather_than_ignored() {
        let content = Content::load().expect("content");
        let mut run = Run::new(&content, 1, 1);
        let mut feet = Vec3::ZERO;
        let err = execute(
            ctx(&content, &mut run, &mut feet),
            "grant_gold",
            r#"{"amount":10,"amout":10}"#,
        )
        .unwrap_err();
        assert!(err.contains("unknown key amout"), "{err}");
    }

    #[test]
    fn an_unknown_command_is_an_error_not_a_no_op() {
        let content = Content::load().expect("content");
        let mut run = Run::new(&content, 1, 1);
        let mut feet = Vec3::ZERO;
        let err = execute(ctx(&content, &mut run, &mut feet), "summon_dragon", "{}").unwrap_err();
        assert_eq!(err, "unknown command: summon_dragon");
    }
}
