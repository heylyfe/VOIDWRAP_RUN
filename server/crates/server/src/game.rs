//! The game task: owns all simulation state and runs the authoritative tick loop.
//!
//! Input model: clients send one [`InputFrame`] per fixed physics step (1/PHYSICS_HZ s),
//! tagged with an increasing `seq`. Each 20 Hz tick, every player gets
//! `STEPS_PER_TICK` step tokens (capped, so short network jitter can catch up but a
//! client can never run faster than real time), and the server consumes that many
//! queued frames. Snapshots carry the last applied `seq` so the client can reconcile.
//!
//! Run flow (see [`crate::run`]): lobby on the spawn hub with a draft offer → every player
//! votes → a generated level → collect all normal gifts → golden gifts + beacon → lobby.
//! Tick order: movement → (playing) gifts → enemies → deaths/lives → beacon → snapshots.

use crate::content::Content;
use crate::run::Run;
use protocol::{
    BTN_ABILITY, BTN_INTERACT, BTN_JUMP, ClientMsg, ControllerState, InputFrame, MAX_DRAFT_PICKS, MAX_INPUTS_PER_MSG,
    MAX_NAME_LEN, PHASE_ENEMY_DRAFT, PHASE_OVER, PHASE_PLAYING, PHYSICS_HZ, PROTOCOL_VERSION, PlayerView, RunSummary,
    ServerMsg, TICK_HZ, Tuning, Vec3,
};
use sim::controller;
use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

pub type ConnId = u64;

/// Outbound frames buffered per connection before we consider the client too slow.
pub const OUTBOX_CAPACITY: usize = 64;
pub const MAX_PLAYERS: usize = 8;
/// One second of buffered input at most.
const MAX_QUEUED_INPUTS: usize = PHYSICS_HZ as usize;
const STEPS_PER_TICK: u32 = PHYSICS_HZ / TICK_HZ;
/// Token cap: up to 3 ticks' worth of catch-up after jitter.
const MAX_STEP_TOKENS: u32 = STEPS_PER_TICK * 3;
/// After this many ticks with no input, simulate neutral input so gravity keeps acting.
const STARVE_TICKS: u32 = 4;
const BUTTON_MASK: u8 = BTN_JUMP | BTN_ABILITY | BTN_INTERACT;
/// Players spawn spread around the shared spawn point so they never stack.
const SPAWN_RING: [(f32, f32); MAX_PLAYERS] = [
    (0.0, 0.0),
    (1.6, 0.0),
    (-1.6, 0.0),
    (0.0, 1.6),
    (0.0, -1.6),
    (1.6, 1.6),
    (-1.6, -1.6),
    (1.6, -1.6),
];

pub enum GameEvent {
    Connected { id: ConnId, out: mpsc::Sender<Vec<u8>> },
    Message { id: ConnId, msg: ClientMsg },
    Disconnected { id: ConnId },
}

struct Player {
    id: u32,
    name: String,
    state: ControllerState,
    inputs: VecDeque<InputFrame>,
    last_recv_seq: Option<u32>,
    last_applied_seq: u32,
    tokens: u32,
    starved_ticks: u32,
    alive: bool,
}

impl Player {
    fn centre(&self) -> Vec3 {
        Vec3::new(
            self.state.pos.x,
            self.state.pos.y + sim::tuning::PLAYER_HEIGHT / 2.0,
            self.state.pos.z,
        )
    }
}

struct Conn {
    out: mpsc::Sender<Vec<u8>>,
    player: Option<Player>,
}

pub struct Game {
    tick: u32,
    conns: BTreeMap<ConnId, Conn>,
    content: Content,
    run: Run,
    /// Draft votes of the current lobby, by connection.
    votes: BTreeMap<ConnId, Vec<u8>>,
    next_player_id: u32,
    /// Level every new run starts at (1 in production; `VOIDWRAP_START_LEVEL` for visual QA).
    start_level: u32,
    /// Seeds successive runs (draft offers and level layouts).
    seed: u32,
}

