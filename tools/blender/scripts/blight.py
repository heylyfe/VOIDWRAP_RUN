"""Blight -- a low, wide, asymmetric spreading mass.

Silhouette brief (T-160): Warden and Blight used to read as the same shape --
a wide flat disc under a vertical mass (Blight: a 1.02-radius skirt cylinder;
Warden: a 0.82-radius torus ring at the base). Fixed-frame IoU 0.660, the
roster's worst pair, against a roster p90 of 0.410. Blight becomes low, wide
and asymmetric: an irregular off-centre spill with a lopsided mound on top --
no radial symmetry, no disc. Nothing about the enemy's behaviour changes.

Animation stays in the client: `blight_spill` creeps and surges outward,
`blight_mound` heaves, `blight_sore_0..3` and `blight_core` pulse.
"""
import sys, os, math
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "lib"))
from vw import (start, material, srgb, sphere, mesh_object, empty,
                center_origin, shade_smooth, finish)

ctx = start("blight")

SPILL = material("blight_spill", srgb("#232b12"), metallic=0.0, roughness=0.85)
MOUND_A = material("blight_mound_a", srgb("#222a16"), metallic=0.05, roughness=0.16)
MOUND_B = material("blight_mound_b", srgb("#1f2614"), metallic=0.05, roughness=0.14)
MOUND_C = material("blight_mound_c", srgb("#252d18"), metallic=0.05, roughness=0.14)
MOUND_D = material("blight_mound_d", srgb("#1e2413"), metallic=0.05, roughness=0.16)
MOUND_E = material("blight_mound_e", srgb("#28301a"), metallic=0.05, roughness=0.15)
SORE = material("blight_sore", srgb("#39441c"), emission=srgb("#d9e89b"), emission_strength=0.75)
CORE = material("blight_core", srgb("#a3c34a"), emission=srgb("#a3c34a"), emission_strength=2.2)


def ellipsoid(name, radii, local_center, mat, parent, segs=12, rings=8):
    """An axis-aligned ellipsoid, baked into the verts (not a node scale).

    The client reads only each node's translation, so proportions that live in
    a node scale would be silently dropped. Vertices carry the shape instead.
    """
    rx, ry, rz = radii
    verts, faces, rows = [], [], []
    for i in range(rings + 1):
        phi = math.pi * i / rings
        z, r = math.cos(phi), math.sin(phi)
        row = []
        for j in range(segs):
            a = 2.0 * math.pi * j / segs
            row.append(len(verts))
            verts.append((rx * r * math.cos(a), ry * r * math.sin(a), rz * z))
        rows.append(row)
    for i in range(rings):
        lo, hi = rows[i], rows[i + 1]
        for j in range(segs):
            k = (j + 1) % segs
            faces.append((lo[j], lo[k], hi[k], hi[j]))
    ob = mesh_object(name, verts, faces, mat, parent=parent)
    ob.location = local_center
    shade_smooth(ob)
    return ob


# --- the spill: flattened lobes biased to the left, with a long thin tendril
# running right -- the infection crawling away. No radial symmetry anywhere.
spill = empty("blight_spill", location=(0.0, 0.0, 0.0))
SPILLS = [
    ("blight_spill_1", (0.50, 0.34, 0.125), (-0.50, 0.02, 0.115)),
    ("blight_spill_2", (0.30, 0.24, 0.10), (0.12, -0.12, 0.095)),
    ("blight_spill_3", (0.16, 0.12, 0.048), (0.62, 0.10, 0.055)),
    ("blight_spill_4", (0.13, 0.095, 0.038), (0.97, -0.05, 0.038)),
    ("blight_spill_5", (0.14, 0.105, 0.038), (-1.01, -0.08, 0.04)),
]
for name, radii, center in SPILLS:
    ellipsoid(name, radii, center, SPILL, spill, segs=9, rings=6)

# --- the mound: one big mass sitting left of centre with a single spire rising
# out of its top -- the "about to break" wave. One shoulder descending to the
# right in dribbles. Mass is pushed left, so the outline is different from
# every angle and from every other enemy's.
mound = empty("blight_mound", location=(0.0, 0.0, 0.45))
MOUND_PIVOT = (0.0, 0.0, 0.45)
MOUNDS = [
    ("blight_mound_main", (0.46, 0.36, 0.44), (-0.40, 0.04, 0.49), MOUND_A),
    ("blight_mound_r1", (0.24, 0.21, 0.23), (0.10, -0.04, 0.29), MOUND_B),
    ("blight_mound_r2", (0.15, 0.13, 0.115), (0.43, 0.08, 0.175), MOUND_C),
    ("blight_mound_l1", (0.235, 0.20, 0.18), (-0.73, 0.13, 0.235), MOUND_D),
    ("blight_mound_spire", (0.195, 0.175, 0.43), (-0.31, -0.02, 0.90), MOUND_E),
    ("blight_mound_bump", (0.135, 0.125, 0.115), (-0.04, 0.12, 0.585), MOUND_B),
]
for name, radii, center, mat in MOUNDS:
    local = (center[0] - MOUND_PIVOT[0], center[1] - MOUND_PIVOT[1], center[2] - MOUND_PIVOT[2])
    ellipsoid(name, radii, local, mat, mound, segs=11, rings=7)

# --- sores: embers under the skin, placed on the front faces, lopsided.
SORES = [
    ("blight_sore_0", 0.075, (-0.58, -0.30, 0.47)),
    ("blight_sore_1", 0.075, (0.10, -0.26, 0.30)),
    ("blight_sore_2", 0.065, (-0.22, -0.19, 0.78)),
    ("blight_sore_3", 0.060, (0.41, -0.055, 0.17)),
]
for name, radius, center in SORES:
    sphere(name, radius, center=center, mat=SORE, segs=8, rings=5)

# --- the core: a sickly ember buried in the mound, half-proud of the front.
sphere("blight_core", 0.14, center=(-0.36, -0.33, 0.45), mat=CORE, segs=9, rings=6)

center_origin()
finish(ctx)
