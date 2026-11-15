"""Shared helpers for the repo's headless Blender scripts (Blender 5.3).

A script under `tools/blender/scripts/` does:

    import sys, os
    sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "lib"))
    from vw import start, material, export_glb, render_views, finish

    ctx = start("peal")
    ... build geometry with the bpy data API / bmesh ...
    finish(ctx)          # exports web/public/models/peal.glb and renders previews

Blender 5.3 notes that cost time to rediscover:
  * do NOT set `material.use_nodes` -- new materials already have a node tree
  * DO set `world.use_nodes = True` -- worlds still need it
  * the render engine id is `BLENDER_EEVEE`
  * set `view_settings.view_transform = 'Standard'`; the AgX default washes colours out
  * prefer the data API / `bmesh` over `bpy.ops` -- many ops need a UI context headless
  * export with `export_apply=True` so modifiers bake
  * 1 unit = 1 metre, Z-up in Blender; the glTF exporter converts to Y-up
"""
import bpy
import math
import os
import sys
from mathutils import Vector


def start(name):
    """Empty scene; returns the context dict every other helper takes."""
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    repo = argv[0] if argv else os.getcwd()
    bpy.ops.wm.read_factory_settings(use_empty=True)
    out = os.path.join(repo, "web", "public", "models")
    renders = os.path.join(repo, "tools", "blender", "renders")
    os.makedirs(out, exist_ok=True)
    os.makedirs(renders, exist_ok=True)
    return {"name": name, "repo": repo, "out": out, "renders": renders, "extra": argv[1:]}


def material(name, color=(0.5, 0.5, 0.5, 1), emission=None, emission_strength=1.0,
             metallic=0.0, roughness=0.5):
    """Principled material. `color` and `emission` are linear RGBA tuples."""
    m = bpy.data.materials.new(name)
    b = m.node_tree.nodes["Principled BSDF"]
    b.inputs["Base Color"].default_value = color
    b.inputs["Metallic"].default_value = metallic
    b.inputs["Roughness"].default_value = roughness
    if emission is not None:
        b.inputs["Emission Color"].default_value = emission
        b.inputs["Emission Strength"].default_value = emission_strength
    return m


def srgb(hex_str):
    """`#rrggbb` -> linear RGBA, so a colour picked from design/palette.json lands right."""
    h = hex_str.lstrip("#")
    out = []
    for i in (0, 2, 4):
        c = int(h[i:i + 2], 16) / 255.0
        out.append(c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4)
    return (out[0], out[1], out[2], 1.0)


def mesh_object(name, verts, faces, mat=None, parent=None):
    """Build a mesh from raw vertex/face lists. The node name is what the client addresses."""
    me = bpy.data.meshes.new(name)
    me.from_pydata(verts, [], faces)
    me.validate()
    me.update()
    ob = bpy.data.objects.new(name, me)
    if mat is not None:
        ob.data.materials.append(mat)
    bpy.context.scene.collection.objects.link(ob)
    if parent is not None:
        ob.parent = parent
    return ob


def empty(name, parent=None, location=(0, 0, 0)):
    """An empty node. Use these as animation pivots -- the client finds them by name."""
    ob = bpy.data.objects.new(name, None)
    ob.empty_display_size = 0.05
    ob.location = location
    bpy.context.scene.collection.objects.link(ob)
    if parent is not None:
        ob.parent = parent
    return ob


def lathe(name, profile, segments=24, mat=None, parent=None, close_top=False, close_bottom=False):
    """Revolve a 2D profile [(radius, z), ...] around Z.

    The workhorse for bells, caps and discs: a profile is far easier to reason about than
    a vertex list, and the result is a clean quad grid.
    """
    verts = []
    faces = []
    rings = []
    for (r, z) in profile:
        ring = []
        for s in range(segments):
            a = 2.0 * math.pi * s / segments
            ring.append(len(verts))
            verts.append((r * math.cos(a), r * math.sin(a), z))
        rings.append(ring)
    for i in range(len(rings) - 1):
        a, b = rings[i], rings[i + 1]
        for s in range(segments):
            t = (s + 1) % segments
            faces.append((a[s], a[t], b[t], b[s]))
    if close_bottom and profile[0][0] > 1e-6:
        c = len(verts)
        verts.append((0.0, 0.0, profile[0][1]))
        for s in range(segments):
            t = (s + 1) % segments
            faces.append((rings[0][t], rings[0][s], c))
    if close_top and profile[-1][0] > 1e-6:
        c = len(verts)
        verts.append((0.0, 0.0, profile[-1][1]))
        for s in range(segments):
            t = (s + 1) % segments
            faces.append((rings[-1][s], rings[-1][t], c))
    return mesh_object(name, verts, faces, mat, parent)