impl Game {
    pub fn starting_at(start_level: u32, seed: u32) -> Result<Self, String> {
        let content = Content::load()?;
        let start_level = start_level.max(1);
        let run = Run::new(&content, seed, start_level);
        Ok(Game {
            tick: 0,
            conns: BTreeMap::new(),
            content,
            run,
            votes: BTreeMap::new(),
            next_player_id: 1,
            start_level,
            seed,
        })
    }

    pub fn tick_count(&self) -> u32 {
        self.tick
    }

    pub fn player_count(&self) -> usize {
        self.conns.values().filter(|c| c.player.is_some()).count()
    }

    fn joined_ids(&self) -> Vec<ConnId> {
        self.conns
            .iter()
            .filter(|(_, c)| c.player.is_some())
            .map(|(id, _)| *id)
            .collect()
    }

    fn tuning(&self) -> Tuning {
        self.run.mods.tuning()
    }

    /// Spawn position for the n-th joined player.
    fn spawn_point(&self, slot: usize) -> Vec3 {
        let (dx, dz) = SPAWN_RING[slot % MAX_PLAYERS];
        let s = &self.run.level.spawn;
        Vec3::new(s.x + dx, s.y, s.z + dz)
    }

    pub fn handle(&mut self, ev: GameEvent) {
        match ev {
            GameEvent::Connected { id, out } => {
                self.conns.insert(id, Conn { out, player: None });
            }
            GameEvent::Disconnected { id } => {
                if let Some(Conn { player: Some(p), .. }) = self.conns.remove(&id) {
                    tracing::info!(conn = id, player = p.id, name = %p.name, "player left");
                }
                self.votes.remove(&id);
                self.maybe_resolve_draft();
            }
            GameEvent::Message { id, msg } => self.on_message(id, msg),
        }
    }

    fn send(&mut self, id: ConnId, msg: &ServerMsg) {
        let Some(conn) = self.conns.get(&id) else { return };
        let bytes = match protocol::encode(msg) {
            Ok(b) => b,
            Err(e) => {
                tracing::error!("encode failed: {e}");
                return;
            }
        };
        if conn.out.try_send(bytes).is_err() {
            // Outbox full (client not reading) or writer gone: drop the connection.
            tracing::warn!(conn = id, "dropping slow or closed connection");
            self.conns.remove(&id);
            self.votes.remove(&id);
        }
    }

    fn broadcast(&mut self, msg: &ServerMsg) {
        for id in self.joined_ids() {
            self.send(id, msg);
        }
    }

    /// Send an error and drop the connection.
    fn reject(&mut self, id: ConnId, code: u16, message: &str) {
        tracing::warn!(conn = id, code, "rejecting: {message}");
        self.send(
            id,
            &ServerMsg::Error {
                code,
                message: message.to_owned(),
            },
        );
        self.conns.remove(&id);
        self.votes.remove(&id);
    }

    fn draft_msg(&self) -> Option<ServerMsg> {
        let offer = self.run.offer.as_ref()?;
        (self.run.phase == PHASE_ENEMY_DRAFT).then(|| ServerMsg::Draft {
            enemy_picks: offer.enemy_picks,
            reward_picks: offer.reward_picks,
            cards: offer.cards.clone(),
        })
    }

    /// Everyone back on the spawn platform of the current level (hub or generated), alive, then tell them.
    fn respawn_all_and_broadcast(&mut self) {
        let ids = self.joined_ids();
        for (slot, id) in ids.iter().enumerate() {
            let at = self.spawn_point(slot);
            if let Some(p) = self.conns.get_mut(id).and_then(|c| c.player.as_mut()) {
                p.state = controller::spawn_state(at);
                p.alive = true;
                p.starved_ticks = 0;
            }
        }
        let level = ServerMsg::Level {
            data: self.run.level.clone(),
        };
        let info = ServerMsg::RunState { info: self.run.info() };
        self.broadcast(&level);
        self.broadcast(&info);
        if let Some(draft) = self.draft_msg() {
            self.broadcast(&draft);
        }
    }

