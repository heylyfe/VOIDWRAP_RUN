//! T-069 composition harness.
//!
//! Run with `cargo test -p sim --test patterns -- --nocapture` to read the summary.
//!
//! Two layers are checked:
//!
//! * the **plan layer** ([`CompositionPlan`]) over a tagged pattern set, because the
//!   shipped catalog is still untagged until T-070/T-071 land: divergence, repeat
//!   rates and theme diversity;
//! * the **generator layer** ([`generate_for_with_spacing`]) over the real catalog:
//!   every level validates, the same seed reproduces the same level, and different
//!   seeds produce different geometry.

use std::collections::BTreeSet;

use sim::LevelData;
use sim::levelgen::{ChunkRole, generate_for_with_spacing, validate};
use sim::pattern::{CompositionPlan, PatternMeta};

/// One platform reduced to the six integers that identify its shape and place.
type PlatformFingerprint = (u32, u32, u32, u32, u32, u32);

/// Seeds per level for the plan layer.
const PLAN_SEEDS: u32 = 1_000;

/// Seeds per level for the generator layer.
const GEN_SEEDS: u32 = 1_000;

/// Seeds used for the reproducibility check.
const REPRO_SEEDS: u32 = 100;

/// Levels exercised by both layers.
const LEVELS: [u32; 3] = [1, 5, 10];

/// Levels exercised by the generator layer.
const GEN_LEVELS: [u32; 2] = [1, 8];

/// A tagged stand-in for the catalog: 2 hubs, 12 paths, 4 junctions, 8 themes.
///
/// The shipped `design/chunks.json` carries no `theme:`/`mech:` tags yet, so the
/// plan layer is exercised against this set. It is built through the real
/// [`PatternMeta::from_tags`] parser, so the tag vocabulary is covered too.
const SPECS: [(&str, ChunkRole, u32, u8, &[&str]); 18] = [
    (
        "pt01_ember_hub",
        ChunkRole::Hub,
        1,
        1,
        &["theme:ember", "mech:rest", "mech:loot", "mech:fork"],
    ),
    (
        "pt02_frost_hub",
        ChunkRole::Hub,
        4,
        2,
        &["theme:frost", "mech:rest", "mech:fork"],
    ),
    (
        "pt03_ember_climb",
        ChunkRole::Path,
        1,
        1,
        &["theme:ember", "mech:climb"],
    ),
    ("pt04_ember_span", ChunkRole::Path, 1, 2, &["theme:ember", "mech:span"]),
    ("pt05_void_drop", ChunkRole::Path, 1, 2, &["theme:void", "mech:drop"]),
    ("pt06_void_crawl", ChunkRole::Path, 1, 2, &["theme:void", "mech:crawl"]),
    ("pt07_frost_span", ChunkRole::Path, 1, 1, &["theme:frost", "mech:span"]),
    (
        "pt08_frost_bounce",
        ChunkRole::Path,
        1,
        2,
        &["theme:frost", "mech:bounce", "hazard:trampoline"],
    ),
    (
        "pt09_ruin_gauntlet",
        ChunkRole::Path,
        1,
        3,
        &["theme:ruin", "mech:gauntlet", "hazard:tripmine"],
    ),
    ("pt10_ruin_climb", ChunkRole::Path, 1, 2, &["theme:ruin", "mech:climb"]),
    (
        "pt11_garden_rest",
        ChunkRole::Path,
        1,
        1,
        &["theme:garden", "mech:rest", "mech:loot"],
    ),
    (
        "pt12_garden_span",
        ChunkRole::Path,
        1,
        1,
        &["theme:garden", "mech:span"],
    ),
    (
        "pt13_machine_gauntlet",
        ChunkRole::Path,
        1,
        3,
        &["theme:machine", "mech:gauntlet", "hazard:fog"],
    ),
    (
        "pt14_machine_drop",
        ChunkRole::Path,
        1,
        2,
        &["theme:machine", "mech:drop"],
    ),
    (
        "pt15_spire_climb",
        ChunkRole::Junction,
        1,
        2,
        &["theme:spire", "mech:climb", "mech:fork"],
    ),
    (
        "pt16_spire_bounce",
        ChunkRole::Junction,
        1,
        3,
        &["theme:spire", "mech:bounce", "mech:fork"],
    ),
    (
        "pt17_hollow_crawl",
        ChunkRole::Junction,
        1,
        2,
        &["theme:hollow", "mech:crawl", "mech:fork"],
    ),
    (
        "pt18_hollow_gauntlet",
        ChunkRole::Junction,
        1,
        3,
        &["theme:hollow", "mech:gauntlet", "mech:fork"],
    ),
];

/// Builds the tagged pattern set through the real tag parser.
fn tagged_patterns() -> Vec<PatternMeta> {
    SPECS
        .iter()
        .map(|(id, role, min_level, difficulty, tags)| {
            let owned: Vec<String> = tags.iter().map(|tag| (*tag).to_string()).collect();
            PatternMeta::from_tags(id, *role, *min_level, *difficulty, &owned)
        })
        .collect()
}

/// A geometry fingerprint: every platform box, bit-exact.
fn signature(data: &LevelData) -> Vec<(u32, u32, u32, u32, u32, u32)> {
    data.platforms
        .iter()
        .map(|p| {
            (
                p.bounds.min.x.to_bits(),
                p.bounds.min.y.to_bits(),
                p.bounds.min.z.to_bits(),
                p.bounds.max.x.to_bits(),
                p.bounds.max.y.to_bits(),
                p.bounds.max.z.to_bits(),
            )
        })
        .collect()
}

