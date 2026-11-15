"""Peal -- a wide squat bell with its clapper hanging below the lip.

Silhouette brief (T-157): Peal and Coil used to read as the same tall rounded
column -- fixed-frame IoU 0.780 against a roster p90 of 0.432. Peal becomes a
squat flared trapezoid with a bottom-centre tail; Coil becomes a tall thin
column on a wide flat base. Nothing about either enemy's behaviour changes.

Animation stays in the client. The shells hinge on `peal_shell_l` /
`peal_shell_r` (origins at the crown, so a plain Y rotation opens them), the
clapper swings on `peal_clapper`, and `peal_glow` is the inner light.
"""
import sys, os
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "lib"))
from vw import (start, material, srgb, lathe, torus, sphere, revolve_solid,
                set_origin, center_origin, shade_smooth, finish)

ctx = start("peal")

BRONZE = material("peal_bronze", srgb("#a8845a"), metallic=0.55, roughness=0.45)
DARK = material("peal_dark", srgb("#7a6242"), metallic=0.45, roughness=0.55)
ACCENT = material("peal_accent", srgb("#9d0208"), emission=srgb("#9d0208"), emission_strength=2.5)
GLOW = material("peal_glow", srgb("#ffd9a0"), emission=srgb("#ffb347"), emission_strength=7.0)

# Closed (radius, z) loop: down the outside, back up the inside. The bell is
# 1.70 wide at the lip and 0.48 at the crown -- a trapezoid, not a column.
SHELL = [
    (0.85, 0.00), (0.78, 0.18), (0.62, 0.52), (0.40, 0.82), (0.24, 0.98),
    (0.20, 0.98), (0.36, 0.82), (0.58, 0.52), (0.74, 0.18), (0.81, 0.00),
]
CROWN_Z = 0.98

for name, a0, a1 in (("peal_shell_r", -1.5707963, 1.5707963),
                     ("peal_shell_l", 1.5707963, 4.7123890)):
    ob = revolve_solid(name, SHELL, a0, a1, segments=14, mat=BRONZE)
    set_origin(ob, (0.0, 0.0, CROWN_Z))
    shade_smooth(ob)

lip = torus("peal_lip", 0.84, 0.055, 0.02, mat=DARK, major_segs=28, tube_segs=8)
shade_smooth(lip)

crown = lathe("peal_crown", [(0.24, 0.98), (0.17, 1.06), (0.10, 1.10)],
              segments=20, mat=ACCENT, close_top=True, close_bottom=True)
shade_smooth(crown)

glow = sphere("peal_glow", 0.22, center=(0.0, 0.0, 0.55), mat=GLOW, segs=16, rings=10)

# The tail: a ball hanging clear of the lip, so the outline has a bottom-centre
# stem that no other enemy has. Origin at the hinge, not the ball.
clapper = sphere("peal_clapper", 0.16, center=(0.0, 0.0, -0.36), mat=DARK, segs=16, rings=10)
set_origin(clapper, (0.0, 0.0, 0.02))
shade_smooth(clapper)

center_origin()
finish(ctx)