def helix(name, turns, radius, tube, z0, z1, mat=None, parent=None, steps_per_turn=16):
    """A tube swept along a helix around Z. Used for the coil spring."""
    verts = []
    faces = []
    steps = max(8, int(turns * steps_per_turn))
    rings = []
    for i in range(steps + 1):
        u = i / steps
        a = 2.0 * math.pi * turns * u
        cz = z0 + (z1 - z0) * u
        cx, cy = radius * math.cos(a), radius * math.sin(a)
        nx, ny = math.cos(a), math.sin(a)
        ring = []
        for s in range(8):
            b = 2.0 * math.pi * s / 8
            ox = nx * math.cos(b) * tube
            oy = ny * math.cos(b) * tube
            oz = math.sin(b) * tube
            ring.append(len(verts))
            verts.append((cx + ox, cy + oy, cz + oz))
        rings.append(ring)
    for i in range(len(rings) - 1):
        a, b = rings[i], rings[i + 1]
        for s in range(8):
            t = (s + 1) % 8
            faces.append((a[s], a[t], b[t], b[s]))
    return mesh_object(name, verts, faces, mat, parent)


def shade_smooth(ob):
    """Smooth shading via the data API (no UI context needed headless)."""
    for p in ob.data.polygons:
        p.use_smooth = True


def export_glb(ctx):
    path = os.path.join(ctx["out"], ctx["name"] + ".glb")
    bpy.ops.export_scene.gltf(
        filepath=path,
        export_format='GLB',
        export_apply=True,
        export_yup=True,
        use_selection=False,
    )
    print("EXPORTED", path)
    return path


def _bounds():
    pts = []
    for o in bpy.context.scene.objects:
        if o.type == 'MESH':
            pts += [o.matrix_world @ Vector(c) for c in o.bound_box]
    if not pts:
        return Vector((0, 0, 0)), 1.0
    lo = Vector((min(p.x for p in pts), min(p.y for p in pts), min(p.z for p in pts)))
    hi = Vector((max(p.x for p in pts), max(p.y for p in pts), max(p.z for p in pts)))
    return (lo + hi) / 2, max((hi - lo).length, 0.001)


def center_origin():
    """Shift every root object so the scene's bbox centre sits at the world origin.

    The client places an enemy at its hit-sphere centre, so a model authored with
    its feet on z=0 would sink half a body into the floor.
    """
    center, _ = _bounds()
    for o in bpy.context.scene.objects:
        if o.parent is None:
            o.location = (o.location.x - center.x, o.location.y - center.y, o.location.z - center.z)


def render_views(ctx, size=768, views=("front", "side", "three_quarter")):
    """Dark-void studio previews, one PNG per angle, into tools/blender/renders/."""
    scene = bpy.context.scene
    center, diag = _bounds()
    dist = diag * 1.6
    scene.render.engine = 'BLENDER_EEVEE'
    scene.render.resolution_x = size
    scene.render.resolution_y = size
    scene.render.film_transparent = False
    world = bpy.data.worlds.new("Void")
    world.use_nodes = True
    bg = world.node_tree.nodes["Background"]
    bg.inputs["Color"].default_value = (0.01, 0.01, 0.025, 1)
    bg.inputs["Strength"].default_value = 1.0
    scene.world = world
    scene.view_settings.view_transform = 'Standard'

    cam = bpy.data.objects.new("Cam", bpy.data.cameras.new("Cam"))
    scene.collection.objects.link(cam)
    scene.camera = cam
    for nm, loc, en in (("Key", (1, -1, 1.2), 4.0), ("Fill", (-1, -0.5, 0.4), 1.5), ("Rim", (0, 1, 0.8), 3.0)):
        ld = bpy.data.lights.new(nm, 'SUN')
        ld.energy = en
        lo = bpy.data.objects.new(nm, ld)
        lo.rotation_euler = Vector(loc).to_track_quat('-Z', 'Y').to_euler()
        scene.collection.objects.link(lo)

    dirs = {
        "front": Vector((0, -1, 0.15)),
        "side": Vector((1, 0, 0.15)),
        "three_quarter": Vector((0.8, -0.8, 0.5)),
    }
    outs = []
    for v in views:
        d = dirs[v].normalized()
        cam.location = center + d * dist
        cam.rotation_euler = (center - cam.location).to_track_quat('-Z', 'Y').to_euler()
        p = os.path.join(ctx["renders"], f"{ctx['name']}_{v}.png")
        scene.render.filepath = p
        bpy.ops.render.render(write_still=True)
        outs.append(p)
        print("RENDERED", p)
    return outs


