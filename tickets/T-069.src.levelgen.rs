// ---- chunk_weight / weighted_pick ----
}

/// Weight for a chunk at a level: prefer lower difficulty, ×3 for climbing tags on HIGH_RISE.
fn chunk_weight(chunk: &Chunk, tier: u32, high_rise: bool) -> f32 {
    let over = chunk.difficulty as f32 - tier as f32;
    let mut w = 1.0 / (1.0 + over.max(0.0));
    if high_rise && chunk.tags.iter().any(|t| t == "climb" || t == "tower" || t == "lift") {
        w *= 3.0;
    }
    w
}

fn weighted_pick(rng: &mut Rng, weights: &[f32]) -> Option<usize> {
    let sum: f32 = weights.iter().copied().sum();
    if !sum.is_finite() || sum <= 0.0 {
        return None;
    }
    let mut roll = rng.f32() * sum;
    for (i, &w) in weights.iter().enumerate() {
        roll -= w;
        if roll <= 0.0 {
            return Some(i);
        }
    }
    Some(weights.len().saturating_sub(1))
}

fn weighted_platform_index(rng: &mut Rng, normal: &[usize], cum: &[f32]) -> usize {

// ---- assemble_level (the growth walk) ----
fn assemble_level(
    cat: &Catalog,
    rng_seed: u32,
    run_seed: u32,
    level: u32,
    players: u32,
    event: u8,
    target_area_u: f32,
    max_chunks: usize,
    max_junctions_u: u32,
    spacing_mul: f32,
) -> Option<GeneratedLevel> {
    let mut rng = Rng::new(seed_mix(rng_seed, level));
    let tier = level_tier(level);
    let high_rise = event == EVENT_HIGH_RISE;

    // --- hub at the origin (unrotated).
    let hub_choices: Vec<usize> = cat
        .chunks
        .iter()
        .enumerate()
        .filter(|(_, c)| c.role == ChunkRole::Hub && c.min_level <= level)
        .map(|(i, _)| i)
        .collect();
    if hub_choices.is_empty() {
        return None;
    }
    let hub_weights: Vec<f32> = hub_choices
        .iter()
        .map(|&i| chunk_weight(&cat.chunks[i], tier, false))
        .collect();
    let hub_pick = weighted_pick(&mut rng, &hub_weights)?;
    let hub_chunk = &cat.chunks[hub_choices[hub_pick]];
    let hub_geom = build_geometry(hub_chunk, 0, Vec3::new(0.0, 0.0, 0.0));
    let beacon = hub_geom.beacon?; // hubs are guaranteed a beacon by parse_catalog
    let enemy_spawns = hub_geom.enemy_slots.clone();
    let hub_top_area = hub_geom.top_area;
    let mut gift_slots = hub_geom.gift_slots;
    let mut altar_slots = hub_geom.altar_slots;
    // Platform 0 = the hub's main platform.
    let mut hub_plats = hub_geom.platforms;
    let main_idx = main_platform_index(&hub_plats);
    hub_plats.swap(0, main_idx);

    let mut platforms: Vec<Platform> = Vec::new();
    let mut grid = CoarseGrid::new();
    let mut next_id: u16 = 0;
    commit_platforms(&mut platforms, &mut grid, hub_plats, &mut next_id)?;
    let hub_platform_count = platforms.len();
    let mut area_sum = hub_top_area;
    let mut open: Vec<OpenSocket> = hub_geom.sockets;

    // --- budget-driven random walk over open sockets. The random pick may target any open
    // socket (including earlier-placed ones); when the walk stalls (>= 8 consecutive failed
    // picks) every open socket is swept in order before the map is accepted. Per A1 the walk
    // stops accepting new failures once at least 70% of the area budget is reached.
    let mut chunk_count = 1usize;
    let mut junctions_used = 0u32;
    let mut dropped: Vec<OpenSocket> = Vec::new();
    let mut iters = 0usize;
    let mut consecutive_failures = 0u32;
    while iters < MAX_GROWTH_ITERS && chunk_count < max_chunks && area_sum < target_area_u && !open.is_empty() {
        iters += 1;
        let oi = rng.below(open.len());
        let mut placed: Option<(usize, Placement, usize)> = try_place(
            cat,
            &open[oi],
            &platforms,
            &grid,
            level,
            tier,
            high_rise,
            junctions_used,
            max_junctions_u,
            &mut rng,
        )
        .map(|(p, used)| (oi, p, used));
        if placed.is_none() && consecutive_failures >= 8 {
            for k in 0..open.len() {
                if k == oi {
                    continue;
                }
                if let Some((p, used)) = try_place(
                    cat,
                    &open[k],
                    &platforms,
                    &grid,
                    level,
                    tier,
                    high_rise,
                    junctions_used,
                    max_junctions_u,
                    &mut rng,
                ) {
                    placed = Some((k, p, used));
                    break;
                }
            }
        }
        match placed {
            Some((k, p, used)) => {
                commit_placement(
                    &mut open,
                    k,
                    p,
                    used,
                    &mut platforms,
                    &mut grid,
                    &mut next_id,
                    &mut gift_slots,
                    &mut altar_slots,
                    &mut area_sum,
                    &mut chunk_count,
                    &mut junctions_used,
                )?;
                consecutive_failures = 0;
            }
            None => {
                consecutive_failures += 1;
                open[oi].failures = open[oi].failures.saturating_add(1);
                if open[oi].failures >= 3 {
                    let s = open.swap_remove(oi);
                    dropped.push(s);
                }
                // A1: accept the map once >= 70% of the area budget is reached and the walk
                // is stalled; remaining open sockets are bridged or sealed below.
                if consecutive_failures >= STALL_LIMIT && area_sum >= BUDGET_ACCEPT * target_area_u {
                    break;
                }
            }
        }
    }

    // --- A1: close loops with generated bridges, then seal the rest.
    open.append(&mut dropped);
    let _bridges = close_loops(&mut open, &mut platforms, &mut grid, &mut next_id)?;
    let _stubs = seal_open_sockets(cat, level, &mut rng, &mut open, &mut platforms, &mut grid, &mut next_id)?;

    // --- crumble (event) BEFORE gifts: gift filling only accepts NORMAL tops, so a crumbled
    // platform can never carry a gift and the A3 density cap stays consistent with validate.
    let spawn = spawn_point(&platforms[0].bounds, &beacon);
    let mut slot_supports = vec![false; platforms.len()];
    for (pos, _) in &gift_slots {
        let top = pos.y - 0.3;
        if let Some(i) = platform_top_under(&platforms, pos.x, pos.z, top, 0.0) {
            slot_supports[i] = true;
        }
    }
    for pos in &altar_slots {
        if let Some(i) = platform_top_under(&platforms, pos.x, pos.z, pos.y, 0.0)
            .or_else(|| platform_top_under(&platforms, pos.x, pos.z, pos.y - 0.3, 0.0))
        {
            slot_supports[i] = true;
        }
    }
    if event == EVENT_CRUMBLE {
        apply_crumble(&mut platforms, hub_platform_count, &slot_supports, run_seed, level);
    }

    // --- gifts (A3), altars (legacy).
    let fill = fill_gifts(&platforms, &gift_slots, &beacon, level, players, &mut rng, spacing_mul);
    let gifts = fill.gifts;
    let altar = choose_altar(
        &altar_slots,
        &platforms,
        &gifts,
        &spawn,
        &beacon,
        &enemy_spawns,
        run_seed,
        level,
        &mut rng,
    );
    let altars = match altar {
        Some((a, _)) => vec![a],
        None => Vec::new(),
    };

    let kill_y = platforms.iter().map(|p| p.bounds.min.y).fold(f32::INFINITY, f32::min) - 15.0;
    let data = LevelData {
        level,
        seed: run_seed,
        spawn,
        beacon,
        kill_y,
        event,
        altars,
        platforms,
        gifts,
    };
    Some(GeneratedLevel { data, enemy_spawns })
}

/// Builds a level from the catalog with the design 4.3 / amendment A1 budget formulas.
fn try_build(
    cat: &Catalog,
    rng_seed: u32,
    run_seed: u32,
    level: u32,

// ---- try_place (chunk selection per socket) ----
fn try_place(
    cat: &Catalog,
    anchor: &OpenSocket,
    platforms: &[Platform],
    grid: &CoarseGrid,
    level: u32,
    tier: u32,
    high_rise: bool,
    junctions_used: u32,
    max_junctions_u: u32,
    rng: &mut Rng,
) -> Option<(Placement, usize)> {
    for _ in 0..CANDIDATE_TRIALS {
        let eligible: Vec<usize> = cat
            .chunks
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                c.min_level <= level
                    && match c.role {
                        ChunkRole::Path => true,
                        ChunkRole::Junction => junctions_used < max_junctions_u,
                        ChunkRole::Hub | ChunkRole::DeadEnd => false,
                    }
            })
            .map(|(i, _)| i)
            .collect();
        if eligible.is_empty() {
            return None;
        }
        let weights: Vec<f32> = eligible
            .iter()
            .map(|&i| chunk_weight(&cat.chunks[i], tier, high_rise))
            .collect();
        let chosen = weighted_pick(rng, &weights)?;
        let ci = eligible[chosen];
        let chunk = &cat.chunks[ci];
        let mut valid: Vec<(Placement, usize)> = Vec::new();
        for (si, _) in chunk.sockets.iter().enumerate() {
            for turns in 0..4u32 {
                let Some(p) = plan_join(chunk, si, turns, anchor) else {
                    continue;
                };
                if placement_fits(&p, platforms, grid) {
                    valid.push((p, si));
                }
            }
        }
        if !valid.is_empty() {
            let pick = rng.below(valid.len());
            return Some(valid.swap_remove(pick));
        }
    }
    None
}

