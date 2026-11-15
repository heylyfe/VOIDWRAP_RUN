"""Coil -- a tall thin wound spring on a wide flat base.

Silhouette brief (T-157): the old Coil was a rounded column that read almost
identically to Peal (fixed-frame IoU 0.780). This version is deliberately the
opposite outline -- a 1.20-wide flat disc under a 0.56-wide column 1.70 tall,
i.e. a "T" against Peal's squat trapezoid.

Animation stays in the client: `coil_ring_0` .. `coil_ring_4` slide along Y to
compress the spring, `coil_core` is the spindle, `coil_base` and `coil_cap`
are the anchors.
"""
import sys, os
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "lib"))
from vw import (start, material, srgb, lathe, torus, sphere,
                set_origin, center_origin, shade_smooth, finish)

ctx = start("coil")

STEEL = material("coil_steel", srgb("#8a8f9a"), metallic=0.6, roughness=0.4)
DARK = material("coil_dark", srgb("#22242a"), metallic=0.7, roughness=0.5)
ACCENT = material("coil_accent", srgb("#e07a5f"), emission=srgb("#e07a5f"), emission_strength=3.0)
CORE = material("coil_core", srgb("#ffb59a"), emission=srgb("#e07a5f"), emission_strength=5.0)

base = lathe("coil_base", [(0.60, 0.00), (0.60, 0.08), (0.50, 0.12), (0.02, 0.12)],
             segments=28, mat=DARK, close_top=True, close_bottom=True)
shade_smooth(base)

spindle = lathe("coil_core", [(0.05, 0.10), (0.05, 1.58)],
                segments=12, mat=CORE, close_top=True, close_bottom=True)
shade_smooth(spindle)

# Five rings the client can slide independently -- the compression animation.
for i, z in enumerate((0.30, 0.55, 0.80, 1.05, 1.30)):
    ring = torus("coil_ring_%d" % i, 0.28, 0.055, z, mat=STEEL, major_segs=22, tube_segs=8)
    # Origin at the ring's own centre so the client can slide it along Y.
    set_origin(ring, (0.0, 0.0, z))
    shade_smooth(ring)

cap = lathe("coil_cap", [(0.16, 1.55), (0.16, 1.66), (0.10, 1.70)],
            segments=20, mat=ACCENT, close_top=True, close_bottom=True)
shade_smooth(cap)

center_origin()
finish(ctx)
