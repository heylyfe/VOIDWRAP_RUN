"""Warden -- a tall grave-post with a ward halo held high on a spike.

Silhouette brief (T-160): Warden and Blight used to read as the same shape --
a wide flat disc under a vertical mass (Warden: torus ring at the base; Blight:
a skirt cylinder). Fixed-frame IoU 0.660, the roster's worst pair, against a
roster p90 of 0.410. Warden becomes tall and narrow with a distinctive top:
the ring is raised into a halo threaded on a spear, and the base foot shrinks
to a plinth. Nothing about the enemy's behaviour changes.

Animation stays in the client: `warden_halo` breathes and flares on telegraph,
`warden_finial` is the ward light, `warden_eye` is the cold sentry slit.
"""
import sys, os
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "lib"))
import bpy
import math
from mathutils import Matrix, Vector
from vw import (start, material, srgb, lathe, torus, mesh_object,
                center_origin, shade_smooth, finish)

ctx = start("warden")

POST = material("warden_post", srgb("#1c1a17"), metallic=0.45, roughness=0.62)
FINIAL = material("warden_finial", srgb("#f4d35e"), emission=srgb("#f4d35e"),
                  emission_strength=2.2, metallic=0.6, roughness=0.35)
HALO = material("warden_halo", srgb("#3a2f10"), emission=srgb("#f4d35e"),
                emission_strength=1.15, metallic=0.7, roughness=0.35)
EYE = material("warden_eye", srgb("#2a2410"), emission=srgb("#fdf3b8"),
               emission_strength=1.1)

# One revolved profile, plinth to spike. The plinth is deliberately narrow --
# the base disc is the whole thing this ticket exists to remove.
profile = [
    (0.34, -1.12), (0.345, -1.02), (0.30, -0.98), (0.245, -0.90),
    (0.215, -0.45), (0.195, 0.05), (0.185, 0.45),
    (0.20, 0.55), (0.30, 0.63), (0.30, 0.71), (0.115, 0.78),
    (0.085, 0.82), (0.078, 1.24),
]
post = lathe("warden_post", profile, segments=36, mat=POST, close_bottom=True)
shade_smooth(post)

finial = lathe("warden_finial", [(0.078, 1.24), (0.055, 1.33), (0.0, 1.45)],
               segments=24, mat=FINIAL)
shade_smooth(finial)

# The ward halo -- high on the spike, flat, the widest part of the silhouette:
# the ward that contains even the thin towers beats containing nothing.
halo = torus("warden_halo", 0.55, 0.085, 1.02, mat=HALO, major_segs=36, tube_segs=10)
shade_smooth(halo)

# The cold sentry slit, proud of the shaft face.
eye = mesh_object(
    "warden_eye",
    [(-0.15, -0.178, 0.153), (0.15, -0.178, 0.153), (0.15, -0.212, 0.153), (-0.15, -0.212, 0.153),
     (-0.15, -0.178, 0.208), (0.15, -0.178, 0.208), (0.15, -0.212, 0.208), (-0.15, -0.212, 0.208)],
    # Wound CCW seen from outside (right-hand rule), or three.js culls them.
    [(0, 1, 5, 4), (2, 3, 7, 6), (1, 5, 6, 2), (4, 0, 3, 7), (4, 7, 6, 5), (0, 1, 2, 3)],
    EYE,
)

# The whole post leans 12 degrees off vertical, pivoting at the plinth -- a
# grave-post bent by weather, bound by ward light. A rigid column covers the
# same vertical pixels every row (which is why the tall-thin neighbours kept
# beating this shape); a leaning one does not.
LEAN = math.radians(12.0)
BASE = Vector((0.0, 0.0, -1.05))
M = Matrix.Translation(BASE) @ Matrix.Rotation(LEAN, 4, 'Y') @ Matrix.Translation(-BASE)
for ob in list(bpy.context.scene.objects):
    if ob.type == 'MESH':
        ob.data.transform(M)

center_origin()
finish(ctx)
