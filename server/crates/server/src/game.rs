//! The game task: owns all simulation state and runs the authoritative tick loop.
//!
//! Input model: clients send one [`InputFrame`] per fixed physics step (1/PHYSICS_HZ s),
//! tagged with an increasing `seq`. Each 20 Hz tick, every player gets
//! `STEPS_PER_TICK` step tokens (capped, so short network jitter can catch up but a
//! client can never run faster than real time), and the server consumes that many
//! queued frames. Snapshots carry the last applied `seq` so the client can reconcile.
//!
//! Tick order: player movement -> gift pickup -> enemies move -> deaths (void, enemy touch)
//! -> beacon (advance level) -> snapshots. Game rules live in [`crate::run::Run`].

use crate::run::Run;
use protocol::{
    BTN_ABILITY, BTN_INTERACT, BTN_JUMP, ClientMsg, ControllerState, InputFrame, MAX_INPUTS_PER_MSG, MAX_NAME_LEN,
    PHYSICS_HZ, PROTOCOL_VERSION, PlayerView, RunSummary, ServerMsg, TICK_HZ, Vec3,
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
    /// Put the player back on `spawn`, alive. Input bookkeeping is kept so seqs stay monotonic.
    fn respawn(&mut self, spawn: &Vec3) {
        self.state = controller::spawn_state(spawn.clone());
        self.alive = true;
        self.starved_ticks = 0;
    }

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
    run: Run,
    next_player_id: u32,
}

impl Default for Game {
    fn default() -> Self {
        Self::new()
    }
}

impl Game {
    pub fn new() -> Self {
        Game {
            tick: 0,
            conns: BTreeMap::new(),
            run: Run::start(),
            next_player_id: 1,
        }
    }

    pub fn tick_count(&self) -> u32 {
        self.tick
    }

