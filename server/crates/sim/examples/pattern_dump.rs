//! T-077 generator dump: one NDJSON level record per line, for `tools/pattern-audit`.
//!
//! Usage: `cargo run -p sim --release --example pattern_dump -- <seeds>`
//!
//! The record shape is the contract in `tools/pattern-audit/README.md`. `patterns[].platforms`
//! is the pattern's OWN local geometry from the catalog, not its placed world position: the
//! audit's geometry metrics (ramp rules, ramp sequences, height deltas, reference scale) are
//! properties of the pattern, and the placed level is already covered by `validate()`.
//!
//! Levels are generated for the three shipped player counts (1, 4, 8) so the audit sees the
//! same spread the server does.

use std::collections::BTreeMap;

use sim::levelgen::{catalog, generate_for_with_spacing};
use sim::pattern::PatternMeta;

/// Platform kind names the audit understands.
fn kind_name(kind: u8) -> &'static str {
    match kind {
        protocol::PLATFORM_CRUMBLE => "CRUMBLE",
        protocol::PLATFORM_JUMP_PAD => "JUMP_PAD",
        protocol::PLATFORM_RAMP_PX => "RAMP_PX",
        protocol::PLATFORM_RAMP_NX => "RAMP_NX",
        protocol::PLATFORM_RAMP_PZ => "RAMP_PZ",
        protocol::PLATFORM_RAMP_NZ => "RAMP_NZ",
        _ => "NORMAL",
    }
}

fn num(v: f32) -> String {
    // Two decimals is plenty for the audit's thresholds and keeps the stream small.
    format!("{v:.2}")
}

fn main() {
    let seeds: u32 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(5000);

    let cat = match catalog() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("pattern_dump: catalog failed: {e}");
            std::process::exit(2);
        }
    };

    // id -> (themes, mechs, local platforms) for every catalog entry.
    let mut meta: BTreeMap<String, (Vec<String>, Vec<String>, String)> = BTreeMap::new();
    for chunk in &cat.chunks {
        let m = PatternMeta::from_tags(
            &chunk.id,
            chunk.role,
            chunk.min_level,
            chunk.difficulty as u8,
            &chunk.tags,
        );
        let themes: Vec<String> = m.theme.iter().map(|t| t.as_str().to_string()).collect();
        let mechs: Vec<String> = m.mechanic.iter().map(|t| t.as_str().to_string()).collect();
        let mut plats = String::new();
        for p in &chunk.platforms {
            if !plats.is_empty() {
                plats.push(',');
            }
            plats.push_str(&format!(
                "{{\"kind\":\"{}\",\"min\":[{},{},{}],\"max\":[{},{},{}]}}",
                kind_name(p.kind),
                num(p.min.x),
                num(p.min.y),
                num(p.min.z),
                num(p.max.x),
                num(p.max.y),
                num(p.max.z)
            ));
        }
        meta.insert(chunk.id.clone(), (themes, mechs, plats));
    }

    let levels: [u32; 5] = [1, 3, 6, 10, 15];
    let players: [u32; 3] = [1, 4, 8];
    let mut out = String::new();
    for seed in 0..seeds {
        for &level in &levels {
            for &pc in &players {
                let built = generate_for_with_spacing(seed, level, pc, 1.0);
                let plan = &built.plan;
                let mut pats = String::new();
                for id in plan {
                    if !pats.is_empty() {
                        pats.push(',');
                    }
                    let (themes, mechs, plats) = meta
                        .get(id)
                        .cloned()
                        .unwrap_or_else(|| (Vec::new(), Vec::new(), String::new()));
                    let themes = themes.iter().map(|t| format!("\"{t}\"")).collect::<Vec<_>>().join(",");
                    let mechs = mechs.iter().map(|t| format!("\"{t}\"")).collect::<Vec<_>>().join(",");
                    pats.push_str(&format!(
                        "{{\"id\":\"{id}\",\"themes\":[{themes}],\"mechs\":[{mechs}],\"platforms\":[{plats}]}}"
                    ));
                }
                let plan_json = plan.iter().map(|id| format!("\"{id}\"")).collect::<Vec<_>>().join(",");
                let enemies = built
                    .enemy_spawns
                    .iter()
                    .map(|_| "{\"type\":\"spawn\"}")
                    .collect::<Vec<_>>()
                    .join(",");
                // A level with no plan came from a fallback; report it so metric 3 sees it.
                let fallbacks = if plan.is_empty() {
                    "{\"kind\":\"no_plan\",\"detail\":\"catalog walk produced no plan\"}"
                } else {
                    ""
                };
                out.push_str(&format!(
                    "{{\"seed\":{seed},\"level\":{level},\"players\":{pc},\"plan\":[{plan_json}],\"patterns\":[{pats}],\"enemies\":[{enemies}],\"fallbacks\":[{fallbacks}]}}\n"
                ));
            }
        }
    }
    print!("{out}");
}