/// Commits a placement made by [`try_place`] (or the growth sweep). `at` is the index of the
/// consumed anchor in `open`. Ids are assigned by [`commit_platforms`].
#[allow(clippy::too_many_arguments)]
fn commit_placement(
    open: &mut Vec<OpenSocket>,
    at: usize,
    placement: Placement,
    used_socket: usize,
    platforms: &mut Vec<Platform>,
    grid: &mut CoarseGrid,
    next_id: &mut u16,
    gift_slots: &mut Vec<(Vec3, bool)>,
    altar_slots: &mut Vec<Vec3>,
    area_sum: &mut f32,
    chunk_count: &mut usize,
    junctions_used: &mut u32,
) -> Option<()> {
    let p = placement;
    open.swap_remove(at);
    let mut socks = p.sockets;
    if used_socket < socks.len() {
        socks.swap_remove(used_socket);
    }
    open.extend(socks);
    *area_sum += p.top_area;
    *chunk_count += 1;
    if p.role == ChunkRole::Junction {
        *junctions_used += 1;
    }
    gift_slots.extend(p.gift_slots);
    altar_slots.extend(p.altar_slots);
    commit_platforms(platforms, grid, p.platforms, next_id)
}

/// Commits the railing-less 3 u stub for `socket` iff it satisfies the placement rules.
/// Returns `true` when the stub was placed.
fn commit_stub(
    socket: &OpenSocket,
    platforms: &mut Vec<Platform>,
    grid: &mut CoarseGrid,
    next_id: &mut u16,
) -> Option<bool> {
    let bounds = stub_bounds(socket);
    for idx in grid.candidates(&bounds) {
        if !platform_pair_ok(&bounds, &platforms[idx].bounds) {
            return Some(false);
        }
    }
    commit_platforms(platforms, grid, vec![(PLATFORM_NORMAL, bounds)], next_id)?;
    Some(true)
}

/// Test hook: runs the full generate pipeline (regeneration attempts, then the
/// guaranteed-valid fallbacks) against an explicit catalog instead of the shared one.
/// Normal callers want [`generate`] / [`generate_for`].
pub fn generate_with_catalog(cat: &Catalog, seed: u32, level: u32, players: u32) -> GeneratedLevel {
    generate_with_catalog_spacing(cat, seed, level, players, 1.0)
}

/// [`generate_with_catalog`] with an explicit gift-spacing multiplier (Clustered Gifts curse).
fn generate_with_catalog_spacing(
    cat: &Catalog,