    pub fn player_count(&self) -> usize {
        self.conns.values().filter(|c| c.player.is_some()).count()
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
                tracing::info!(conn = id, player = player_id, name = %name, "player joined");
                if let Some(conn) = self.conns.get_mut(&id) {
                    conn.player = Some(Player {
                        id: player_id,
                        name,
                        state: controller::spawn_state(self.run.level.spawn.clone()),
                        inputs: VecDeque::new(),
                        last_recv_seq: None,
                        last_applied_seq: 0,
                        tokens: 0,
                        starved_ticks: 0,
                        alive: true,
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
                let level = self.run.level.clone();
                self.send(id, &ServerMsg::Level { data: level });
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
            ClientMsg::Restart => {
                // A new run starts only once the whole team is down; otherwise ignore.
                let anyone_alive = self.conns.values().filter_map(|c| c.player.as_ref()).any(|p| p.alive);
                if !anyone_alive {
                    tracing::info!(conn = id, "new run");
                    self.run = Run::start();
                    self.respawn_all_and_send_level();
                }
            }
        }
    }

    /// Respawn every player on the current level's spawn and send everyone the level.
    fn respawn_all_and_send_level(&mut self) {
        let spawn = self.run.level.spawn.clone();
        for p in self.conns.values_mut().filter_map(|c| c.player.as_mut()) {
            p.respawn(&spawn);
        }
        let level = ServerMsg::Level {
            data: self.run.level.clone(),
        };
        for id in self.joined_ids() {
            self.send(id, &level);
        }
    }

    fn joined_ids(&self) -> Vec<ConnId> {
        self.conns
            .iter()
            .filter(|(_, c)| c.player.is_some())
            .map(|(id, _)| *id)
            .collect()
    }

    /// Advance the simulation one 20 Hz tick and broadcast snapshots.
    pub fn tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);

        // 1. Movement.
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
                controller::step(&mut p.state, &f, &self.run.world);
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
                        controller::step(&mut p.state, &neutral, &self.run.world);
                        p.tokens -= 1;
                    }
                }
            } else {
                p.starved_ticks = 0;
            }
        }

        // 2. Gifts (shared by the team).
        for conn in self.conns.values() {
            if let Some(p) = conn.player.as_ref().filter(|p| p.alive) {
                self.run.collect_gifts(&p.state.pos);
            }
        }

        // 3. Enemies chase the living.
        let targets: Vec<Vec3> = self
            .conns
            .values()
            .filter_map(|c| c.player.as_ref())
            .filter(|p| p.alive)
            .map(Player::centre)
            .collect();
        self.run.step_enemies(&targets);

        // 4. Deaths.
        let mut deaths: Vec<(ConnId, u8)> = Vec::new();
        for (id, conn) in &mut self.conns {
            let Some(p) = conn.player.as_mut().filter(|p| p.alive) else {
                continue;
            };
            if let Some(cause) = self.run.hazard(&p.state.pos) {
                p.alive = false;
                p.state.vel = Vec3::ZERO;
                deaths.push((*id, cause));
            }
        }
        for (id, cause) in deaths {
            tracing::info!(conn = id, cause, level = self.run.number(), "player died");
            let summary = RunSummary {
                level_reached: self.run.number(),
                gifts_collected: self.run.gifts_collected,
                cause,
            };
            self.send(id, &ServerMsg::RunOver { summary });
        }

        // 5. Beacon: a living player in the open beacon takes the whole team onward.
        let through_beacon = self
            .conns
            .values()
            .filter_map(|c| c.player.as_ref())
            .any(|p| p.alive && self.run.beacon_reached(&p.state.pos));
        if through_beacon {
            self.run.advance();
            tracing::info!(level = self.run.number(), "level cleared");
            self.respawn_all_and_send_level();
        }

        // 6. Snapshots.
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
                level: self.run.number(),
                last_input_seq: p.last_applied_seq,
                you: p.state.clone(),
                players: players.clone(),
                enemies: enemies.clone(),
                gifts_taken: self.run.gifts_taken.clone(),
            };
            self.send(id, &msg);
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
pub async fn run(mut rx: mpsc::Receiver<GameEvent>) {
    let mut game = Game::new();
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

    #[test]
    fn hello_gets_welcome_and_level_then_snapshots() {
        let mut g = Game::new();
        let mut rx = connect(&mut g, 1);
        hello(&mut g, 1);
        let msgs = drain(&mut rx);
        assert!(matches!(msgs[0], ServerMsg::Welcome { player_id: 1, .. }));
        assert!(matches!(msgs[1], ServerMsg::Level { .. }));
        g.tick();
        let msgs = drain(&mut rx);
        assert!(matches!(&msgs[..], [ServerMsg::Snapshot { tick: 1, players, .. }] if players.len() == 1));
        assert_eq!(g.conns[&1].player.as_ref().unwrap().name, "tester");
    }

    #[test]
    fn messages_before_hello_are_rejected() {
        let mut g = Game::new();
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
        let mut g = Game::new();
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
        let mut g = Game::new();
        let mut rx = connect(&mut g, 1);
        hello(&mut g, 1);
        // Flood 30 frames at once; one tick may only consume STEPS_PER_TICK of them.
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
        let mut g = Game::new();
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
    fn oversized_input_batch_rejected() {
        let mut g = Game::new();
        let _rx = connect(&mut g, 1);
        hello(&mut g, 1);
        g.handle(GameEvent::Message {
            id: 1,
            msg: inputs(0..MAX_INPUTS_PER_MSG + 1, 0),
        });
        assert!(g.conns.is_empty());
    }

    #[test]
    fn slow_client_is_dropped_not_buffered_forever() {
        let mut g = Game::new();
        let _rx = connect(&mut g, 1); // never drained
        hello(&mut g, 1);
        for _ in 0..OUTBOX_CAPACITY + 5 {
            g.tick();
        }
        assert!(g.conns.is_empty());
    }

    #[test]
    fn server_full() {
        let mut g = Game::new();
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
    fn starved_player_still_falls() {
        let mut g = Game::new();
        let _rx = connect(&mut g, 1);
        hello(&mut g, 1);
        g.conns.get_mut(&1).unwrap().player.as_mut().unwrap().state.pos.y = 10.0;
        for _ in 0..STARVE_TICKS + 2 {
            g.tick();
        }
        assert!(g.conns[&1].player.as_ref().unwrap().state.pos.y < 10.0);
    }

    fn player(g: &mut Game, id: ConnId) -> &mut Player {
        g.conns.get_mut(&id).unwrap().player.as_mut().unwrap()
    }

    #[test]
    fn falling_into_the_void_ends_the_run_and_restart_begins_a_new_one() {
        let mut g = Game::new();
        let mut rx = connect(&mut g, 1);
        hello(&mut g, 1);
        drain(&mut rx);
        let kill_y = g.run.world.kill_y;
        player(&mut g, 1).state.pos.y = kill_y - 5.0;
        g.tick();
        let died = drain(&mut rx).into_iter().any(|m| {
            matches!(m, ServerMsg::RunOver { summary } if summary.level_reached == 1 && summary.cause == protocol::DEATH_VOID)
        });
        assert!(died);
        assert!(!player(&mut g, 1).alive);
        // Dead players do not die twice.
        g.tick();
        assert!(!drain(&mut rx).iter().any(|m| matches!(m, ServerMsg::RunOver { .. })));
        g.handle(GameEvent::Message {
            id: 1,
            msg: ClientMsg::Restart,
        });
        assert!(player(&mut g, 1).alive);
        assert!(
            drain(&mut rx)
                .iter()
                .any(|m| matches!(m, ServerMsg::Level { data } if data.level == 1))
        );
    }

    #[test]
    fn restart_is_ignored_while_alive() {
        let mut g = Game::new();
        let _rx = connect(&mut g, 1);
        hello(&mut g, 1);
        player(&mut g, 1).state.pos.x = 3.25;
        g.handle(GameEvent::Message {
            id: 1,
            msg: ClientMsg::Restart,
        });
        assert_eq!(player(&mut g, 1).state.pos.x, 3.25);
    }

    #[test]
    fn gifts_then_beacon_advance_the_level() {
        let mut g = Game::new();
        let mut rx = connect(&mut g, 1);
        hello(&mut g, 1);
        let gifts = g.run.level.gifts.clone();
        for gift in &gifts {
            player(&mut g, 1).state.pos = Vec3::new(gift.pos.x, gift.pos.y - 0.9, gift.pos.z);
            g.tick();
        }
        assert!(g.run.all_gifts_taken());
        let beacon = g.run.level.beacon.clone();
        player(&mut g, 1).state.pos = beacon;
        g.tick();
        assert_eq!(g.run.number(), 2);
        assert_eq!(g.run.gifts_collected, 5);
        assert!(
            drain(&mut rx)
                .iter()
                .any(|m| matches!(m, ServerMsg::Level { data } if data.level == 2))
        );
        let spawn = g.run.level.spawn.clone();
        assert_eq!(player(&mut g, 1).state.pos, spawn);
    }

    #[test]
    fn name_sanitized() {
        assert_eq!(sanitize_name("\n\t"), "blocky");
        assert_eq!(sanitize_name(&"x".repeat(100)).len(), MAX_NAME_LEN as usize);
    }
}