def finish(ctx, render=True):
    export_glb(ctx)
    if render and "--no-render" not in ctx["extra"]:
        render_views(ctx)


def set_origin(ob, pivot):
    """Move an object's origin to `pivot` (world space), keeping the mesh in place.

    Animation pivots matter: a bell shell has to hinge at the crown, not at the
    world origin, or swinging it open translates it across the floor.
    """
    px, py, pz = pivot
    lx, ly, lz = ob.location
    dx, dy, dz = px - lx, py - ly, pz - lz
    for v in ob.data.vertices:
        v.co.x -= dx
        v.co.y -= dy
        v.co.z -= dz
    ob.location = (px, py, pz)
    return ob


def revolve_solid(name, profile, a0, a1, segments=16, mat=None, parent=None):
    """Revolve a CLOSED (r, z) loop over an angular range, capping both ends.

    A closed profile gives a shell real thickness, so the inside of a bell still
    reads when its halves swing open.
    """
    n = len(profile)
    verts = []
    faces = []
    rings = []
    for s in range(segments + 1):
        a = a0 + (a1 - a0) * s / segments
        ca, sa = math.cos(a), math.sin(a)
        ring = []
        for (r, z) in profile:
            ring.append(len(verts))
            verts.append((r * ca, r * sa, z))
        rings.append(ring)
    for s in range(segments):
        lo, hi = rings[s], rings[s + 1]
        for i in range(n):
            j = (i + 1) % n
            faces.append((lo[i], lo[j], hi[j], hi[i]))
    for ring, flip in ((rings[0], True), (rings[-1], False)):
        cx = sum(verts[k][0] for k in ring) / n
        cy = sum(verts[k][1] for k in ring) / n
        cz = sum(verts[k][2] for k in ring) / n
        c = len(verts)
        verts.append((cx, cy, cz))
        for i in range(n):
            j = (i + 1) % n
            faces.append((ring[j], ring[i], c) if flip else (ring[i], ring[j], c))
    return mesh_object(name, verts, faces, mat, parent)


def torus(name, major, tube, z, mat=None, parent=None, major_segs=24, tube_segs=8):
    """A ring lying flat in XY at height `z`."""
    verts = []
    faces = []
    rings = []
    for i in range(major_segs):
        a = 2.0 * math.pi * i / major_segs
        ca, sa = math.cos(a), math.sin(a)
        ring = []
        for j in range(tube_segs):
            b = 2.0 * math.pi * j / tube_segs
            rr = major + tube * math.cos(b)
            ring.append(len(verts))
            verts.append((rr * ca, rr * sa, z + tube * math.sin(b)))
        rings.append(ring)
    for i in range(major_segs):
        lo, hi = rings[i], rings[(i + 1) % major_segs]
        for j in range(tube_segs):
            k = (j + 1) % tube_segs
            faces.append((lo[j], lo[k], hi[k], hi[j]))
    return mesh_object(name, verts, faces, mat, parent)


def sphere(name, radius, center=(0, 0, 0), mat=None, parent=None, segs=16, rings=10):
    """UV sphere. `center` is in world space; the object origin stays at the centre."""
    cx, cy, cz = center
    verts = []
    faces = []
    rows = []
    for i in range(rings + 1):
        phi = math.pi * i / rings
        z = math.cos(phi)
        r = math.sin(phi)
        row = []
        for j in range(segs):
            a = 2.0 * math.pi * j / segs
            row.append(len(verts))
            verts.append((cx + radius * r * math.cos(a), cy + radius * r * math.sin(a), cz + radius * z))
        rows.append(row)
    for i in range(rings):
        lo, hi = rows[i], rows[i + 1]
        for j in range(segs):
            k = (j + 1) % segs
            faces.append((lo[j], lo[k], hi[k], hi[j]))
    ob = mesh_object(name, verts, faces, mat, parent)
    set_origin(ob, (cx, cy, cz))
    return ob
