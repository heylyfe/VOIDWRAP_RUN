// The frozen enemy registry, transcribed from design/ENEMY_REGISTRY.md §10 (statuses FROZEN, T-072).
// This is spec data, not server source: the conformance kit and the stub both read it from here so
// that pool assertions and pool behaviour share one transcription of the contract. If this table and
// design/content.json ever disagree, the kit's FAILs are a reportable mismatch.

export const POOL_OFFER = 3; // T-072 invention: how many candidates the vote draws, in pool order

// Array order is the content order used to break min_level ties (and the network kind index).
export const ENEMIES = [
  { id: "pallbearer", name: "Footprint Chaser", min_level: 1, cap: 2, tags: ["chase", "large"], class_interactions: { striker: "counters", glider: "ignores" }, hooks: [] },
  { id: "snapshrike", name: "Orbit Dasher", min_level: 3, cap: 1, tags: ["chase", "small"], class_interactions: { striker: "counters", bulwark: "immune" }, hooks: [] },
  { id: "moteling", name: "Stillness Shrieker", min_level: 1, cap: 1, tags: ["zone", "debuff", "small"], class_interactions: { bulwark: "immune" }, hooks: [] },
  { id: "rift_warden", name: "Wall Drifter", min_level: 2, cap: 1, tags: ["zone", "large"], class_interactions: { grappler: "counters", bulwark: "immune" }, hooks: [] },
  { id: "gale_wight", name: "Shell Bomber", min_level: 4, cap: 1, tags: ["zone", "ambush"], class_interactions: { glider: "counters" }, hooks: [] },
  { id: "lantern_leech", name: "Gift Stinger", min_level: 2, cap: 1, tags: ["chase", "loot", "small"], class_interactions: { grappler: "counters" }, hooks: [] },
  { id: "turret", name: "Bolt Turret", min_level: 2, cap: 1, tags: ["zone"], class_interactions: { bulwark: "immune", glider: "counters" }, hooks: [] },
  { id: "mimic", name: "False Gift", min_level: 3, cap: 2, tags: ["ambush", "small"], class_interactions: { striker: "counters" }, hooks: [] },
  { id: "corrupter", name: "Floor Rotter", min_level: 2, cap: 1, tags: ["zone", "debuff", "collapse"], class_interactions: { glider: "immune", grappler: "counters" }, hooks: [] },
  { id: "rewind", name: "Path Replayer", min_level: 1, cap: 2, tags: ["chase", "zone"], class_interactions: { striker: "counters", grappler: "immune" }, hooks: [] },
  { id: "warden", name: "Gift Guard", min_level: 2, cap: 1, tags: ["chase", "loot"], class_interactions: { grappler: "counters", bulwark: "immune" }, hooks: [] },
  { id: "peal", name: "Bell Ringer", min_level: 4, cap: 1, tags: ["zone", "debuff"], class_interactions: { bulwark: "immune", glider: "counters" }, hooks: [{ with: "coil", effect: "peal_lethal_when_overtuned" }] },
  { id: "kite", name: "Tether Tracker", min_level: 6, cap: 1, tags: ["chase", "zone"], class_interactions: { striker: "counters", grappler: "counters" }, hooks: [] },
  { id: "static", name: "Fog Dasher", min_level: 5, cap: 2, tags: ["fog", "chase"], class_interactions: { glider: "ignores", bulwark: "immune" }, hooks: [] },
  { id: "blight", name: "Ground Infector", min_level: 8, cap: 1, tags: ["debuff", "chase"], class_interactions: { glider: "immune", grappler: "counters" }, hooks: [{ with: "rewind", effect: "infection_pauses_timer" }, { with: "kite", effect: "infection_pauses_timer" }] },
  { id: "metronome", name: "Beat Keeper", min_level: 10, cap: 1, tags: ["zone", "debuff"], class_interactions: { glider: "counters", bulwark: "immune" }, hooks: [] },
  { id: "overlook", name: "Marking Eye", min_level: 12, cap: 1, tags: ["zone", "chase"], class_interactions: { striker: "counters", glider: "ignores" }, hooks: [] },
  { id: "chorus", name: "Choir Summoner", min_level: 10, cap: 1, tags: ["summon", "small"], class_interactions: { striker: "counters", bulwark: "immune" }, hooks: [{ requires: { tag: "small", count: 2 }, effect: "chorus_needs_small_pair" }] },
  { id: "coil", name: "Spring Leaper", min_level: 4, cap: 2, tags: ["zone", "debuff"], class_interactions: { striker: "counters", glider: "ignores" }, hooks: [] },
  { id: "lure", name: "Fog Decoy", min_level: 3, cap: 2, tags: ["fog", "ambush", "collapse"], class_interactions: { glider: "immune", grappler: "counters" }, hooks: [] },
];

export const ENEMY_BY_ID = new Map(ENEMIES.map((entry) => [entry.id, entry]));

export const isKnownEnemy = (id) => ENEMY_BY_ID.has(id);
export const minLevelOf = (id) => ENEMY_BY_ID.get(id)?.min_level;
export const capOf = (id) => ENEMY_BY_ID.get(id)?.cap;
export const enemyName = (id) => ENEMY_BY_ID.get(id)?.name;