#[test]
fn composition_plans_diverge_and_never_repeat() {
    let metas = tagged_patterns();
    let mut distinct: BTreeSet<String> = BTreeSet::new();
    let mut plans = 0usize;
    let mut picks = 0usize;
    let mut immediate = 0usize;
    let mut within_six = 0usize;
    let mut theme_total = 0usize;

    for &level in &LEVELS {
        for seed in 0..PLAN_SEEDS {
            let plan = CompositionPlan::build(&metas, seed, level, 1);
            assert!(!plan.is_empty(), "empty plan for seed {seed} level {level}");
            let ids = plan.ids();
            distinct.insert(ids.join(","));
            plans += 1;
            picks += ids.len();

            for pair in ids.windows(2) {
                if pair[0] == pair[1] {
                    immediate += 1;
                }
            }
            for i in 0..ids.len() {
                let end = (i + 7).min(ids.len());
                if ids[i + 1..end].iter().any(|other| other == &ids[i]) {
                    within_six += 1;
                }
            }

            let mut themes: BTreeSet<&str> = BTreeSet::new();
            for id in ids {
                if let Some(meta) = metas.iter().find(|m| &m.id == id) {
                    for theme in &meta.theme {
                        themes.insert(theme.as_str());
                    }
                }
            }
            theme_total += themes.len();
        }
    }

    let within_six_rate = within_six as f64 / picks as f64;
    let theme_average = theme_total as f64 / plans as f64;

    println!(
        "plan layer: {plans} plans, {} distinct lists, {picks} picks, \
         immediate repeats {immediate}, within-6 repeats {within_six} ({:.3}%), \
         avg distinct themes {:.2}",
        distinct.len(),
        within_six_rate * 100.0,
        theme_average
    );

    assert_eq!(immediate, 0, "the bag returned the same pattern twice in a row");
    // Uniform baseline over the 16 walk patterns is 6/16 = 37.5%; the bag's 1/32
    // recency divisor targets ~1.5%, so 5% is a generous ceiling.
    assert!(
        within_six_rate < 0.05,
        "within-6 repeat rate {within_six_rate:.4} exceeds 0.05"
    );
    assert_eq!(
        distinct.len(),
        plans,
        "two seeds produced the same ordered pattern list"
    );
    assert!(
        theme_average >= 2.0,
        "average distinct themes per level {theme_average:.2} is below 2"
    );
}

/// The shipped catalog is the 25 legacy chunks plus the two authored batches. A
/// regression that drops a catalog file would silently shrink the pool, so count the
/// ids in the three files the sim crate merges.
#[test]
fn shipped_catalog_has_at_least_fifty_patterns() {
    const SOURCES: [&str; 3] = [
        include_str!("../../../../design/chunks.json"),
        include_str!("../../../../design/patterns-a.json"),
        include_str!("../../../../design/patterns-b.json"),
    ];
    let mut ids: BTreeSet<String> = BTreeSet::new();
    for source in SOURCES {
        let doc: serde_json::Value = serde_json::from_str(source).expect("catalog file is valid JSON");
        let list = doc
            .get("chunks")
            .or_else(|| doc.get("patterns"))
            .and_then(serde_json::Value::as_array)
            .expect("catalog file has a chunks or patterns array");
        for entry in list {
            let id = entry
                .get("id")
                .and_then(serde_json::Value::as_str)
                .expect("pattern has an id");
            assert!(ids.insert(id.to_string()), "duplicate pattern id {id}");
        }
    }
    assert!(ids.len() >= 50, "expected at least 50 patterns, found {}", ids.len());
    println!("shipped catalog: {} patterns", ids.len());
}

#[test]
fn generated_levels_validate_and_reproduce() {
    let mut failures = 0usize;
    let mut runs = 0usize;
    let mut geometries: BTreeSet<Vec<PlatformFingerprint>> = BTreeSet::new();

    for &level in &GEN_LEVELS {
        for seed in 0..GEN_SEEDS {
            let generated = generate_for_with_spacing(seed, level, 1, 1.0);
            if validate(&generated.data).is_err() {
                failures += 1;
            }
            geometries.insert(signature(&generated.data));
            runs += 1;
        }
    }

    let mut repro_checked = 0usize;
    for seed in (0..REPRO_SEEDS).map(|i| i * 7) {
        let first = generate_for_with_spacing(seed, 3, 2, 1.0);
        let second = generate_for_with_spacing(seed, 3, 2, 1.0);
        assert_eq!(
            signature(&first.data),
            signature(&second.data),
            "seed {seed} level 3 is not reproducible"
        );
        repro_checked += 1;
    }

    println!(
        "generator layer: {runs} levels, {failures} validation failures, \
         {} distinct geometries, {repro_checked} reproducible pairs",
        geometries.len()
    );

    assert_eq!(failures, 0, "{failures} generated levels failed validate");
    // The guaranteed-valid fallback layout is shared by every seed that reaches it,
    // so allow a 1% collision budget rather than demanding 100% distinct geometry.
    assert!(
        geometries.len() * 100 >= runs * 99,
        "only {} of {runs} generated levels are geometrically distinct",
        geometries.len()
    );
}
