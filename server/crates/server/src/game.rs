//! The game task: owns all simulation state and runs the authoritative tick loop.
//!
//! Input model: clients send one [`InputFrame`] per fixed physics step (1/PHYSICS_HZ s),
//! tagged with an increasing `seq`. Each 20 Hz tick, every player gets
//! `STEPS_PER_TICK` step tokens (capped, so short network jitter can catch up but a
//! client can never run faster than real time), and the server consumes that many
//! queued frames. Snapshots carry the last applied `seq` so the client can reconcile.
//!
//! Run flow (see [`crate::run`]): lobby on the spawn hub with a draft offer (and, from level 2 on, the shop)
//! -> every player votes -> a generated level -> collect all normal gifts -> golden gifts + beacon -> lobby.
//! During the collapse/exit window every connected player votes on one permanent addition (T-078); the
//! vote closes on its own 20 s countdown, early once every joined player has balloted, or when the level ends.
//! Tick order: movement (altar uses on rising-edge BTN_INTERACT) -> (playing) vote countdown -> gifts -> hazards -> deaths/lives -> beacon -> end check -> snapshots.
//!
//! Multiplayer: a player who enters the open beacon is FINISHED and one who runs out of lives is DEAD; both
//! become invisible spectators. The level ends when nobody is ACTIVE: back to the lobby if anyone finished,
//! otherwise the run is over. Solo is the same rule with one player.

use crate::content::Content;
use crate::matchmaker_client::GameStateHandle;
use crate::run::{Run, ShopError, ShopPurchase};
use protocol::{
    BTN_ABILITY, BTN_INTERACT, BTN_JUMP, CARD_ENEMY, CLASS_GLIDER, CLASS_STRIKER, ClientMsg, ControllerState,
    DEATH_VOID, EVENT_NONE, InputFrame, MAX_DRAFT_PICKS, MAX_INPUTS_PER_MSG, MAX_NAME_LEN, NOTICE_ALTAR, NOTICE_BOUGHT,
    NOTICE_EVENT, PHASE_ENEMY_DRAFT, PHASE_OVER, PHASE_PLAYING, PHYSICS_HZ, PLAYER_ACTIVE, PLAYER_DEAD,
    PLAYER_FINISHED, PLAYER_WAITING, PROTOCOL_VERSION, PlayerView, RunSummary, ServerMsg, ShopItem, TICK_HZ, Vec3,
};
use sim::controller;
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
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
    /// PLAYER_ACTIVE / PLAYER_FINISHED / PLAYER_DEAD / PLAYER_WAITING.
    mode: u8,
    /// How this player last died (for the run summary).
    cause: u8,
    /// The class the player chose (CLASS_*); copied into the controller state at every spawn/respawn.
    class: u8,
    /// Ticks of OVERTUNED left (Coil's shockwave, Peal's ring). Peal's ring is lethal only while set.
    overtuned_ticks: u16,
    /// Ticks of Blight's infection left. Slows the victim and pauses Rewind's and Kite's timers.
    infected_ticks: u16,
    /// True on the tick a jump pad launched this player (cleanses Blight, escapes Lure).
    pad_launched: bool,
}

impl Player {
    fn active(&self) -> bool {
        self.mode == PLAYER_ACTIVE
    }
}

struct Conn {
    out: mpsc::Sender<Vec<u8>>,
    player: Option<Player>,
}

/// Spawn state at `at` with the player's class baked into the controller state.
fn player_spawn_state(at: Vec3, class: u8) -> ControllerState {
    let mut state = controller::spawn_state(at);
    state.class = class;
    state
}