    fn broadcast_run_state(&mut self) {
        let info = ServerMsg::RunState { info: self.run.info() };
        self.broadcast(&info);
    }

    /// Resolve the lobby draft once every joined player has voted.
    fn maybe_resolve_draft(&mut self) {
        if self.run.phase != PHASE_ENEMY_DRAFT || self.run.offer.is_none() {
            return;
        }
        let ids = self.joined_ids();
        if ids.is_empty() || !ids.iter().all(|id| self.votes.contains_key(id)) {
            return;
        }
        let votes: Vec<Vec<u8>> = ids.iter().filter_map(|id| self.votes.get(id).cloned()).collect();
        self.votes.clear();
        self.run.resolve(&self.content, &votes);
        tracing::info!(
            level = self.run.level_num,
            enemies = ?self.run.drafted_enemies,
            upgrades = ?self.run.upgrades,
            curses = ?self.run.curses,
            "draft resolved; level generated"
        );
        self.respawn_all_and_broadcast();
    }

    fn new_run(&mut self) {
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.run = Run::new(&self.content, self.seed, self.start_level);
        self.votes.clear();
        self.respawn_all_and_broadcast();
    }

    fn on_message(&mut self, id: ConnId, msg: ClientMsg) {
        let Some(conn) = self.conns.get_mut(&id) else { return };
        let joined = conn.player.is_some();
        match msg {
            ClientMsg::Hello { protocol_version, name } => {
                if joined {
                    return self.reject(id, 400, "duplicate hello");
                }
                if protocol_version != PROTOCOL_VERSION {
                    return self.reject(id, 426, "protocol version mismatch; reload the page");
                }
                if self.player_count() >= MAX_PLAYERS {
                    return self.reject(id, 503, "server full");
                }
                let name = sanitize_name(&name);
                let player_id = self.next_player_id;
                self.next_player_id = self.next_player_id.wrapping_add(1).max(1);
                let spawn = self.spawn_point(self.player_count());
                tracing::info!(conn = id, player = player_id, name = %name, "player joined");
                // Joining mid-level: you wait in the lobby of the next level (dead until then); in the lobby you play.
                let alive = self.run.phase == PHASE_ENEMY_DRAFT;
                if let Some(conn) = self.conns.get_mut(&id) {
                    conn.player = Some(Player {
                        id: player_id,
                        name,
                        state: controller::spawn_state(spawn),
                        inputs: VecDeque::new(),
                        last_recv_seq: None,
                        last_applied_seq: 0,
                        tokens: 0,
                        starved_ticks: 0,
                        alive,
                    });
                }
                self.send(
                    id,
                    &ServerMsg::Welcome {
                        protocol_version: PROTOCOL_VERSION,
                        player_id,
                        tick: self.tick,
                    },
                );
                let level = ServerMsg::Level {
                    data: self.run.level.clone(),
                };
                self.send(id, &level);
                let info = ServerMsg::RunState { info: self.run.info() };
                self.send(id, &info);
                if let Some(draft) = self.draft_msg() {
                    self.send(id, &draft);
                }
            }
            _ if !joined => self.reject(id, 400, "hello required first"),
            ClientMsg::Inputs { frames } => {
                if frames.len() > MAX_INPUTS_PER_MSG as usize {
                    return self.reject(id, 400, "too many inputs in one message");
                }
                if let Some(p) = conn.player.as_mut() {
                    for mut f in frames {
                        if p.last_recv_seq.is_some_and(|last| f.seq <= last) {
                            continue; // duplicate or reordered
                        }
                        if p.inputs.len() >= MAX_QUEUED_INPUTS {
                            break; // flooding: drop the excess
                        }
                        f.buttons &= BUTTON_MASK;
                        p.last_recv_seq = Some(f.seq);
                        p.inputs.push_back(f);
                    }
                }
            }
            ClientMsg::Ping { t } => self.send(id, &ServerMsg::Pong { t }),
            ClientMsg::DraftPick { picks } => {
                if picks.len() > MAX_DRAFT_PICKS as usize {
                    return self.reject(id, 400, "too many draft picks");
                }
                match self.run.validate_picks(&picks) {
                    Ok(()) => {
                        self.votes.insert(id, picks);
                        self.maybe_resolve_draft();
                    }
                    Err(why) => self.send(
                        id,
                        &ServerMsg::Error {
                            code: 422,
                            message: why.to_owned(),
                        },
                    ),
                }
            }
            ClientMsg::Restart => {
                // A new run starts once the run is over (the whole team is down); otherwise ignore.
                if self.run.phase == PHASE_OVER {
                    tracing::info!(conn = id, "new run");
                    self.new_run();
                }
            }
        }
    }

