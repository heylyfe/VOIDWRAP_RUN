/// fallback direction, because those abilities do nothing without an aim.
fn aim_dir(wx: f32, wz: f32, vel: &Vec3) -> Option<Vec3> {
    let wish_len_sq = wx * wx + wz * wz;
    if wish_len_sq > 0.0 {
        let len = wish_len_sq.sqrt();
        return Some(Vec3::new(wx / len, 0.0, wz / len));
    }
    let vel_len_sq = vel.x * vel.x + vel.z * vel.z;
    if vel_len_sq > 0.0 {
        let len = vel_len_sq.sqrt();
        return Some(Vec3::new(vel.x / len, 0.0, vel.z / len));
    }
    None
}

/// True when `bounds` overlap any currently solid platform. Only the platforms
/// in the grid cells overlapped by `bounds` are tested.
fn overlaps_any_solid(world: &World, bounds: &Aabb) -> bool {
    world
        .solid_candidates(bounds.min.x, bounds.min.z, bounds.max.x, bounds.max.z)
        .into_iter()
        .any(|index| match world.solid_bounds(index) {
            Some(solid) => overlaps(bounds, solid),
            None => false,
        })
}

// CLASS_BULWARK no longer blinks (M3): the blink was replaced by the braced
// wind-up and the ward. Intentionally deleted; see `bulwark_pulse_frame` and
// the CLASS_BULWARK arm in `step_with`.

/// CLASS_GRAPPLER: pick a grapple target automatically and begin the pull.
///
/// Candidates are the points on solid platforms' top faces closest (in x/z, y
/// fixed at the top) to `pos + aim * GRAPPLE_AIM_DISTANCE`. A candidate is valid
/// when its 3D distance from the feet is within [`GRAPPLE_MIN_RANGE`]..=[`GRAPPLE_RANGE`],
/// its y is at least `pos.y - GRAPPLE_MAX_BELOW`, and the horizontal direction
/// to it dots with the aim at least [`GRAPPLE_MIN_DOT`]. The valid candidate
/// closest to the anchor wins; ties go to the lowest platform id. With no aim,
/// or no valid candidate, nothing happens and no cooldown starts.
///
/// On a fire the stored `ability_dir` is the target POINT (top point +
/// [`GRAPPLE_TARGET_HEIGHT`]), not a direction axis.
fn try_grapple(state: &mut ControllerState, input: &InputFrame, world: &World, cooldown: u16) {
    let (wx, wz) = wish_dir(input);
    let Some(aim) = aim_dir(wx, wz, &state.vel) else {
        return; // no aim: no grapple, no cooldown
    };
    let anchor = Vec3::new(
        state.pos.x + aim.x * GRAPPLE_AIM_DISTANCE,
        state.pos.y,
        state.pos.z + aim.z * GRAPPLE_AIM_DISTANCE,
    );
    // (distance squared to the anchor, platform id, target point).
    let mut best: Option<(f32, u16, Vec3)> = None;
    for platform in &world.platforms {
        if world.gone.contains(&platform.id) {
            continue; // TILE_GONE platforms are not solid
        }
        let point = Vec3::new(
            anchor.x.clamp(platform.bounds.min.x, platform.bounds.max.x),
            platform.bounds.max.y,
            anchor.z.clamp(platform.bounds.min.z, platform.bounds.max.z),
        );
        let dx = point.x - state.pos.x;
        let dy = point.y - state.pos.y;
        let dz = point.z - state.pos.z;
        let dist_sq = dx * dx + dy * dy + dz * dz;
        if dist_sq < GRAPPLE_MIN_RANGE * GRAPPLE_MIN_RANGE || dist_sq > GRAPPLE_RANGE * GRAPPLE_RANGE {
            continue;
        }
        if point.y < state.pos.y - GRAPPLE_MAX_BELOW {
            continue;
        }
        let hlen_sq = dx * dx + dz * dz;
        if hlen_sq == 0.0 {
            continue; // no horizontal direction to P
        }
        let dot = (dx * aim.x + dz * aim.z) / hlen_sq.sqrt();
        if dot < GRAPPLE_MIN_DOT {
            continue;
        }
        let ax = point.x - anchor.x;
        let ay = point.y - anchor.y;
        let az = point.z - anchor.z;
        let anchor_dist_sq = ax * ax + ay * ay + az * az;
        let better = match &best {
            None => true,
            Some((best_dist, best_id, _)) => {
                anchor_dist_sq < *best_dist || (anchor_dist_sq == *best_dist && platform.id < *best_id)
            }
        };
        if better {
            best = Some((anchor_dist_sq, platform.id, point));
        }
    }
    let Some((_, _, point)) = best else {
        return; // no valid target: no grapple, no cooldown
    };
    state.ability_dir = Vec3::new(point.x, point.y + GRAPPLE_TARGET_HEIGHT, point.z);
    state.ability_active_frames = GRAPPLE_MAX_FRAMES;
    state.ability_cooldown_frames = cooldown;
}