/// Map a CLASS_* protocol constant to the class id the enemy registry filters on.
fn class_id_str(class: u8) -> Option<&'static str> {
    match class {
        CLASS_STRIKER => Some("striker"),
        protocol::CLASS_GRAPPLER => Some("grappler"),
        protocol::CLASS_BULWARK => Some("bulwark"),
        CLASS_GLIDER => Some("glider"),
        _ => None,
    }
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

    /// Player ids of every joined player, ascending by connection order.
    fn joined_player_ids(&self) -> Vec<u32> {
        self.conns
            .values()
            .filter_map(|c| c.player.as_ref().map(|p| p.id))
            .collect()
    }

    /// Distinct class ids present in the lobby (the vote pool refuses enemies that ignore any of them).
    fn lobby_class_ids(&self) -> Vec<&'static str> {
        let mut classes: Vec<&'static str> = self
            .conns
            .values()
            .filter_map(|c| c.player.as_ref())
            .filter_map(|p| class_id_str(p.class))
            .collect();
        classes.sort_unstable();
        classes.dedup();
        classes
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
                // Draft votes are connection-scoped and drop with the connection; vote-session
                // ballots are keyed by player id and survive (T-078).
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
            // A leaver may have been the only missing vote. Recursion is bounded: `conns`
            // shrinks with every removal, and resolving the draft leaves the draft phase.
            self.maybe_resolve_draft();
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
        // Same as in `send`: recursion is bounded because `conns` shrinks.
        self.maybe_resolve_draft();
    }

    fn draft_msg(&self) -> Option<ServerMsg> {
        let offer = self.run.offer.as_ref()?;
        (self.run.phase == PHASE_ENEMY_DRAFT).then(|| ServerMsg::Draft {
            enemy_picks: offer.enemy_picks,
            reward_picks: offer.reward_picks,
            cards: offer.cards.clone(),
        })
    }

    /// The lobby shop offer as a message, when a shop is open.
    fn shop_msg(&self) -> Option<ServerMsg> {
        let shop = self.run.shop.as_ref()?;
        Some(ServerMsg::Shop {
            items: shop
                .iter()
                .map(|item| ShopItem {
                    id: item.id.clone(),
                    cost: item.cost,
                    stock: item.stock,
                })
                .collect(),
        })
    }

    /// The VoteOpen message for the open session, if any.
    fn vote_open_msg(&self) -> Option<ServerMsg> {
        let session = self.run.vote_session.as_ref()?;
        Some(ServerMsg::VoteOpen {
            level: session.level,
            options: session
                .options()
                .iter()
                .map(|o| protocol::VoteOption {
                    index: o.index,
                    id: o.id.clone(),
                    name: o.name.clone(),
                    kind: o.kind,
                })
                .collect(),
            countdown_s: u16::try_from(crate::run::VOTE_COUNTDOWN_TICKS / TICK_HZ).unwrap_or(20),
        })
    }

    /// The VoteState message for the open session, if any.
    fn vote_state_msg(&self) -> Option<ServerMsg> {
        let session = self.run.vote_session.as_ref()?;
        Some(ServerMsg::VoteState {
            counts: session.counts(),
            voted: session.voted_ids(),
            seconds_left: session.seconds_left(),
        })
    }

    /// Broadcast the current VoteState (counts, who has voted, seconds left).
    fn broadcast_vote_state(&mut self) {
        if let Some(msg) = self.vote_state_msg() {
            self.broadcast(&msg);
        }
    }

    /// Open and broadcast a fresh vote session for the level being exited (no-op when one is open).
    fn open_vote_session(&mut self) {
        let classes = self.lobby_class_ids();
        let host = self.joined_player_ids().into_iter().min();
        if !self.run.open_vote(&self.content, &classes, host) {
            return;
        }
        if let Some(session) = self.run.vote_session.as_mut() {
            session.last_sent_second = Some(session.seconds_left());
        }
        if let Some(msg) = self.vote_open_msg() {
            self.broadcast(&msg);
        }
        self.broadcast_vote_state();
        tracing::info!(level = self.run.level_num, "vote session opened");
    }

    /// Close the vote session and broadcast the outcome (winner, new permanent).
    fn finish_vote(&mut self) {
        let Some(outcome) = self.run.close_vote(&self.content) else {
            return;
        };
        self.broadcast(&ServerMsg::VoteResult {
            winner: outcome.winner,
            id: outcome.id.clone(),
            name: outcome.name.clone(),
            kind: outcome.kind,
            permanent: outcome.permanent,
        });
        tracing::info!(winner = %outcome.id, permanent = outcome.permanent, "vote resolved");
    }

    /// True when every joined player holds a ballot in the open session.
    fn all_joined_voted(&self) -> bool {
        let Some(session) = self.run.vote_session.as_ref() else {
            return false;
        };
        let players = self.joined_player_ids();
        !players.is_empty() && players.iter().all(|id| session.has_voted(*id))
    }

    /// Everyone back on the spawn platform of the current level (hub or generated), alive, then tell them.
    fn respawn_all_and_broadcast(&mut self) {
        let ids = self.joined_ids();
        for (slot, id) in ids.iter().enumerate() {
            let at = self.spawn_point(slot);
            if let Some(p) = self.conns.get_mut(id).and_then(|c| c.player.as_mut()) {
                p.state = player_spawn_state(at, p.class);
                p.inputs.clear();
                p.mode = PLAYER_ACTIVE;
                p.starved_ticks = 0;
            }
        }
        let level = ServerMsg::Level {
            data: self.run.level.clone(),
        };
        self.broadcast(&level);
        // Level start (generated levels only; the hub has no event): announce it.
        if self.run.level.event != EVENT_NONE
            && let Some(def) = self.content.event(self.run.level.event)
        {
            self.broadcast(&ServerMsg::Notice {
                kind: NOTICE_EVENT,
                ids: vec![def.id.clone()],
            });
        }
        // Escalation milestone banner (design §5, from level 4 on): only on actual level starts, not in the lobby.
        if self.run.phase == PHASE_PLAYING && crate::run::escalation_milestone_level(self.run.level_num) {
            self.broadcast(&ServerMsg::Notice {
                kind: NOTICE_EVENT,
                ids: vec![format!("milestone_{}", self.run.level_num)],
            });
        }
        let info = ServerMsg::RunState { info: self.run.info() };
        self.broadcast(&info);
        if let Some(draft) = self.draft_msg() {
            self.broadcast(&draft);
        }
        if let Some(shop) = self.shop_msg() {
            self.broadcast(&shop);
        }
    }

    fn broadcast_run_state(&mut self) {
        let info = ServerMsg::RunState { info: self.run.info() };
        self.broadcast(&info);
    }

    /// Resolve the lobby draft once every joined player has a currently-valid vote.
    /// (After a shop reroll, stored enemy-only votes are invalid until the player re-votes.)
    fn maybe_resolve_draft(&mut self) {
        if self.run.phase != PHASE_ENEMY_DRAFT || self.run.offer.is_none() {
            return;
        }
        let ids = self.joined_ids();
        let all_valid = !ids.is_empty()
            && ids
                .iter()
                .all(|id| self.votes.get(id).is_some_and(|v| self.run.validate_picks(v).is_ok()));
        if !all_valid {
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
        // Players keep their last class; they can change it in the next first lobby.
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
                // Joining mid-level: you spectate until the next lobby; in the lobby you play.
                let mode = if self.run.phase == PHASE_ENEMY_DRAFT {
                    PLAYER_ACTIVE
                } else {
                    PLAYER_WAITING
                };
                let class = CLASS_STRIKER;
                let state = player_spawn_state(spawn, class);
                if let Some(conn) = self.conns.get_mut(&id) {
                    conn.player = Some(Player {
                        id: player_id,
                        name,
                        state,
                        inputs: VecDeque::new(),
                        last_recv_seq: None,
                        last_applied_seq: 0,
                        tokens: 0,
                        starved_ticks: 0,
                        mode,
                        cause: DEATH_VOID,
                        class,
                        overtuned_ticks: 0,
                        infected_ticks: 0,
                        pad_launched: false,
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
                if let Some(shop) = self.shop_msg() {
                    self.send(id, &shop);
                }
                // Rejoining during an open vote: re-offer the session so the new player can ballot.
                if let Some(open) = self.vote_open_msg() {
                    self.send(id, &open);
                }
                if let Some(state) = self.vote_state_msg() {
                    self.send(id, &state);
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
            ClientMsg::Buy { item } => {
                match self.run.buy_shop_item(&self.content, &item) {
                    Ok(purchase) => {
                        // A reroll replaces the reward cards: enemy cards stay, reward votes are cleared,
                        // everyone gets the fresh Draft again.
                        if purchase == ShopPurchase::Reroll {
                            let cards = self.run.offer.as_ref().map(|o| o.cards.clone()).unwrap_or_default();
                            for vote in self.votes.values_mut() {
                                vote.retain(|&p| cards.get(p as usize).is_some_and(|c| c.kind == CARD_ENEMY));
                            }
                            if let Some(draft) = self.draft_msg() {
                                self.broadcast(&draft);
                            }
                        }
                        self.broadcast(&ServerMsg::Notice {
                            kind: NOTICE_BOUGHT,
                            ids: vec![item],
                        });
                        self.broadcast_run_state();
                        // Re-send the shop so everyone sees the new stock and currency.
                        if let Some(shop) = self.shop_msg() {
                            self.broadcast(&shop);
                        }
                    }
                    Err(why) => {
                        let (code, message) = match why {
                            ShopError::NotInLobby => (409, "the shop is not open"),
                            ShopError::UnknownItem => (404, "unknown shop item"),
                            ShopError::SoldOut => (409, "sold out"),
                            ShopError::NotEnoughCurrency => (402, "not enough currency"),
                        };
                        self.send(
                            id,
                            &ServerMsg::Error {
                                code,
                                message: message.to_owned(),
                            },
                        );
                    }
                }
            }
            ClientMsg::ChooseClass { class } => {
                // Only in the lobby, and only for the first lobby of a run (level 1).
                if self.run.phase != PHASE_ENEMY_DRAFT || self.run.level_num != 1 {
                    return self.send(
                        id,
                        &ServerMsg::Error {
                            code: 409,
                            message: "class can only be changed in the first lobby".to_owned(),
                        },
                    );
                }
                if class > CLASS_GLIDER {
                    return self.send(
                        id,
                        &ServerMsg::Error {
                            code: 400,
                            message: "unknown class".to_owned(),
                        },
                    );
                }
                if let Some(p) = self.conns.get_mut(&id).and_then(|c| c.player.as_mut()) {
                    p.class = class;
                    p.state.class = class;
                }
            }
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
            ClientMsg::Vote { option } => {
                let player_id = match conn.player.as_ref() {
                    Some(p) => p.id,
                    None => return self.reject(id, 400, "hello required first"),
                };
                let Some(session) = self.run.vote_session.as_mut() else {
                    return self.send(
                        id,
                        &ServerMsg::Error {
                            code: 409,
                            message: "no vote is open".to_owned(),
                        },
                    );
                };
                match session.cast(player_id, option) {
                    Ok(()) => {
                        self.broadcast_vote_state();
                        // The session closes early once every joined player holds a ballot.
                        if self.all_joined_voted() {
                            self.finish_vote();
                        }
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
        let tuning = self.run.tuning();
        // Altar uses are discovered while stepping input frames; notices go out after the movement loop.
        let mut altar_notices: Vec<(String, String)> = Vec::new();

        // 1. Movement (lobby and playing).
        for p in self.conns.values_mut().filter_map(|c| c.player.as_mut()) {
            p.pad_launched = false;
            p.tokens = (p.tokens + STEPS_PER_TICK).min(MAX_STEP_TOKENS);
            if !p.active() {
                // Spectators' inputs are acknowledged and dropped (their camera is client-side).
                if let Some(last) = p.inputs.back() {
                    p.last_applied_seq = last.seq;
                }
                p.inputs.clear();
                continue;
            }
            let mut stepped = 0;
            while p.tokens > 0 {
                let Some(f) = p.inputs.pop_front() else { break };
                // Altars: rising-edge BTN_INTERACT from the input frames, ACTIVE players, playing levels only.
                if self.run.phase == PHASE_PLAYING
                    && f.buttons & BTN_INTERACT != 0
                    && p.state.prev_buttons & BTN_INTERACT == 0
                    && let Some(pair) = self.run.try_use_altar(&self.content, p.state.pos)
                {
                    altar_notices.push(pair);
                }
                controller::step_with(&mut p.state, &f, &self.run.world, &tuning);
                // A pad launch is the only way to leave the ground faster than a jump
                // (PAD_LAUNCH_VEL 16 vs JUMP_VELOCITY 12); the controller reports no flag.
                if p.state.vel.y >= sim::tuning::PAD_LAUNCH_VEL - 0.5 {
                    p.pad_launched = true;
                }
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
        for (upgrade, curse) in altar_notices {
            self.broadcast(&ServerMsg::Notice {
                kind: NOTICE_ALTAR,
                ids: vec![upgrade, curse],
            });
            self.broadcast_run_state();
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
                    p.state = player_spawn_state(at, p.class);
                    p.inputs.clear();
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
                pos: p.state.pos,
                vel: p.state.vel,
                mode: p.mode,
                class: p.state.class,
            })
            .collect();
        let enemies = self.run.enemy_views();
        let projectiles = self.run.hazards.projectiles();
        let tiles = self.run.world_tiles();
        let altars_used = self.run.altars_used.clone();
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
                projectiles: projectiles.clone(),
                tiles: tiles.clone(),
                altars_used: altars_used.clone(),
                // M3 (T-056): two-phase levels.
                level_phase: self.run.level_phase,
                golden_taken: self.run.golden_taken.clone(),
                collapse_ms: self.run.collapse_ms(),
            };
            self.send(id, &msg);
        }
    }

    fn tick_playing(&mut self) {
        // Phase timers advance with the tick (PENDING -> COLLAPSING after 3.0 s; batch warnings and drops).
        self.run.tick_collapse();

        // The vote session countdown advances with the playing phase; it closes on zero, on
        // everyone having voted (at cast time), or when the level ends (below).
        let vote_tick = match self.run.vote_session.as_mut() {
            Some(session) => {
                let closed = session.tick();
                let secs = session.seconds_left();
                let changed = session.last_sent_second != Some(secs);
                session.last_sent_second = Some(secs);
                Some((closed, changed))
            }
            None => None,
        };
        match vote_tick {
            Some((true, _)) => self.finish_vote(),
            Some((false, true)) => self.broadcast_vote_state(),
            _ => {}
        }

        // Gifts (shared by the team). In phase 2 these are the golden gifts at the collected positions.
        let currency_before = self.run.currency;
        let mut pickups: Vec<Vec3> = Vec::new();
        let phase_before = self.run.level_phase;
        for conn in self.conns.values() {
            if let Some(p) = conn.player.as_ref().filter(|p| p.active()) {
                pickups.extend(self.run.collect_gifts(&p.state.pos));
            }
        }
        let mut info_changed = self.run.currency != currency_before;
        if phase_before != self.run.level_phase && self.run.level_phase == protocol::LEVEL_COLLAPSE_PENDING {
            self.broadcast(&ServerMsg::Notice {
                kind: NOTICE_EVENT,
                ids: vec!["collapse".to_string()],
            });
            // T-078: the vote session opens with the collapse/exit window.
            self.open_vote_session();
        }

        // Hazards: the senses of the ACTIVE players. `warded` = Bulwark Aegis Bloom is up (knockback-immune).
        // `speed_mul` = the phase-2 pressure multiplier (1.1 in LEVEL_COLLAPSING, design §1.4).
        let speed_mul = if self.run.level_phase == protocol::LEVEL_COLLAPSING {
            1.10
        } else {
            1.0
        };
        let senses: Vec<sim::hazards::PlayerSense> = self
            .conns
            .values()
            .filter_map(|c| c.player.as_ref())
            .filter(|p| p.active())
            .map(|p| sim::hazards::PlayerSense {
                id: p.id,
                feet: p.state.pos,
                vel: p.state.vel,
                warded: p.state.ward_frames > 0,
                class: p.class,
                jump_launched: p.pad_launched,
                used_ability: p.state.ability_active_frames > 0,
                used_support_ability: p.class == protocol::CLASS_BULWARK && p.state.ability_active_frames > 0,
                gliding: p.class == CLASS_GLIDER && p.state.ability_active_frames > 0 && !p.state.grounded,
                overtuned_ticks: p.overtuned_ticks,
                infected_ticks: p.infected_ticks,
            })
            .collect();
        // Registry brains read the phase and the run's permanents.
        let coil_permanent = self.run.all_permanents().iter().any(|id| id == "coil");
        self.run
            .hazards
            .set_run_context(self.run.level_phase == protocol::LEVEL_COLLAPSING, coil_permanent);
        let effects = self.run.hazards.step(
            &mut self.run.enemies,
            &self.run.world,
            &self.run.level,
            &senses,
            &pickups,
            speed_mul,
        );
        let mut shoves: Vec<(u32, Vec3, u8)> = Vec::new();
        let mut kills: BTreeMap<u32, u8> = BTreeMap::new();
        let mut overtuned: Vec<u32> = Vec::new();
        let mut infected: Vec<u32> = Vec::new();
        for effect in effects {
            match effect {
                sim::hazards::HazardEffect::Kill { player, cause } => {
                    kills.entry(player).or_insert(cause);
                }
                sim::hazards::HazardEffect::Shove {
                    player,
                    vel,
                    stagger_ticks,
                } => shoves.push((player, vel, stagger_ticks)),
                sim::hazards::HazardEffect::Overtuned { player } => overtuned.push(player),
                sim::hazards::HazardEffect::Infect { player } => infected.push(player),
            }
        }
        // Debuffs the registry brains applied this tick. OVERTUNED is what makes Peal's
        // ring lethal; the infection slows the victim and pauses Rewind's and Kite's timers.
        for p in self.conns.values_mut().filter_map(|c| c.player.as_mut()) {
            if overtuned.contains(&p.id) {
                p.overtuned_ticks = sim::rules::enemies::OVERTUNED_TICKS as u16;
            }
            if infected.contains(&p.id) {
                p.infected_ticks = sim::rules::enemies::blight::BLIGHT_INFECTION_TICKS as u16;
            }
            p.overtuned_ticks = p.overtuned_ticks.saturating_sub(1);
            p.infected_ticks = p.infected_ticks.saturating_sub(1);
        }

        // Bulwark Aegis Bloom: on the pulse frame the sim knocks enemies back; other ACTIVE, unwarded
        // players within 3.0 u (chest to chest) are shoved directly away at 7 u/s with 48 frames of stagger.
        let pulses: Vec<(u32, Vec3)> = self
            .conns
            .values()
            .filter_map(|c| c.player.as_ref())
            .filter(|p| p.active() && controller::bulwark_pulse_frame(&p.state))
            .map(|p| (p.id, Vec3::new(p.state.pos.x, p.state.pos.y + 0.9, p.state.pos.z)))
            .collect();
        for (caster_id, centre) in pulses {
            self.run
                .hazards
                .pulse(&mut self.run.enemies, &centre, BULWARK_PULSE_RADIUS);
            let others: Vec<PulsePlayer> = self
                .conns
                .values()
                .filter_map(|c| c.player.as_ref())
                .filter(|p| p.active())
                .map(|p| PulsePlayer {
                    id: p.id,
                    feet: p.state.pos,
                    warded: p.state.ward_frames > 0,
                })
                .collect();
            for (player_id, vel, stagger_frames) in pulse_stagger_targets(caster_id, &centre, &others) {
                if let Some(p) = self
                    .conns
                    .values_mut()
                    .filter_map(|c| c.player.as_mut())
                    .find(|p| p.id == player_id)
                {
                    p.state.vel = vel;
                    p.state.stagger_frames = stagger_frames;
                }
            }
        }

        // Shove effects (e.g. the Rift Warden): velocity is replaced; stagger upgraded to 60 Hz frames.
        for (player_id, vel, stagger_ticks) in shoves {
            if let Some(p) = self
                .conns
                .values_mut()
                .filter_map(|c| c.player.as_mut())
                .find(|p| p.id == player_id && p.active())
            {
                p.state.vel = vel;
                p.state.stagger_frames = stagger_ticks.saturating_mul(3);
            }
        }

        // World tiles: hazards (Crumble/Fretted) merged with the collapse (collapse wins on the same id).
        let tiles = self.run.world_tiles();
        self.run.world.set_tiles(&tiles);

        // Deaths: a spare life (shared by the team) respawns you at the level start; otherwise you spectate.
        // `kills` come from the hazards (enemy contact, projectiles); the void check stays the cheap early-out.
        let ids = self.joined_ids();
        for (slot, id) in ids.iter().enumerate() {
            let at = self.spawn_point(slot);
            let Some(p) = self
                .conns
                .get_mut(id)
                .and_then(|c| c.player.as_mut())
                .filter(|p| p.active())
            else {
                continue;
            };
            let cause = if p.state.pos.y < self.run.world.kill_y {
                Some(DEATH_VOID)
            } else {
                kills.get(&p.id).copied()
            };
            let Some(cause) = cause else {
                continue;
            };
            if self.run.lives > 0 {
                self.run.lives -= 1;
                p.state = player_spawn_state(at, p.class);
                p.inputs.clear();
                tracing::info!(conn = id, cause, lives = self.run.lives, "life lost");
            } else {
                p.mode = PLAYER_DEAD;
                p.cause = cause;
                p.state.vel = Vec3::ZERO;
                tracing::info!(conn = id, cause, level = self.run.level_num, "player out; spectating");
            }
            info_changed = true;
        }

        // Beacon: each active player who walks into the open beacon finishes the level and spectates.
        for p in self
            .conns
            .values_mut()
            .filter_map(|c| c.player.as_mut())
            .filter(|p| p.active())
        {
            if self.run.beacon_reached(&p.state.pos) {
                p.mode = PLAYER_FINISHED;
                p.state.vel = Vec3::ZERO;
                info_changed = true;
            }
        }

        // The level ends when nobody is still playing it.
        let players: Vec<&Player> = self.conns.values().filter_map(|c| c.player.as_ref()).collect();
        if !players.is_empty() && !players.iter().any(|p| p.active()) {
            if players.iter().any(|p| p.mode == PLAYER_FINISHED) {
                // Any open vote closes with the level: the winner still applies to the next level.
                self.finish_vote();
                self.run.finish_level(&self.content);
                tracing::info!(
                    next_level = self.run.level_num,
                    currency = self.run.currency,
                    "level cleared"
                );
                self.votes.clear();
                self.respawn_all_and_broadcast();
            } else {
                // Team wipe: the run is over; an open vote is discarded with it.
                self.run.vote_session = None;
                self.end_run();
            }
            return;
        }
        if info_changed {
            self.broadcast_run_state();
        }
    }

    /// Everyone is out: the run is over. Each player gets a summary with their own cause of death
    /// (DEATH_VOID, DEATH_ENEMY or DEATH_PROJECTILE).
    fn end_run(&mut self) {
        self.run.end();
        tracing::info!(level = self.run.level_num, "run over");
        let summaries: Vec<(ConnId, u8)> = self
            .conns
            .iter()
            .filter_map(|(id, c)| c.player.as_ref().map(|p| (*id, p.cause)))
            .collect();
        for (id, cause) in summaries {
            let summary = RunSummary {
                level_reached: self.run.level_num,
                gifts_collected: self.run.gifts_collected,
                cause,
            };
            self.send(id, &ServerMsg::RunOver { summary });
        }
        self.broadcast_run_state();
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
pub async fn run(mut rx: mpsc::Receiver<GameEvent>, mut game: Game, game_state: Arc<GameStateHandle>) {
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
                game_state.publish(
                    game.player_count() as u32,
                    game.run.level_num,
                    game.run.phase,
                );
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
    use crate::run::{ShopOffer, valid_picks};
    use protocol::{Aabb, EVENT_ICE, GIFT_GOLDEN, LevelData, PLATFORM_CRUMBLE, Platform, TILE_WARN};
    use sim::rules::{BEHAVIOR_TURRET, EnemyParams, spawn_enemy_with};
    use sim::world::World;

    fn game() -> Game {
        Game::starting_at(2, 42).unwrap()
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

    fn buy(g: &mut Game, id: ConnId, item: &str) {
        g.handle(GameEvent::Message {
            id,
            msg: ClientMsg::Buy { item: item.to_owned() },
        });
    }

    fn choose_class(g: &mut Game, id: ConnId, class: u8) {
        g.handle(GameEvent::Message {
            id,
            msg: ClientMsg::ChooseClass { class },
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
        // T-082: the shop is the four static items plus contextual upgrades, perks, counters and
        // purges, so the offer is at least the static sample.
        assert!(matches!(&msgs[4], ServerMsg::Shop { items } if items.len() >= 4));
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
        let a = g.conns[&1].player.as_ref().unwrap().state.pos;
        let b = g.conns[&2].player.as_ref().unwrap().state.pos;
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
    fn a_rejected_holdout_does_not_block_the_draft() {
        let mut g = game();
        let _rx1 = connect(&mut g, 1);
        hello(&mut g, 1);
        let _rx2 = connect(&mut g, 2);
        hello(&mut g, 2);
        vote(&mut g, 1);
        assert_eq!(g.run.phase, PHASE_ENEMY_DRAFT);
        g.handle(GameEvent::Message {
            id: 2,
            msg: inputs(0..MAX_INPUTS_PER_MSG + 1, 0),
        });
        assert_eq!(g.conns.len(), 1);
        assert_eq!(g.run.phase, PHASE_PLAYING);
    }

    #[test]
    fn full_loop_gifts_golden_beacon_back_to_lobby() {
        let mut g = game();
        let mut rx = connect(&mut g, 1);
        hello(&mut g, 1);
        vote(&mut g, 1);
        drain(&mut rx);
        // Level 2: all gifts are normal; the last one starts the collapse phase.
        let gifts = g.run.level.gifts.clone();
        for gift in gifts.iter().filter(|g| g.kind != GIFT_GOLDEN) {
            {
                let p = player(&mut g, 1);
                p.state.pos = Vec3::new(gift.pos.x, gift.pos.y - 0.9, gift.pos.z);
                p.state.vel = Vec3::ZERO;
                p.starved_ticks = 0;
            }
            for e in &mut g.run.enemies {
                e.active_in = 1_000; // keep enemies dormant for this test
            }
            g.tick();
            let _ = drain(&mut rx); // keep the outbox under capacity
        }
        assert_eq!(g.run.level_phase, protocol::LEVEL_COLLAPSE_PENDING);
        // Phase 2: a golden gift now sits at every collected position (same ids). Sweep the same spots again.
        for gift in &gifts {
            {
                let p = player(&mut g, 1);
                p.state.pos = Vec3::new(gift.pos.x, gift.pos.y - 0.9, gift.pos.z);
                p.state.vel = Vec3::ZERO;
                p.starved_ticks = 0;
            }
            for e in &mut g.run.enemies {
                e.active_in = 1_000;
            }
            g.tick();
            let _ = drain(&mut rx);
        }
        assert!(g.run.currency > 0);
        assert_eq!(g.run.golden_taken.len(), gifts.len());
        let beacon = g.run.level.beacon;
        player(&mut g, 1).state.pos = beacon;
        g.tick();
        assert_eq!(g.run.phase, PHASE_ENEMY_DRAFT);
        assert_eq!(g.run.level_num, 3);
        let msgs = drain(&mut rx);
        assert!(
            msgs.iter()
                .any(|m| matches!(m, ServerMsg::Draft { enemy_picks: 0, .. }))
        );
        // After finish_level the team is back in the lobby for level 3; RunState reports
        // the upcoming level and the persistent team currency.
        assert!(
            msgs.iter()
                .any(|m| matches!(m, ServerMsg::RunState { info } if info.currency > 0 && info.level == 3))
        );
        assert!(
            msgs.iter().any(|m| matches!(m, ServerMsg::Shop { .. })),
            "the next lobby offers the shop"
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
        assert!(player(&mut g, 1).active());
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
        assert!(player(&mut g, 1).active());
        assert!(player(&mut g, 1).state.pos.y > kill_y);
        assert!(!drain(&mut rx).iter().any(|m| matches!(m, ServerMsg::RunOver { .. })));
    }

    fn dormant_enemies(g: &mut Game) {
        for e in &mut g.run.enemies {
            e.active_in = 1_000;
        }
    }

    fn collect_normal_gifts(g: &mut Game, id: ConnId, rxs: &mut [&mut mpsc::Receiver<Vec<u8>>]) {
        let gifts: Vec<_> = g
            .run
            .level
            .gifts
            .iter()
            .filter(|g| g.kind != GIFT_GOLDEN)
            .cloned()
            .collect();
        for gift in gifts {
            {
                let p = player(g, id);
                p.state.pos = Vec3::new(gift.pos.x, gift.pos.y - 0.9, gift.pos.z);
                p.state.vel = Vec3::ZERO;
                p.starved_ticks = 0;
            }
            dormant_enemies(g);
            g.tick();
            // Keep every outbox under capacity: undrained receivers drop the connection mid-sweep.
            for rx in &mut *rxs {
                let _ = drain(rx);
            }
        }
    }

    #[test]
    fn multiplayer_beacon_makes_you_spectate_until_everyone_is_done() {
        let mut g = game();
        let mut rx1 = connect(&mut g, 1);
        hello(&mut g, 1);
        let mut rx2 = connect(&mut g, 2);
        hello(&mut g, 2);
        vote(&mut g, 1);
        vote(&mut g, 2);
        collect_normal_gifts(&mut g, 1, &mut [&mut rx1, &mut rx2]);
        let beacon = g.run.level.beacon;
        player(&mut g, 1).state.pos = beacon;
        dormant_enemies(&mut g);
        g.tick();
        assert_eq!(player(&mut g, 1).mode, PLAYER_FINISHED, "player 1 spectates");
        assert_eq!(g.run.phase, PHASE_PLAYING, "player 2 is still playing");
        let snaps = drain(&mut rx1);
        assert!(snaps.iter().any(|m| matches!(m, ServerMsg::Snapshot { players, .. }
            if players.iter().any(|p| p.id == 1 && p.mode == PLAYER_FINISHED))));
        // Player 2 falls with no lives: nobody is active, someone finished -> the team goes to the lobby.
        g.run.lives = 0;
        let kill_y = g.run.world.kill_y;
        player(&mut g, 2).state.pos.y = kill_y - 5.0;
        g.tick();
        assert_eq!(g.run.phase, PHASE_ENEMY_DRAFT);
        assert_eq!(g.run.level_num, 3);
        assert!(player(&mut g, 1).active() && player(&mut g, 2).active());
    }

    #[test]
    fn multiplayer_run_ends_only_when_everyone_is_out() {
        let mut g = game();
        let mut rx1 = connect(&mut g, 1);
        hello(&mut g, 1);
        let mut rx2 = connect(&mut g, 2);
        hello(&mut g, 2);
        vote(&mut g, 1);
        vote(&mut g, 2);
        g.run.lives = 0;
        let kill_y = g.run.world.kill_y;
        player(&mut g, 1).state.pos.y = kill_y - 5.0;
        g.tick();
        assert_eq!(player(&mut g, 1).mode, PLAYER_DEAD);
        assert_eq!(g.run.phase, PHASE_PLAYING);
        assert!(
            !drain(&mut rx1).iter().any(|m| matches!(m, ServerMsg::RunOver { .. })),
            "no summary yet"
        );
        player(&mut g, 2).state.pos.y = kill_y - 5.0;
        g.tick();
        assert_eq!(g.run.phase, PHASE_OVER);
        for rx in [&mut rx1, &mut rx2] {
            assert!(drain(rx).iter().any(|m| matches!(m, ServerMsg::RunOver { .. })));
        }
    }

    #[test]
    fn joining_mid_level_waits_as_a_spectator() {
        let mut g = game();
        let _rx1 = connect(&mut g, 1);
        hello(&mut g, 1);
        vote(&mut g, 1);
        let _rx2 = connect(&mut g, 2);
        hello(&mut g, 2);
        assert_eq!(player(&mut g, 2).mode, PLAYER_WAITING);
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

    #[test]
    fn choose_class_only_in_first_lobby_and_persists_across_respawn() {
        let mut g = Game::starting_at(1, 99).unwrap(); // level 1: the first lobby
        let mut rx = connect(&mut g, 1);
        hello(&mut g, 1);
        assert_eq!(player(&mut g, 1).class, CLASS_STRIKER);
        // Choose the glider.
        choose_class(&mut g, 1, protocol::CLASS_GLIDER);
        assert_eq!(player(&mut g, 1).class, protocol::CLASS_GLIDER);
        assert_eq!(player(&mut g, 1).state.class, protocol::CLASS_GLIDER);
        // Out-of-range class: 400, no change.
        drain(&mut rx);
        choose_class(&mut g, 1, 4);
        assert!(matches!(drain(&mut rx)[..], [ServerMsg::Error { code: 400, .. }]));
        assert_eq!(player(&mut g, 1).class, protocol::CLASS_GLIDER);
        // Vote into the level: class is copied into the spawned state.
        vote(&mut g, 1);
        assert_eq!(g.run.phase, PHASE_PLAYING);
        assert_eq!(player(&mut g, 1).state.class, protocol::CLASS_GLIDER);
        // Now playing: choosing is rejected with 409.
        drain(&mut rx);
        choose_class(&mut g, 1, CLASS_STRIKER);
        assert!(matches!(drain(&mut rx)[..], [ServerMsg::Error { code: 409, .. }]));
        assert_eq!(player(&mut g, 1).class, protocol::CLASS_GLIDER);
        // Respawn with a spare life: the class survives.
        g.run.lives = 1;
        let kill_y = g.run.world.kill_y;
        player(&mut g, 1).state.pos.y = kill_y - 5.0;
        g.tick();
        assert_eq!(player(&mut g, 1).mode, PLAYER_ACTIVE);
        assert_eq!(player(&mut g, 1).class, protocol::CLASS_GLIDER);
        assert_eq!(player(&mut g, 1).state.class, protocol::CLASS_GLIDER);
    }

    #[test]
    fn choose_class_rejected_outside_the_first_lobby() {
        let mut g = game(); // starting at level 2: not the first lobby
        let mut rx = connect(&mut g, 1);
        hello(&mut g, 1);
        drain(&mut rx);
        choose_class(&mut g, 1, protocol::CLASS_BULWARK);
        assert!(matches!(drain(&mut rx)[..], [ServerMsg::Error { code: 409, .. }]));
        assert_eq!(player(&mut g, 1).class, CLASS_STRIKER);
    }

    #[test]
    fn new_run_keeps_last_class() {
        let mut g = Game::starting_at(1, 99).unwrap();
        let mut rx = connect(&mut g, 1);
        hello(&mut g, 1);
        choose_class(&mut g, 1, protocol::CLASS_GLIDER);
        vote(&mut g, 1);
        drain(&mut rx);
        g.run.lives = 0;
        let kill_y = g.run.world.kill_y;
        player(&mut g, 1).state.pos.y = kill_y - 5.0;
        g.tick();
        assert_eq!(g.run.phase, PHASE_OVER);
        g.handle(GameEvent::Message {
            id: 1,
            msg: ClientMsg::Restart,
        });
        assert_eq!(g.run.phase, PHASE_ENEMY_DRAFT);
        assert_eq!(g.run.level_num, 1);
        assert_eq!(
            player(&mut g, 1).class,
            protocol::CLASS_GLIDER,
            "class survives a new run"
        );
        // ...and can be changed again in the next first lobby.
        choose_class(&mut g, 1, CLASS_STRIKER);
        assert_eq!(player(&mut g, 1).class, CLASS_STRIKER);
    }

    #[test]
    fn event_notice_and_ice_friction_when_level_has_event() {
        let mut g = game();
        let mut rx = connect(&mut g, 1);
        hello(&mut g, 1);
        drain(&mut rx);
        // Simulate a freshly generated EVENT_ICE level (content carries friction_mul 0.25).
        g.run.level.event = EVENT_ICE;
        g.run.event_friction_mul = 0.25;
        g.respawn_all_and_broadcast();
        let msgs = drain(&mut rx);
        assert!(msgs.iter().any(|m| matches!(
            m,
            ServerMsg::Notice { kind: NOTICE_EVENT, ids } if ids == &vec!["slick_ice".to_string()]
        )));
        assert!(msgs.iter().any(|m| matches!(
            m,
            ServerMsg::RunState { info } if info.tuning.friction_mul == 0.25
        )));
        // With no event, no notice and normal friction.
        g.run.level.event = EVENT_NONE;
        g.run.event_friction_mul = 1.0;
        g.respawn_all_and_broadcast();
        let msgs = drain(&mut rx);
        assert!(
            !msgs
                .iter()
                .any(|m| matches!(m, ServerMsg::Notice { kind: NOTICE_EVENT, .. }))
        );
        assert!(msgs.iter().any(|m| matches!(
            m,
            ServerMsg::RunState { info } if info.tuning.friction_mul == 1.0
        )));
    }

    #[test]
    fn buy_errors_map_to_codes_and_never_kick() {
        let mut g = game(); // level 2 lobby: the shop is open
        let mut rx = connect(&mut g, 1);
        hello(&mut g, 1);
        drain(&mut rx);
        g.run.currency = 0;
        g.run.shop = Some(vec![ShopOffer {
            id: "kept_flame".to_string(),
            cost: 30,
            stock: 1,
        }]);
        // Unknown item: 404.
        buy(&mut g, 1, "nope");
        assert!(matches!(drain(&mut rx)[..], [ServerMsg::Error { code: 404, .. }]));
        // Not enough currency: 402.
        buy(&mut g, 1, "kept_flame");
        assert!(matches!(drain(&mut rx)[..], [ServerMsg::Error { code: 402, .. }]));
        // Bought; bought again: sold out, 409.
        g.run.currency = 100;
        buy(&mut g, 1, "kept_flame");
        drain(&mut rx);
        buy(&mut g, 1, "kept_flame");
        assert!(matches!(drain(&mut rx)[..], [ServerMsg::Error { code: 409, .. }]));
        // The shop is closed while playing: 409.
        vote(&mut g, 1);
        drain(&mut rx);
        g.run.currency = 100;
        buy(&mut g, 1, "kept_flame");
        assert!(matches!(drain(&mut rx)[..], [ServerMsg::Error { code: 409, .. }]));
        assert_eq!(g.player_count(), 1, "errors never kick the player");
    }

    #[test]
    fn buying_reroll_clears_reward_votes_and_resends_draft() {
        let mut g = game(); // level 2 lobby
        let mut rx1 = connect(&mut g, 1);
        hello(&mut g, 1);
        let mut rx2 = connect(&mut g, 2);
        hello(&mut g, 2);
        drain(&mut rx1);
        drain(&mut rx2);
        g.run.currency = 100;
        g.run.shop = Some(vec![ShopOffer {
            id: "new_hand".to_string(),
            cost: 15,
            stock: 1,
        }]);
        vote(&mut g, 1);
        assert_eq!(g.run.phase, PHASE_ENEMY_DRAFT);
        // Player 2 buys the reroll.
        buy(&mut g, 2, "new_hand");
        // Player 1's stored vote is stripped down to enemy picks only.
        let offer = g.run.offer.clone().unwrap();
        assert!(g.votes[&1].iter().all(|&p| offer.cards[p as usize].kind == CARD_ENEMY));
        // Both players got the fresh Draft, the Notice and the re-sent Shop (stock 0).
        for rx in [&mut rx1, &mut rx2] {
            let msgs = drain(rx);
            assert!(msgs.iter().any(|m| matches!(m, ServerMsg::Draft { .. })));
            assert!(msgs.iter().any(|m| matches!(
                m,
                ServerMsg::Notice { kind: NOTICE_BOUGHT, ids } if ids == &vec!["new_hand".to_string()]
            )));
            assert!(
                msgs.iter()
                    .any(|m| matches!(m, ServerMsg::Shop { items } if items.len() == 1 && items[0].stock == 0))
            );
            assert!(msgs.iter().any(|m| matches!(
                m,
                ServerMsg::RunState { info } if info.currency == 85
            )));
        }
        // Player 2 votes; the draft must NOT resolve yet (player 1's vote was cleared).
        vote(&mut g, 2);
        assert_eq!(g.run.phase, PHASE_ENEMY_DRAFT);
        // Player 1 re-votes; now the draft resolves.
        vote(&mut g, 1);
        assert_eq!(g.run.phase, PHASE_PLAYING);
    }

    #[test]
    fn altar_use_broadcasts_notice_and_happens_once() {
        let mut g = game(); // level 2 lobby
        let mut rx = connect(&mut g, 1);
        hello(&mut g, 1);
        vote(&mut g, 1);
        assert_eq!(g.run.phase, PHASE_PLAYING);
        drain(&mut rx);
        // Construct an altar next to the player.
        g.run.level.altars = vec![
            protocol::Altar {
                id: 9,
                pos: Vec3::new(0.0, 0.0, 0.0),
            },
            protocol::Altar {
                id: 10,
                pos: Vec3::new(0.0, 0.0, 0.0),
            },
        ];
        g.run.altars_used.clear();
        player(&mut g, 1).state.pos = Vec3::new(0.25, 0.0, 0.25);
        // Rising edge: press E (BTN_INTERACT).
        g.handle(GameEvent::Message {
            id: 1,
            msg: ClientMsg::Inputs {
                frames: vec![InputFrame {
                    seq: 1,
                    move_x: 0,
                    move_z: 0,
                    buttons: BTN_INTERACT,
                }],
            },
        });
        g.tick();
        assert_eq!(g.run.altars_used, vec![9], "first unused altar in range is used");
        // Altar 9 is the gift-sight design (9 % 4 == 1): it lights every gift this level and pushes
        // the next draft's reward cards into the hardest pool.
        assert!(g.run.altar_ledger.gift_sight, "the benefit is recorded");
        assert!(g.run.altar_ledger.hardest_pool_next, "the cost is recorded");
        let msgs = drain(&mut rx);
        assert!(msgs.iter().any(|m| matches!(
            m,
            ServerMsg::Notice { kind: NOTICE_ALTAR, ids }
                if ids.len() == 2 && ids[0] == "gift_sight" && ids[1] == "hardest_pool"
        )));
        assert!(msgs.iter().any(|m| matches!(
            m,
            ServerMsg::Snapshot { altars_used, .. } if altars_used == &vec![9]
        )));
        // Holding the key (no rising edge) does not touch the second altar.
        g.handle(GameEvent::Message {
            id: 1,
            msg: ClientMsg::Inputs {
                frames: vec![InputFrame {
                    seq: 2,
                    move_x: 0,
                    move_z: 0,
                    buttons: BTN_INTERACT,
                }],
            },
        });
        g.tick();
        assert_eq!(g.run.altars_used, vec![9], "holding BTN_INTERACT is not a rising edge");
        // Release, then press again: the second altar is used.
        g.handle(GameEvent::Message {
            id: 1,
            msg: ClientMsg::Inputs {
                frames: vec![InputFrame {
                    seq: 3,
                    move_x: 0,
                    move_z: 0,
                    buttons: 0,
                }],
            },
        });
        g.tick();
        g.handle(GameEvent::Message {
            id: 1,
            msg: ClientMsg::Inputs {
                frames: vec![InputFrame {
                    seq: 4,
                    move_x: 0,
                    move_z: 0,
                    buttons: BTN_INTERACT,
                }],
            },
        });
        g.tick();
        assert_eq!(g.run.altars_used, vec![9, 10]);
        // And again: both altars are spent for this level.
        g.handle(GameEvent::Message {
            id: 1,
            msg: ClientMsg::Inputs {
                frames: vec![
                    InputFrame {
                        seq: 5,
                        move_x: 0,
                        move_z: 0,
                        buttons: 0,
                    },
                    InputFrame {
                        seq: 6,
                        move_x: 0,
                        move_z: 0,
                        buttons: BTN_INTERACT,
                    },
                ],
            },
        });
        g.tick();
        assert_eq!(g.run.altars_used, vec![9, 10], "one use per altar per level");
    }

    #[test]
    fn projectile_kills_and_run_over_cause_is_death_projectile() {
        let mut g = game(); // level 2 lobby
        let mut rx = connect(&mut g, 1);
        hello(&mut g, 1);
        vote(&mut g, 1);
        drain(&mut rx);
        // Minimal firing range: a single platform far below the line of fire and nothing in it,
        // so the Bolt Sentry's line-of-sight check is unambiguously clear and its flat bolt can
        // only be stopped by the player. (On a generated hub the LOS/bolt lane grazes the deck's
        // top face, which counts as an intersection, so the turret never got to charge or fire.)
        g.run.level = LevelData {
            level: 2,
            seed: 1,
            spawn: Vec3::new(0.0, 2.0, 0.0),
            beacon: Vec3::new(50.0, 2.0, 0.0), // never reached; the level only ends by deaths
            kill_y: -50.0,
            platforms: vec![Platform {
                id: 1,
                kind: protocol::PLATFORM_NORMAL,
                bounds: Aabb {
                    min: Vec3::new(-5.0, -1.0, -5.0),
                    max: Vec3::new(5.0, 0.0, 5.0),
                },
            }],
            gifts: Vec::new(),
            event: EVENT_NONE,
            altars: Vec::new(),
        };
        g.run.world = World::from_level(&g.run.level);
        g.run.hazards.reset();
        g.run.altars_used.clear();
        g.run.gifts_taken.clear();
        g.run.enemies.clear();
        g.run.lives = 0;
        let pos = Vec3::new(0.0, 2.0, 0.0); // feet; the player floats here (pinned, no gravity)
        // Replace the drafted enemies with a point-blank turret (Bolt Sentry charges flat bolts).
        g.run.enemies.push(spawn_enemy_with(
            1,
            0,
            Vec3::new(pos.x + 3.0, pos.y + 0.9, pos.z),
            EnemyParams {
                behavior: BEHAVIOR_TURRET,
                speed_mul: 1.0,
                grace_ticks: 0,
                size: 0.8,
            },
        ));
        // 12 simulated seconds, draining periodically so the client buffer never fills.
        // The player is pinned in place so the turret's flat bolts are the only possible kill.
        let mut summary = None;
        'outer: for i in 0..240 {
            {
                let p = player(&mut g, 1);
                p.state.pos = pos;
                p.state.vel = Vec3::ZERO;
                p.starved_ticks = 0;
            }
            g.tick();
            if i % 16 == 15 {
                summary = drain(&mut rx).into_iter().find_map(|m| match m {
                    ServerMsg::RunOver { summary } => Some(summary),
                    _ => None,
                });
                if summary.is_some() {
                    break 'outer;
                }
            }
        }
        if summary.is_none() {
            summary = drain(&mut rx).into_iter().find_map(|m| match m {
                ServerMsg::RunOver { summary } => Some(summary),
                _ => None,
            });
        }
        assert_eq!(
            summary.map(|s| s.cause),
            Some(protocol::DEATH_PROJECTILE),
            "the run summary carries DEATH_PROJECTILE"
        );
    }

    #[test]
    fn crumbling_tile_shows_up_in_the_snapshot() {
        let mut g = game(); // level 2 lobby
        let mut rx = connect(&mut g, 1);
        hello(&mut g, 1);
        vote(&mut g, 1);
        drain(&mut rx);
        // Construct a level whose main platform is crumbling.
        g.run.level = LevelData {
            level: 2,
            seed: 1,
            spawn: Vec3::new(0.0, 0.0, 0.0),
            beacon: Vec3::new(0.0, 0.0, 50.0),
            kill_y: -100.0,
            platforms: vec![Platform {
                id: 7,
                kind: PLATFORM_CRUMBLE,
                bounds: Aabb {
                    min: Vec3::new(-5.0, -1.0, -5.0),
                    max: Vec3::new(5.0, 10.0, 5.0),
                },
            }],
            gifts: Vec::new(),
            event: EVENT_NONE,
            altars: Vec::new(),
        };
        g.run.world = World::from_level(&g.run.level);
        g.run.hazards.reset();
        g.run.altars_used.clear();
        g.run.gifts_taken.clear();
        g.run.enemies.clear();
        player(&mut g, 1).state.pos = Vec3::new(0.0, 10.0, 0.0); // feet on the platform top
        g.tick();
        let msgs = drain(&mut rx);
        let tiles = msgs.iter().find_map(|m| match m {
            ServerMsg::Snapshot { tiles, .. } => Some(tiles.clone()),
            _ => None,
        });
        let tiles = tiles.expect("snapshot carries tiles");
        assert!(
            tiles.iter().any(|t| t.id == 7 && t.state == TILE_WARN),
            "standing on a crumbling platform warns it; snapshot reports it"
        );
    }
}

/// Radius of a Bulwark Aegis Bloom pulse (design §3: 3.0 u).
const BULWARK_PULSE_RADIUS: f32 = 3.0;
/// Speed of the radial knockback applied by the pulse (7 u/s, away from the caster).
const BULWARK_PULSE_SPEED: f32 = 7.0;
/// Stagger applied by the pulse to other players, in 60 Hz frames (0.8 s).
const BULWARK_PULSE_STAGGER_FRAMES: u8 = 48;

/// One ACTIVE player as seen by a Bulwark pulse.
struct PulsePlayer {
    /// Player id (the caster is excluded by id).
    id: u32,
    /// Feet position (the chest is derived as `feet + 0.9`).
    feet: Vec3,
    /// True while the player is rooted in their own ward (knockback-immune).
    warded: bool,
}

/// Compute the player shoves for one Bulwark pulse: every ACTIVE, unwarded player other than the caster
/// within [`BULWARK_PULSE_RADIUS`] (measured chest to chest). Each is pushed horizontally directly away at
/// [`BULWARK_PULSE_SPEED`] u/s with [`BULWARK_PULSE_STAGGER_FRAMES`] frames of stagger.
fn pulse_stagger_targets(caster_id: u32, centre: &Vec3, others: &[PulsePlayer]) -> Vec<(u32, Vec3, u8)> {
    let mut out = Vec::new();
    for player in others {
        if player.id == caster_id || player.warded {
            continue;
        }
        let dx = player.feet.x - centre.x;
        let dy = (player.feet.y + 0.9) - centre.y;
        let dz = player.feet.z - centre.z;
        if dx * dx + dy * dy + dz * dz > BULWARK_PULSE_RADIUS * BULWARK_PULSE_RADIUS {
            continue;
        }
        let horizontal = (dx * dx + dz * dz).sqrt();
        let (ux, uz) = if horizontal <= 1e-6 {
            (1.0, 0.0)
        } else {
            (dx / horizontal, dz / horizontal)
        };
        out.push((
            player.id,
            Vec3::new(ux * BULWARK_PULSE_SPEED, 0.0, uz * BULWARK_PULSE_SPEED),
            BULWARK_PULSE_STAGGER_FRAMES,
        ));
    }
    out
}

#[cfg(test)]
mod m3_tests {
    use super::*;

    fn game2() -> Game {
        Game::starting_at(2, 42).unwrap()
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
                name: "tester".into(),
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

    fn player(g: &mut Game, id: ConnId) -> &mut Player {
        g.conns.get_mut(&id).unwrap().player.as_mut().unwrap()
    }

    fn vote(g: &mut Game, id: ConnId) {
        let picks = crate::run::valid_picks(&g.run);
        g.handle(GameEvent::Message {
            id,
            msg: ClientMsg::DraftPick { picks },
        });
    }

    /// Collect every remaining normal gift as `id`; the last one starts the collapse window
    /// (and with it the vote session). The final tick's messages are left in the outboxes.
    fn enter_collapse_and_open_vote(g: &mut Game, id: ConnId, rxs: &mut [&mut mpsc::Receiver<Vec<u8>>]) {
        let gifts: Vec<_> = g
            .run
            .level
            .gifts
            .iter()
            .filter(|g| g.kind != protocol::GIFT_GOLDEN)
            .cloned()
            .collect();
        for (i, gift) in gifts.iter().enumerate() {
            {
                let p = player(g, id);
                p.state.pos = Vec3::new(gift.pos.x, gift.pos.y - 0.9, gift.pos.z);
                p.state.vel = Vec3::ZERO;
                p.starved_ticks = 0;
            }
            for e in &mut g.run.enemies {
                e.active_in = 1_000;
            }
            g.tick();
            // Keep the outboxes under capacity; leave the last tick's burst for the caller.
            if i + 1 < gifts.len() {
                for rx in &mut *rxs {
                    let _ = drain(rx);
                }
            }
        }
        assert_eq!(g.run.level_phase, protocol::LEVEL_COLLAPSE_PENDING);
        assert!(
            g.run.vote_session.is_some(),
            "the vote session opens with the collapse window"
        );
    }

    #[test]
    fn pulse_stagger_targets_are_radius_limited_and_never_the_caster_nor_warded_players() {
        let centre = Vec3::new(0.0, 0.9, 0.0);
        let others = vec![
            PulsePlayer {
                id: 7,
                feet: Vec3::new(0.0, 0.0, 0.0),
                warded: false,
            }, // the caster (chest at the centre)
            PulsePlayer {
                id: 8,
                feet: Vec3::new(2.9, 0.0, 0.0),
                warded: false,
            }, // inside the radius
            PulsePlayer {
                id: 9,
                feet: Vec3::new(3.5, 0.0, 0.0),
                warded: false,
            }, // outside
            PulsePlayer {
                id: 10,
                feet: Vec3::new(1.0, 0.0, 0.0),
                warded: true,
            }, // warded: immune
        ];
        let out = pulse_stagger_targets(7, &centre, &others);
        assert_eq!(out.len(), 1);
        let (id, vel, stagger) = &out[0];
        assert_eq!(*id, 8);
        assert_eq!(*stagger, BULWARK_PULSE_STAGGER_FRAMES);
        assert!((((vel.x * vel.x) + (vel.z * vel.z)).sqrt() - BULWARK_PULSE_SPEED).abs() < 1e-4);
        assert!(vel.x > 0.0, "shoved directly away from the caster");
        assert_eq!(vel.y, 0.0);
    }

    #[test]
    fn bulwark_cast_roots_and_raises_the_ward() {
        let mut g = Game::starting_at(1, 99).unwrap(); // level 1: the first lobby, class choice is open
        let mut rx = connect(&mut g, 1);
        hello(&mut g, 1);
        g.handle(GameEvent::Message {
            id: 1,
            msg: ClientMsg::ChooseClass {
                class: protocol::CLASS_BULWARK,
            },
        });
        vote(&mut g, 1);
        // Let the level start and the player land on the deck before any ability is held:
        // a BTN_ABILITY rising edge consumed while airborne never opens the ward, and this
        // sends frames at the real-time rate (STEPS_PER_TICK per tick) so physics ticks normally.
        // Drain every tick: an undrained outbox fills (OUTBOX_CAPACITY) and the server drops us.
        let mut seq = 0u32;
        for _ in 0..60 {
            let frames: Vec<InputFrame> = (0..STEPS_PER_TICK)
                .map(|offset| InputFrame {
                    seq: seq + offset,
                    move_x: 0,
                    move_z: 0,
                    buttons: 0,
                })
                .collect();
            seq += STEPS_PER_TICK;
            g.handle(GameEvent::Message {
                id: 1,
                msg: ClientMsg::Inputs { frames },
            });
            g.tick();
            let _ = drain(&mut rx);
        }
        // A fresh rising edge of BTN_ABILITY on a grounded, still player casts Aegis Bloom.
        let mut warded = false;
        for _ in 0..60 {
            let frames: Vec<InputFrame> = (0..STEPS_PER_TICK)
                .map(|offset| InputFrame {
                    seq: seq + offset,
                    move_x: 0,
                    move_z: 0,
                    buttons: BTN_ABILITY,
                })
                .collect();
            seq += STEPS_PER_TICK;
            g.handle(GameEvent::Message {
                id: 1,
                msg: ClientMsg::Inputs { frames },
            });
            g.tick();
            let _ = drain(&mut rx);
            if g.conns[&1].player.as_ref().unwrap().state.ward_frames > 0 {
                warded = true;
                break;
            }
        }
        assert!(warded, "Aegis Bloom raised the ward (rooted, knockback-immune)");
    }

    #[test]
    fn rift_warden_knockback_is_applied_with_stagger() {
        let mut g = game2();
        let mut rx = connect(&mut g, 1);
        hello(&mut g, 1);
        vote(&mut g, 1);
        let _ = drain(&mut rx); // flush the level-start burst so the outbox stays slack
        for e in &mut g.run.enemies {
            e.active_in = 1_000; // keep the drafted enemies dormant
        }
        let pos = player(&mut g, 1).state.pos;
        g.run.enemies.push(sim::rules::spawn_enemy_with(
            77,
            0,
            Vec3::new(pos.x + 2.0, pos.y, pos.z),
            sim::rules::EnemyParams {
                behavior: sim::rules::BEHAVIOR_RIFT_WARDEN,
                speed_mul: 1.0,
                size: 1.5,
                grace_ticks: 0,
            },
        ));
        let mut saw_stagger = false;
        for _ in 0..120 {
            // Pin the player next to the warden: a no-input player freefalls out of knockback range.
            {
                let p = player(&mut g, 1);
                p.state.pos = pos;
                p.state.vel = Vec3::ZERO;
                p.starved_ticks = 0;
            }
            g.tick();
            let _ = drain(&mut rx); // drain every tick so the outbox never fills
            let p = g.conns[&1].player.as_ref().unwrap();
            if p.state.stagger_frames > 0 {
                assert_eq!(p.state.stagger_frames % 3, 0, "stagger is stored in 60 Hz frames");
                let knockback_sq =
                    p.state.vel.x * p.state.vel.x + p.state.vel.y * p.state.vel.y + p.state.vel.z * p.state.vel.z;
                assert!(knockback_sq > 0.0, "knockback velocity was applied");
                saw_stagger = true;
                break;
            }
        }
        assert!(saw_stagger, "the rift warden shoved the player (stagger applied)");
    }

    #[test]
    fn last_gift_starts_phase_two_with_the_collapse_banner() {
        let mut g = game2();
        let mut rx = connect(&mut g, 1);
        hello(&mut g, 1);
        vote(&mut g, 1);
        drain(&mut rx);
        // Dormantise everything that could kill us and teleport to every gift, one per tick.
        let mut collected_msgs: Vec<ServerMsg> = Vec::new();
        let gifts: Vec<_> = g.run.level.gifts.clone();
        for gift in gifts {
            {
                let p = player(&mut g, 1);
                p.state.pos = Vec3::new(gift.pos.x, gift.pos.y - 0.9, gift.pos.z);
                p.state.vel = Vec3::ZERO;
                p.starved_ticks = 0;
            }
            for e in &mut g.run.enemies {
                e.active_in = 1_000;
            }
            g.tick();
            collected_msgs.extend(drain(&mut rx));
        }
        assert_eq!(g.run.level_phase, protocol::LEVEL_COLLAPSE_PENDING);
        assert!(collected_msgs.iter().chain(drain(&mut rx).iter()).any(|m| matches!(
            m,
            ServerMsg::Notice { kind: NOTICE_EVENT, ids } if ids == &vec!["collapse".to_string()]
        )));
        // Idle (pinned) so starvation gravity cannot drag the player into the void during the siren.
        let safe = player(&mut g, 1).state.pos;
        for i in 0..60 {
            {
                let p = player(&mut g, 1);
                p.state.pos = safe;
                p.state.vel = Vec3::ZERO;
                p.starved_ticks = 0;
            }
            g.tick();
            if i % 16 == 15 {
                let _ = drain(&mut rx);
            }
        }
        assert_eq!(g.run.level_phase, protocol::LEVEL_COLLAPSING, "the siren lasts 3.0 s");
        // Drain first so the next snapshot read is the final tick's, not a stale buffered one.
        let _ = drain(&mut rx);
        g.tick();
        let snap = drain(&mut rx).into_iter().find_map(|m| match m {
            ServerMsg::Snapshot {
                collapse_ms,
                level_phase,
                ..
            } => Some((collapse_ms, level_phase)),
            _ => None,
        });
        assert_eq!(snap.map(|s| s.1), Some(protocol::LEVEL_COLLAPSING));
        assert!(
            snap.is_some_and(|s| s.0 > 0),
            "collapse_ms counts down until the last batch falls"
        );
    }

    #[test]
    fn milestone_banner_is_broadcast_at_level_start_for_escalation_levels() {
        let mut g = Game::starting_at(4, 42).unwrap();
        let mut rx = connect(&mut g, 1);
        hello(&mut g, 1);
        drain(&mut rx);
        vote(&mut g, 1); // level 4 starts: Second Step is active
        assert_eq!(g.run.level_phase, protocol::LEVEL_COLLECT);
        assert!(drain(&mut rx).iter().any(|m| matches!(
            m,
            ServerMsg::Notice { kind: NOTICE_EVENT, ids } if ids == &vec!["milestone_4".to_string()]
        )));
    }

    #[test]
    fn vote_opens_with_the_collapse_window_and_the_countdown_closes_it() {
        let mut g = game2();
        let mut rx = connect(&mut g, 1);
        hello(&mut g, 1);
        vote(&mut g, 1);
        drain(&mut rx);
        enter_collapse_and_open_vote(&mut g, 1, &mut [&mut rx]);
        let opened = drain(&mut rx);
        assert!(
            opened.iter().any(|m| matches!(
                m,
                ServerMsg::VoteOpen { options, countdown_s, .. } if options.len() == 3 && *countdown_s == 20
            )),
            "VoteOpen carries three options and a 20 s countdown"
        );
        let safe = player(&mut g, 1).state.pos;
        let mut result: Option<(String, u8, bool)> = None;
        for _ in 0..crate::run::VOTE_COUNTDOWN_TICKS {
            {
                let p = player(&mut g, 1);
                p.state.pos = safe;
                p.state.vel = Vec3::ZERO;
                p.starved_ticks = 0;
            }
            for e in &mut g.run.enemies {
                e.active_in = 1_000;
            }
            g.tick();
            for m in drain(&mut rx) {
                if let ServerMsg::VoteResult {
                    id, kind, permanent, ..
                } = m
                {
                    result = Some((id, kind, permanent));
                }
            }
        }
        let (id, kind, permanent) = result.expect("a VoteResult is broadcast when the countdown closes");
        assert!(g.run.vote_session.is_none(), "the countdown closed the session");
        if kind == CARD_ENEMY {
            if permanent {
                assert_eq!(g.run.permanents, vec![id], "the winner became a permanent");
            } else {
                assert_eq!(g.run.pending_temporary, vec![id], "the winner is a one-level temporary");
            }
        } else {
            assert!(!permanent, "a curse is not a permanent enemy");
            assert!(g.run.curses.contains(&id), "the winning curse was applied");
        }
    }

    #[test]
    fn a_vote_survives_a_disconnect_and_is_re_offered_on_rejoin() {
        let mut g = game2();
        let mut rx1 = connect(&mut g, 1);
        hello(&mut g, 1);
        let mut rx2 = connect(&mut g, 2);
        hello(&mut g, 2);
        vote(&mut g, 1);
        vote(&mut g, 2);
        drain(&mut rx1);
        drain(&mut rx2);
        enter_collapse_and_open_vote(&mut g, 1, &mut [&mut rx1, &mut rx2]);
        let _ = drain(&mut rx1);
        let _ = drain(&mut rx2);
        // Player 2 casts a ballot, then the connection drops.
        g.handle(GameEvent::Message {
            id: 2,
            msg: ClientMsg::Vote { option: 2 },
        });
        g.handle(GameEvent::Disconnected { id: 2 });
        assert_eq!(
            g.run.vote_session.as_ref().map(|s| s.has_voted(2)),
            Some(true),
            "the ballot is keyed by player id and survives the disconnect"
        );
        // Player 2 comes back (a fresh player id, 3) and is offered the session again.
        let mut rx3 = connect(&mut g, 3);
        hello(&mut g, 3);
        let msgs = drain(&mut rx3);
        assert!(
            msgs.iter()
                .any(|m| matches!(m, ServerMsg::VoteOpen { options, .. } if options.len() == 3)),
            "the re-joining player is re-offered the vote"
        );
        assert!(
            msgs.iter()
                .any(|m| matches!(m, ServerMsg::VoteState { voted, .. } if voted == &vec![2])),
            "the re-sent VoteState carries the surviving ballot"
        );
        // The new ballot is cast; the old one still counts.
        g.handle(GameEvent::Message {
            id: 3,
            msg: ClientMsg::Vote { option: 0 },
        });
        let session = g.run.vote_session.as_ref().unwrap();
        assert!(session.has_voted(2) && session.has_voted(3));
        assert_eq!(session.counts(), vec![1, 0, 1]);
    }
}
