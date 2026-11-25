"""Export the M0 round-trip fixture from Blender in background mode."""

from pathlib import Path
import sys

import bpy

FPS = 24
FIRST_FRAME = 1
LAST_FRAME = FIRST_FRAME + FPS


def _output_path():
    args = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    if len(args) != 1:
        raise RuntimeError("Usage: --python export_fixture.py -- /absolute/path/fixture.glb")
    path = Path(args[0])
    if not path.is_absolute() or path.suffix != ".glb":
        raise RuntimeError(f"Expected an absolute .glb output path, got {args[0]!r}")
    return path


def _create_rig(scene):
    armature = bpy.data.armatures.new("FixtureRig")
    rig = bpy.data.objects.new("FixtureRig", armature)
    scene.collection.objects.link(rig)
    bpy.context.view_layer.objects.active = rig
    rig.select_set(True)

    bpy.ops.object.mode_set(mode="EDIT")
    root = armature.edit_bones.new("Root")
    root.head = (0, 0, 0)
    root.tail = (0, 0, 1)
    root.roll = 0.0
    hinge = armature.edit_bones.new("Hinge")
    hinge.head = (0, 0, 1)
    hinge.tail = (0, 0, 2)
    hinge.roll = 0.0
    hinge.parent = root
    hinge.use_connect = False
    bpy.ops.object.mode_set(mode="OBJECT")
    return rig


def _create_mesh(scene, rig):
    # The off-center tip marks authored forward (+Y). The widest ring sets the X/Y bounds.
    outline = ((-0.25, -0.5), (0.75, -0.5), (0.75, 0.25), (0.35, 1.0), (-0.25, 0.375))
    rings = ((0.0, 0.75), (0.75, 0.875), (1.25, 1.0), (2.0, 0.625))
    vertices = [(x * scale, y * scale, z) for z, scale in rings for x, y in outline]
    n = len(outline)
    faces = [tuple(reversed(range(n)))]
    for ring in range(len(rings) - 1):
        lower, upper = ring * n, (ring + 1) * n
        for i in range(n):
            j = (i + 1) % n
            faces.append((lower + i, lower + j, upper + j, upper + i))
    faces.append(tuple(range(len(vertices) - n, len(vertices))))

    mesh = bpy.data.meshes.new("FixtureMesh")
    mesh.from_pydata(vertices, [], faces)
    mesh.update()
    obj = bpy.data.objects.new("FixtureMesh", mesh)
    scene.collection.objects.link(obj)
    obj.parent = rig
    obj.vertex_groups.new(name="Root").add(list(range(2 * n)), 1.0, "REPLACE")
    obj.vertex_groups.new(name="Hinge").add(list(range(2 * n, len(vertices))), 1.0, "REPLACE")
    skin = obj.modifiers.new("FixtureSkin", "ARMATURE")
    skin.object = rig


def _create_socket(scene, rig):
    socket = bpy.data.objects.new("Socket", None)
    scene.collection.objects.link(socket)
    socket.parent = rig
    socket.parent_type = "BONE"
    socket.parent_bone = "Hinge"
    # Cancel the rest parent matrix (including Blender's bone-tail offset), not its motion.
    bpy.context.view_layer.update()
    socket.matrix_parent_inverse = socket.matrix_world.inverted()
    socket.location = (0.35, 0.4, 1.45)


def _create_action(rig):
    action = bpy.data.actions.new("probe_pose")
    slot = action.slots.new(rig.id_type, rig.name)
    layer = action.layers.new("Pose")
    strip = layer.strips.new(type="KEYFRAME")
    curves = strip.channelbag(slot, ensure=True).fcurves
    hinge = rig.pose.bones["Hinge"]
    curve = curves.new(hinge.path_from_id("location"), index=0)
    curve.keyframe_points.insert(FIRST_FRAME, 0.0).interpolation = "LINEAR"
    curve.keyframe_points.insert(LAST_FRAME, 0.6).interpolation = "LINEAR"

    animation = rig.animation_data_create()
    animation.action = action
    animation.action_slot = slot


def _export(path):
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        result = bpy.ops.export_scene.gltf(
            filepath=str(path),
            export_format="GLB",
            export_yup=True,
            export_skins=True,
            export_animations=True,
            export_animation_mode="ACTIONS",
            export_anim_single_armature=False,
            export_frame_range=True,
            export_current_frame=True,
            # Use the sampled exporter to shift authored frames 1..25 to seconds 0..1.
            export_force_sampling=True,
            export_anim_slide_to_zero=True,
            export_sampling_interpolation_fallback="LINEAR",
            export_optimize_animation_size=True,
            export_optimize_animation_keep_anim_armature=False,
        )
    except Exception as exc:
        raise RuntimeError(f"Could not export fixture GLB to {path}: {exc}") from exc

    if result != {"FINISHED"}:
        raise RuntimeError(f"Fixture GLB export to {path} returned {result}")
    if not path.is_file() or path.stat().st_size == 0:
        raise RuntimeError(f"Fixture export didn't write a non-empty GLB to {path}")


def main():
    path = _output_path()
    scene = bpy.context.scene
    for obj in tuple(scene.objects):
        bpy.data.objects.remove(obj, do_unlink=True)
    scene.unit_settings.system = "METRIC"
    scene.unit_settings.scale_length = 1.0
    scene.render.fps = FPS
    scene.render.fps_base = 1.0
    scene.frame_start = FIRST_FRAME
    scene.frame_end = LAST_FRAME

    rig = _create_rig(scene)
    _create_mesh(scene, rig)
    _create_socket(scene, rig)
    _create_action(rig)
    scene.frame_set(FIRST_FRAME)
    _export(path)


if __name__ == "__main__":
    main()