    /// Advance the simulation one 20 Hz tick and broadcast snapshots.
    pub fn tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        let tuning = self.tuning();

        // 1. Movement (lobby and playing).
        for p in self.conns.values_mut().filter_map(|c| c.player.as_mut()) {
            p.tokens = (p.tokens + STEPS_PER_TICK).min(MAX_STEP_TOKENS);
            if !p.alive {
                // Dead players' inputs are acknowledged and dropped so the client's queue drains.
                if let Some(last) = p.inputs.back() {
                    p.last_applied_seq = last.seq;
                }
                p.inputs.clear();
                continue;
            }
            let mut stepped = 0;
            while p.tokens > 0 {
                let Some(f) = p.inputs.pop_front() else { break };
                controller::step_with(&mut p.state, &f, &self.run.world, &tuning);
                p.last_applied_seq = f.seq;
                p.tokens -= 1;
                stepped += 1;
            }
            if stepped == 0 {
                p.starved_ticks += 1;
                if p.starved_ticks >= STARVE_TICKS {
                    let neutral = InputFrame {
                        seq: p.last_applied_seq,
                        move_x: 0,
                        move_z: 0,
                        buttons: 0,
                    };
                    for _ in 0..STEPS_PER_TICK.min(p.tokens) {
                        controller::step_with(&mut p.state, &neutral, &self.run.world, &tuning);
                        p.tokens -= 1;
                    }
                }
            } else {
                p.starved_ticks = 0;
            }
        }

        if self.run.phase == PHASE_PLAYING {
            self.tick_playing();
        } else {
            // Lobby / over: the hub is a safe space - falling off just puts you back on the spawn.
            let kill_y = self.run.world.kill_y;
            let ids = self.joined_ids();
            for (slot, id) in ids.iter().enumerate() {
                let at = self.spawn_point(slot);
                if let Some(p) = self.conns.get_mut(id).and_then(|c| c.player.as_mut())
                    && p.state.pos.y < kill_y
                {
                    p.state = controller::spawn_state(at);
                }
            }
        }

