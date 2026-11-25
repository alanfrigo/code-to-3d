extends SceneTree

const FIXTURE_PATH := "res://fixture.glb"
const SAMPLE_TIMES := [0.0, 0.5, 1.0]


func _initialize() -> void:
	# SceneTree enters its root after _initialize returns.
	_run.call_deferred()


func _run() -> void:
	# quit takes effect after this callback; an aborted script must not exit zero.
	quit(1)
	var args := OS.get_cmdline_user_args()
	if args.size() != 1 or not args[0].is_absolute_path():
		printerr("probe: expected -- /absolute/path/to/observed.json")
		return

	var packed := ResourceLoader.load(FIXTURE_PATH, "PackedScene") as PackedScene
	if packed == null:
		printerr("probe: cannot load editor-imported PackedScene %s" % FIXTURE_PATH)
		return
	var scene := packed.instantiate()
	if scene == null:
		printerr("probe: cannot instantiate %s" % FIXTURE_PATH)
		return
	root.add_child(scene)

	var observed := _observe()
	if not observed.is_empty() and _publish(args[0], observed):
		quit(0)


func _observe() -> Dictionary:
	var meshes := root.find_children("*", "MeshInstance3D", true, false)
	var skeletons := root.find_children("*", "Skeleton3D", true, false)
	var sockets := root.find_children("Socket", "Node3D", true, false)
	var players := root.find_children("*", "AnimationPlayer", true, false)
	if meshes.size() != 1 or meshes[0].name != &"FixtureMesh":
		return _invalid("expected one MeshInstance3D named FixtureMesh, found %s" % [meshes])
	if skeletons.size() != 1 or sockets.size() != 1 or players.size() != 1:
		return _invalid("expected one Skeleton3D, Socket and AnimationPlayer; found %d, %d, %d" % [
			skeletons.size(), sockets.size(), players.size(),
		])

	var mesh_instance := meshes[0] as MeshInstance3D
	var skeleton := skeletons[0] as Skeleton3D
	var socket := sockets[0] as Node3D
	var player := players[0] as AnimationPlayer
	if mesh_instance.mesh == null or mesh_instance.mesh.get_surface_count() == 0:
		return _invalid("FixtureMesh has no mesh surfaces")
	var skin := mesh_instance.get_skin()
	var has_skin := skin != null and skin.get_bind_count() > 0 and (
		mesh_instance.get_skin_reference() != null
		and mesh_instance.get_node_or_null(mesh_instance.get_skeleton_path()) == skeleton
	)
	if not has_skin:
		return _invalid("FixtureMesh has no imported skin bound to the Skeleton3D")
	var bounds := mesh_instance.global_transform * mesh_instance.get_aabb()
	if not bounds.is_finite() or not bounds.end.is_finite():
		return _invalid("FixtureMesh has non-finite world bounds")

	var root_bone := skeleton.find_bone("Root")
	var hinge_bone := skeleton.find_bone("Hinge")
	var bone_count := skeleton.get_bone_count()
	if root_bone < 0 or hinge_bone < 0:
		return _invalid("Skeleton3D must contain Root and Hinge")
	var bones: Array[Dictionary] = []
	for index in range(bone_count):
		var parent := skeleton.get_bone_parent(index)
		bones.append({
			"name": skeleton.get_bone_name(index),
			"parent": null if parent == -1 else skeleton.get_bone_name(parent),
		})

	var ancestor := socket.get_parent()
	while ancestor != null and not (ancestor is BoneAttachment3D):
		ancestor = ancestor.get_parent()
	var attachment := ancestor as BoneAttachment3D
	if attachment == null or attachment.get_skeleton() != skeleton or attachment.override_pose:
		return _invalid("Socket must follow a BoneAttachment3D on the imported Skeleton3D")
	var socket_bone := attachment.get_bone_idx()
	if socket_bone < 0 or socket_bone >= bone_count:
		return _invalid("Socket attachment has no valid bone index")

	var clips := player.get_animation_list()
	var reset_index := clips.find("RESET")
	if reset_index != -1:
		clips.remove_at(reset_index)
	if clips.size() != 1 or clips[0] != "probe_pose" or not player.active:
		return _invalid("expected an active AnimationPlayer with only probe_pose (excluding RESET), found %s" % [clips])
	var clip_name := clips[0]
	var duration := player.get_animation(clip_name).length
	if not is_finite(duration) or duration <= 0.0:
		return _invalid("probe_pose has invalid duration: %s" % duration)

	# Seek on a stopped player to sample the exact endpoint without ticking playback.
	player.callback_mode_process = AnimationMixer.ANIMATION_CALLBACK_MODE_PROCESS_MANUAL
	player.stop(true)
	player.assigned_animation = clip_name
	var samples: Array[Dictionary] = []
	for time: float in SAMPLE_TIMES:
		player.seek(time, true, true)
		# Pose queries resolve dirty bones; refresh the attachment before reading Socket.
		var skeleton_world := skeleton.global_transform
		var root_m := skeleton_world * skeleton.get_bone_global_pose(root_bone).origin
		var hinge_m := skeleton_world * skeleton.get_bone_global_pose(hinge_bone).origin
		attachment.on_skeleton_update()
		var socket_m := socket.global_position
		if not root_m.is_finite() or not hinge_m.is_finite() or not socket_m.is_finite():
			return _invalid("probe_pose produced a non-finite position at %s seconds" % time)
		samples.append({
			"time_seconds": time,
			"root_m": _xyz(root_m),
			"hinge_m": _xyz(hinge_m),
			"socket_m": _xyz(socket_m),
		})

	return {
		"schema_version": 1,
		"mesh": {
			"name": String(mesh_instance.name),
			"has_skin": has_skin,
			"aabb_min_m": _xyz(bounds.position),
			"aabb_max_m": _xyz(bounds.end),
		},
		"bones": bones,
		"socket_bone": skeleton.get_bone_name(socket_bone),
		"clip": {"name": clip_name, "duration_seconds": duration, "samples": samples},
	}


func _xyz(vector: Vector3) -> Array:
	return [vector.x, vector.y, vector.z]


func _invalid(message: String) -> Dictionary:
	printerr("probe: %s" % message)
	return {}


func _publish(path: String, observed: Dictionary) -> bool:
	var staging_path := path + ".tmp"
	var file := FileAccess.open(staging_path, FileAccess.WRITE)
	if file == null:
		printerr("probe: cannot open %s: %s" % [staging_path, error_string(FileAccess.get_open_error())])
		return false
	var bytes := (JSON.stringify(observed, "", true, true) + "\n").to_utf8_buffer()
	var written := file.store_buffer(bytes)
	file.close()
	if not written or FileAccess.get_size(staging_path) != bytes.size():
		DirAccess.remove_absolute(staging_path)
		printerr("probe: incomplete write to %s" % staging_path)
		return false
	var result := DirAccess.rename_absolute(staging_path, path)
	if result != OK:
		DirAccess.remove_absolute(staging_path)
		printerr("probe: cannot publish %s: %s" % [path, error_string(result)])
		return false
	return true