/// True when the feet rest on a solid top (within [`GROUND_EPSILON`]) while
/// overlapping it in x/z and not moving upwards. Needed because a dash zeroes
/// `vel.y`, which makes the downward Y move (and therefore `landed`) zero.
fn resting_on_solid(state: &ControllerState, world: &World) -> bool {
    if state.vel.y > 0.0 {
        return false;
    }
    let b = player_box(&state.pos);
    for index in world.solid_candidates(b.min.x, b.min.z, b.max.x, b.max.z) {
        let Some(solid) = world.solid_bounds(index) else {
            continue;
        };
        let overlaps_xz =
            b.min.x < solid.max.x && b.max.x > solid.min.x && b.min.z < solid.max.z && b.max.z > solid.min.z;
        if overlaps_xz && (state.pos.y - solid.max.y).abs() <= GROUND_EPSILON {
            return true;
        }
    }
    false
}

#[derive(Clone, Copy)]
enum Axis {
    X,
    Y,
    Z,
}

fn get(v: &Vec3, a: Axis) -> f32 {
    match a {
        Axis::X => v.x,
        Axis::Y => v.y,
        Axis::Z => v.z,
    }
}

fn set(v: &mut Vec3, a: Axis, value: f32) {
    match a {
        Axis::X => v.x = value,
        Axis::Y => v.y = value,
        Axis::Z => v.z = value,
    }
}

/// Moves along one axis, stopping flush against the first solid hit.
/// Returns true if the move was a downward move that landed on something.
///
/// Amendment A2 (step-up): a horizontal move blocked by a solid platform whose
/// top is at most [`STEP_HEIGHT`] above the feet is turned into a step up onto
/// that platform (the player is raised so the feet rest on the top), keeping
/// horizontal motion. This happens only when the player is grounded or within
/// coyote frames and only when the player box fits at the new height.
///
/// Only platforms in the grid cells overlapped by the swept player box are
/// tested; the iteration order matches platform order, so the result is
/// identical to brute force.
fn move_axis(state: &mut ControllerState, world: &World, axis: Axis, delta: f32) -> bool {
    if delta == 0.0 {
        return false;
    }
    let mut pos = state.pos;
    let target = get(&pos, axis) + delta;
    set(&mut pos, axis, target);
    // Swept box (XZ) used to pick candidate platforms from the spatial grid.
    let start_box = player_box(&state.pos);
    let end_box = player_box(&pos);
    let min_x = start_box.min.x.min(end_box.min.x);
    let max_x = start_box.max.x.max(end_box.max.x);
    let min_z = start_box.min.z.min(end_box.min.z);
    let max_z = start_box.max.z.max(end_box.max.z);
    let mut hit = false;
    for index in world.solid_candidates(min_x, min_z, max_x, max_z) {
        let Some(solid) = world.solid_bounds(index) else {
            continue;
        };
        let b = player_box(&pos);
        if !overlaps(&b, solid) {
            continue;
        }
        // Step-up attempt for horizontal moves only.
        if matches!(axis, Axis::X | Axis::Z) && (state.grounded || state.coyote_frames > 0) {
            let feet = pos.y;
            let top = solid.max.y;
            if top > feet && top - feet <= STEP_HEIGHT {
                let stepped = Vec3::new(pos.x, top, pos.z);
                if !overlaps_any_solid(world, &player_box(&stepped)) {
                    pos = stepped; // step up; horizontal motion is kept
                    continue;
                }
            }
        }
        hit = true;
        // Push back flush against the face we entered through.
        let p = get(&pos, axis);
        let resolved = match axis {
            Axis::Y if delta < 0.0 => solid.max.y,
            Axis::Y => solid.min.y - PLAYER_HEIGHT,
            _ if delta < 0.0 => get(&solid.max, axis) + PLAYER_HALF_WIDTH,
            _ => get(&solid.min, axis) - PLAYER_HALF_WIDTH,
        };
        // Only ever push backwards along the motion, never forwards.
        let clamped = if delta < 0.0 { resolved.max(p) } else { resolved.min(p) };
        set(&mut pos, axis, clamped);
    }
    if hit {
        set(&mut state.vel, axis, 0.0);
    }
    state.pos = pos;
    hit && matches!(axis, Axis::Y) && delta < 0.0
}

#[cfg(test)]
#[allow(clippy::explicit_counter_loop)] // frame counters double as input sequence numbers
mod tests {
    use super::*;
    use crate::levels;
    use protocol::{EVENT_NONE, LevelData, PLATFORM_NORMAL, Platform, TILE_GONE, TILE_WARN, TileState};

    fn input(seq: u32, move_x: i8, move_z: i8, buttons: u8) -> InputFrame {
        InputFrame {
            seq,
            move_x,
            move_z,
            buttons,
        }
    }

    fn idle(seq: u32) -> InputFrame {
        input(seq, 0, 0, 0)
    }

    fn run_x(seq: u32, buttons: u8) -> InputFrame {
        input(seq, 127, 0, buttons)
    }

    fn world() -> World {
        World::from_level(&levels::bootstrap())