        // Snapshots.
        let players: Vec<PlayerView> = self
            .conns
            .values()
            .filter_map(|c| c.player.as_ref())
            .map(|p| PlayerView {
                id: p.id,
                pos: p.state.pos.clone(),
                vel: p.state.vel.clone(),
                alive: p.alive,
            })
            .collect();
        let enemies = self.run.enemy_views();
        for id in self.joined_ids() {
            let Some(p) = self.conns.get(&id).and_then(|c| c.player.as_ref()) else {
                continue;
            };
            let msg = ServerMsg::Snapshot {
                tick: self.tick,
                level: self.run.level.level,
                last_input_seq: p.last_applied_seq,
                you: p.state.clone(),
                players: players.clone(),
                enemies: enemies.clone(),
                gifts_taken: self.run.gifts_taken.clone(),
            };
            self.send(id, &msg);
        }
    }

    fn tick_playing(&mut self) {
        // Gifts (shared by the team; golden ones unlock after the normal ones).
        let currency_before = self.run.currency;
        for conn in self.conns.values() {
            if let Some(p) = conn.player.as_ref().filter(|p| p.alive) {
                self.run.collect_gifts(&p.state.pos);
            }
        }
        let mut info_changed = self.run.currency != currency_before;

        // Enemies chase the living.
        let targets: Vec<Vec3> = self
            .conns
            .values()
            .filter_map(|c| c.player.as_ref())
            .filter(|p| p.alive)
            .map(Player::centre)
            .collect();
        self.run.step_enemies(&targets);

        // Deaths: a spare life respawns you at the level start, otherwise you are out for this run.
        let ids = self.joined_ids();
        let mut deaths: Vec<(ConnId, u8)> = Vec::new();
        for (slot, id) in ids.iter().enumerate() {
            let at = self.spawn_point(slot);
            let Some(p) = self
                .conns
                .get_mut(id)
                .and_then(|c| c.player.as_mut())
                .filter(|p| p.alive)
            else {
                continue;
            };
            let Some(cause) = self.run.hazard(&p.state.pos) else {
                continue;
            };
            if self.run.lives > 0 {
                self.run.lives -= 1;
                p.state = controller::spawn_state(at);
                info_changed = true;
                tracing::info!(conn = id, cause, lives = self.run.lives, "life lost");
            } else {
                p.alive = false;
                p.state.vel = Vec3::ZERO;
                deaths.push((*id, cause));
            }
        }
        for (id, cause) in deaths {
            tracing::info!(conn = id, cause, level = self.run.level_num, "player died");
            let summary = RunSummary {
                level_reached: self.run.level_num,
                gifts_collected: self.run.gifts_collected,
                cause,
            };
            self.send(id, &ServerMsg::RunOver { summary });
        }

        let anyone_alive = self.conns.values().filter_map(|c| c.player.as_ref()).any(|p| p.alive);
        if !anyone_alive && self.player_count() > 0 {
            self.run.end();
            self.broadcast_run_state();
            return;
        }

        // Beacon: a living player in the open beacon takes the team back to the lobby for the next level.
        let through_beacon = self
            .conns
            .values()
            .filter_map(|c| c.player.as_ref())
            .any(|p| p.alive && self.run.beacon_reached(&p.state.pos));
        if through_beacon {
            self.run.finish_level(&self.content);
            tracing::info!(
                next_level = self.run.level_num,
                currency = self.run.currency,
                "level cleared"
            );
            self.votes.clear();
            self.respawn_all_and_broadcast();
            return;
        }
        if info_changed {
            self.broadcast_run_state();
        }
    }
}

/// Trim, drop control characters, cap length; never empty.
fn sanitize_name(raw: &str) -> String {
    let name: String = raw
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME_LEN as usize)
        .collect();
    let name = name.trim();
    if name.is_empty() {
        "blocky".to_owned()
    } else {
        name.to_owned()
    }
}

/// Runs the game until every sender is gone.
pub async fn run(mut rx: mpsc::Receiver<GameEvent>, mut game: Game) {
    let period = Duration::from_millis(1000 / u64::from(TICK_HZ));
    let mut interval = tokio::time::interval(period);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Burst);
    let mut window_start = Instant::now();
    let mut window_ticks = 0u32;
    loop {
        tokio::select! {
            biased;
            _ = interval.tick() => {
                game.tick();
                window_ticks += 1;
                tracing::trace!(tick = game.tick_count(), "tick");
                let elapsed = window_start.elapsed();
                if elapsed >= Duration::from_secs(1) {
                    let hz = f64::from(window_ticks) / elapsed.as_secs_f64();
                    tracing::info!("tick {} | {hz:.1} Hz | players {}", game.tick_count(), game.player_count());
                    window_start = Instant::now();
                    window_ticks = 0;
                }
            }
            ev = rx.recv() => match ev {
                Some(ev) => game.handle(ev),
                None => break,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::valid_picks;
    use protocol::GIFT_GOLDEN;

    fn game() -> Game {
        Game::starting_at(1, 42).unwrap()
    }

    fn connect(game: &mut Game, id: ConnId) -> mpsc::Receiver<Vec<u8>> {
        let (tx, rx) = mpsc::channel(OUTBOX_CAPACITY);
        game.handle(GameEvent::Connected { id, out: tx });
        rx
    }

    fn hello(game: &mut Game, id: ConnId) {
        game.handle(GameEvent::Message {
            id,
            msg: ClientMsg::Hello {
                protocol_version: PROTOCOL_VERSION,
                name: "  tester\u{7}  ".into(),
            },
        });
    }

    fn drain(rx: &mut mpsc::Receiver<Vec<u8>>) -> Vec<ServerMsg> {
        let mut out = vec![];
        while let Ok(b) = rx.try_recv() {
            out.push(protocol::decode::<ServerMsg>(&b).unwrap());
        }
        out
    }

    fn inputs(seqs: std::ops::Range<u32>, move_x: i8) -> ClientMsg {
        ClientMsg::Inputs {
            frames: seqs
                .map(|seq| InputFrame {
                    seq,
                    move_x,
                    move_z: 0,
                    buttons: 0xff,
                })
                .collect(),
        }
    }

    fn player(g: &mut Game, id: ConnId) -> &mut Player {
        g.conns.get_mut(&id).unwrap().player.as_mut().unwrap()
    }

    fn vote(g: &mut Game, id: ConnId) {
        let picks = valid_picks(&g.run);
        g.handle(GameEvent::Message {
            id,
            msg: ClientMsg::DraftPick { picks },
        });
    }

    #[test]
    fn hello_lands_in_the_lobby_with_a_draft_offer() {
        let mut g = game();
        let mut rx = connect(&mut g, 1);
        hello(&mut g, 1);
        let msgs = drain(&mut rx);
        assert!(matches!(msgs[0], ServerMsg::Welcome { player_id: 1, .. }));
        assert!(matches!(&msgs[1], ServerMsg::Level { data } if data.gifts.is_empty()));
        assert!(matches!(&msgs[2], ServerMsg::RunState { info } if info.phase == PHASE_ENEMY_DRAFT));
        assert!(matches!(&msgs[3], ServerMsg::Draft { enemy_picks: 2, reward_picks: 1, cards } if cards.len() == 7));
        g.tick();
        assert!(matches!(&drain(&mut rx)[..], [ServerMsg::Snapshot { tick: 1, players, .. }] if players.len() == 1));
        assert_eq!(g.conns[&1].player.as_ref().unwrap().name, "tester");
    }

    #[test]
    fn draft_resolves_only_when_every_player_voted() {
        let mut g = game();
        let mut rx1 = connect(&mut g, 1);
        hello(&mut g, 1);
        let mut rx2 = connect(&mut g, 2);
        hello(&mut g, 2);
        vote(&mut g, 1);
        assert_eq!(g.run.phase, PHASE_ENEMY_DRAFT);
        vote(&mut g, 2);
        assert_eq!(g.run.phase, PHASE_PLAYING);
        for rx in [&mut rx1, &mut rx2] {
            let msgs = drain(rx);
            assert!(
                msgs.iter()
                    .any(|m| matches!(m, ServerMsg::Level { data } if data.gifts.len() > 5))
            );
            assert!(
                msgs.iter()
                    .any(|m| matches!(m, ServerMsg::RunState { info } if info.phase == PHASE_PLAYING))
            );
        }
        // Players start spread around the spawn, not stacked.
        let a = g.conns[&1].player.as_ref().unwrap().state.pos.clone();
        let b = g.conns[&2].player.as_ref().unwrap().state.pos.clone();
        assert_ne!(a, b);
    }

    #[test]
    fn invalid_picks_get_an_error_not_a_kick() {
        let mut g = game();
        let mut rx = connect(&mut g, 1);
        hello(&mut g, 1);
        drain(&mut rx);
        g.handle(GameEvent::Message {
            id: 1,
            msg: ClientMsg::DraftPick { picks: vec![0] },
        });
        assert!(matches!(drain(&mut rx)[..], [ServerMsg::Error { code: 422, .. }]));
        assert_eq!(g.player_count(), 1);
        assert_eq!(g.run.phase, PHASE_ENEMY_DRAFT);
    }

    #[test]
    fn a_disconnecting_holdout_does_not_block_the_draft() {
        let mut g = game();
        let _rx1 = connect(&mut g, 1);
        hello(&mut g, 1);
        let _rx2 = connect(&mut g, 2);
        hello(&mut g, 2);
        vote(&mut g, 1);
        g.handle(GameEvent::Disconnected { id: 2 });
        assert_eq!(g.run.phase, PHASE_PLAYING);
    }

    #[test]
    fn full_loop_gifts_golden_beacon_back_to_lobby() {
        let mut g = game();
        let mut rx = connect(&mut g, 1);
        hello(&mut g, 1);
        vote(&mut g, 1);
        drain(&mut rx);
        let gifts = g.run.level.gifts.clone();
        // Normal gifts first, then the golden ones (currency).
        for gift in gifts
            .iter()
            .filter(|g| g.kind != GIFT_GOLDEN)
            .chain(gifts.iter().filter(|g| g.kind == GIFT_GOLDEN))
        {
            player(&mut g, 1).state.pos = Vec3::new(gift.pos.x, gift.pos.y - 0.9, gift.pos.z);
            for e in &mut g.run.enemies {
                e.active_in = 1_000; // keep enemies dormant for this test
            }
            g.tick();
        }
        assert!(g.run.currency > 0);
        let beacon = g.run.level.beacon.clone();
        player(&mut g, 1).state.pos = beacon;
        g.tick();
        assert_eq!(g.run.phase, PHASE_ENEMY_DRAFT);
        assert_eq!(g.run.level_num, 2);
        let msgs = drain(&mut rx);
        assert!(
            msgs.iter()
                .any(|m| matches!(m, ServerMsg::Draft { enemy_picks: 0, .. }))
        );
        assert!(
            msgs.iter()
                .any(|m| matches!(m, ServerMsg::RunState { info } if info.currency > 0 && info.level == 2))
        );
    }

    #[test]
    fn falling_ends_the_run_and_restart_returns_to_the_lobby() {
        let mut g = game();
        let mut rx = connect(&mut g, 1);
        hello(&mut g, 1);
        vote(&mut g, 1);
        drain(&mut rx);
        let kill_y = g.run.world.kill_y;
        let lives = g.run.lives;
        for _ in 0..=lives {
            player(&mut g, 1).state.pos.y = kill_y - 5.0;
            g.tick();
        }
        let msgs = drain(&mut rx);
        assert!(
            msgs.iter()
                .any(|m| matches!(m, ServerMsg::RunOver { summary } if summary.cause == protocol::DEATH_VOID))
        );
        assert_eq!(g.run.phase, PHASE_OVER);
        g.handle(GameEvent::Message {
            id: 1,
            msg: ClientMsg::Restart,
        });
        assert_eq!(g.run.phase, PHASE_ENEMY_DRAFT);
        assert!(player(&mut g, 1).alive);
        assert!(
            drain(&mut rx)
                .iter()
                .any(|m| matches!(m, ServerMsg::Draft { enemy_picks: 2, .. }))
        );
    }

    #[test]
    fn falling_off_the_hub_respawns_instead_of_killing() {
        let mut g = game();
        let mut rx = connect(&mut g, 1);
        hello(&mut g, 1);
        let kill_y = g.run.world.kill_y;
        player(&mut g, 1).state.pos.y = kill_y - 1.0;
        g.tick();
        assert!(player(&mut g, 1).alive);
        assert!(player(&mut g, 1).state.pos.y > kill_y);
        assert!(!drain(&mut rx).iter().any(|m| matches!(m, ServerMsg::RunOver { .. })));
    }

    #[test]
    fn restart_is_ignored_while_the_run_is_live() {
        let mut g = game();
        let _rx = connect(&mut g, 1);
        hello(&mut g, 1);
        vote(&mut g, 1);
        let level = g.run.level.clone();
        g.handle(GameEvent::Message {
            id: 1,
            msg: ClientMsg::Restart,
        });
        assert_eq!(g.run.level, level);
    }

    #[test]
    fn messages_before_hello_are_rejected() {
        let mut g = game();
        let mut rx = connect(&mut g, 1);
        g.handle(GameEvent::Message {
            id: 1,
            msg: inputs(0..1, 0),
        });
        assert!(matches!(drain(&mut rx)[..], [ServerMsg::Error { code: 400, .. }]));
        assert!(g.conns.is_empty());
    }

    #[test]
    fn wrong_protocol_version_rejected() {
        let mut g = game();
        let mut rx = connect(&mut g, 1);
        g.handle(GameEvent::Message {
            id: 1,
            msg: ClientMsg::Hello {
                protocol_version: 999,
                name: "x".into(),
            },
        });
        assert!(matches!(drain(&mut rx)[..], [ServerMsg::Error { code: 426, .. }]));
    }

    #[test]
    fn input_rate_is_capped_to_real_time() {
        let mut g = game();
        let mut rx = connect(&mut g, 1);
        hello(&mut g, 1);
        g.handle(GameEvent::Message {
            id: 1,
            msg: inputs(1..31, 127),
        });
        g.tick();
        let p = g.conns[&1].player.as_ref().unwrap();
        assert_eq!(p.last_applied_seq, STEPS_PER_TICK);
        assert_eq!(p.inputs.len(), 30 - STEPS_PER_TICK as usize);
        drain(&mut rx);
    }

    #[test]
    fn duplicate_and_old_seqs_ignored_and_buttons_masked() {
        let mut g = game();
        let _rx = connect(&mut g, 1);
        hello(&mut g, 1);
        g.handle(GameEvent::Message {
            id: 1,
            msg: inputs(5..8, 0),
        });
        g.handle(GameEvent::Message {
            id: 1,
            msg: inputs(3..9, 0),
        });
        let p = g.conns[&1].player.as_ref().unwrap();
        let seqs: Vec<u32> = p.inputs.iter().map(|f| f.seq).collect();
        assert_eq!(seqs, vec![5, 6, 7, 8]);
        assert!(p.inputs.iter().all(|f| f.buttons == BUTTON_MASK));
    }

    #[test]
    fn oversized_input_batch_and_draft_spam_rejected() {
        let mut g = game();
        let _rx = connect(&mut g, 1);
        hello(&mut g, 1);
        g.handle(GameEvent::Message {
            id: 1,
            msg: inputs(0..MAX_INPUTS_PER_MSG + 1, 0),
        });
        assert!(g.conns.is_empty());
        let _rx = connect(&mut g, 2);
        hello(&mut g, 2);
        g.handle(GameEvent::Message {
            id: 2,
            msg: ClientMsg::DraftPick { picks: vec![0; 50] },
        });
        assert!(g.conns.is_empty());
    }

    #[test]
    fn slow_client_is_dropped_not_buffered_forever() {
        let mut g = game();
        let _rx = connect(&mut g, 1); // never drained
        hello(&mut g, 1);
        for _ in 0..OUTBOX_CAPACITY + 5 {
            g.tick();
        }
        assert!(g.conns.is_empty());
    }

    #[test]
    fn server_full() {
        let mut g = game();
        let mut rxs = vec![];
        for id in 0..=MAX_PLAYERS as u64 {
            rxs.push(connect(&mut g, id));
            hello(&mut g, id);
        }
        assert_eq!(g.player_count(), MAX_PLAYERS);
        assert!(matches!(
            drain(rxs.last_mut().unwrap())[..],
            [ServerMsg::Error { code: 503, .. }]
        ));
    }

    #[test]
    fn name_sanitized() {
        assert_eq!(sanitize_name("\n\t"), "blocky");
        assert_eq!(sanitize_name(&"x".repeat(100)).len(), MAX_NAME_LEN as usize);
    }
}
