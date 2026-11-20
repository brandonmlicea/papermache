use serde_json::{json, Value};

pub fn tools() -> Value {
    let v3 = |d: &str| json!({"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3, "description": d});
    let object = json!({"type": "object", "additionalProperties": true, "description":
        "An object: {name?, shape, ...shape fields, position?:[x,y,z], rotation?:[deg x,y,z], scale?:[x,y,z], material?, tags?:[..], hidden?}. \
         Shapes: box {size:[x,y,z], radius? bevel}; cylinder {radius, radius_top? (cone/frustum), height} along Z; sphere {radius} (scale makes ellipsoids); \
         torus {major, minor} in XY; rod {from, to, radius, radius_top?}; tube {points:[[x,y,z]..], radius} (a wire); plane {size:[w,h]} facing +Z; \
         text {text, size (em), align: center|left|right} flat in XY, readable from +Z; image {file: .png, width (mm), depth (mm, default 1), key [r,g,b] background to drop (default white), detail (cells a pixel, default 4)}: a picture raised into a solid, every column in its own pixel colour; mesh {file: .obj|.stl|.glb|.gltf, node? (a glTF node and everything under it), skip? [node names], smooth? (degrees: corners closer than this share a normal), subdivide? (Loop levels)} or {vertices, faces}; \
         lathe {profile: [[r, z]..] turned around Z (start and end at r 0 to close it), segments? 64, ridges? count, ridge_depth?, ridge_band? [z0, z1], subdivide?, smooth? 40}: crowns, dials, knobs, lenses; \
         extrude {outline: [[x, y]..] or size: [w, h] with radius (corner), depth along Z, bevel? (edge round), segments? 4, wall? (hollow it into a frame that thick), subdivide?, smooth? 40}: buttons, bezels, frames; \
         backdrop {size: [width, depth, height], radius?}: a studio sweep, floor at z 0 curving up into a wall at +Y."});
    let target = |what: &str| json!({"name": {"type": "string", "description": format!("the object to {what}")}, "tag": {"type": "string", "description": format!("or every object with this tag")}});
    let mut update = target("change").as_object().unwrap().clone();
    update.insert("new_name".into(), json!({"type": "string"}));
    let camera = json!({"eye": v3("camera position"), "look": v3("point it looks at"), "up": v3("up hint, default [0,0,1]"),
        "projection": {"type": "string", "enum": ["perspective", "orthographic"]}, "lens": {"type": "number", "description": "focal length in mm, 36 mm sensor"},
        "fov": {"type": "number", "description": "degrees across the larger side, overrides lens"}, "width": {"type": "number", "description": "orthographic: mm across the larger side"},
        "size": {"type": "array", "items": {"type": "integer"}, "description": "[width, height] px"},
        "fstop": {"type": "number", "description": "depth of field: aperture as an f-stop (lens / fstop wide); none is a pinhole"},
        "focus": {"type": "number", "description": "focus distance in mm, default the look point"}, "focus_on": {"type": "string", "description": "an object to keep sharp"}});
    let num = |d: &str| json!({"type": "number", "description": d});
    let lamp = json!({"name": {"type": "string"}, "kind": {"type": "string", "enum": ["area", "spot", "point"]}, "position": v3("where it is"), "look": v3("where it points"),
        "color": v3("linear RGB"), "strength": num("1 lights like a sun of 1 at 100 mm, falling off with distance squared"), "size": {"type": "array", "items": {"type": "number"}, "description": "area: [w, h] mm"},
        "radius": num("point or spot: its size, for softer shadows"), "angle": num("spot: cone degrees"), "blend": num("spot: edge softness 0-1"),
        "visible": {"type": "boolean", "description": "area: seen by the camera and in reflections (default true)"}, "remove": {"type": "boolean"}});
    json!([
        {"name": "scene_info", "description": "The scene: object count by shape, bounds, materials, camera, lights, and each object's name, shape, material, tags and world bounds (first 300).",
         "inputSchema": {"type": "object", "properties": {"tag": {"type": "string", "description": "only objects with this tag"}}}},
        {"name": "scene_clear", "description": "Empties the scene: no objects, default materials, camera and lights.", "inputSchema": {"type": "object", "properties": {}}},
        {"name": "scene_load", "description": "Replaces the scene with a Paper Mache scene JSON file.", "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}},
        {"name": "scene_save", "description": "Writes the scene as JSON (render it later with `papermache render`).", "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}},
        {"name": "object_add", "description": "Adds one object ({object}) or many ({objects:[...]}). Returns their names.",
         "inputSchema": {"type": "object", "properties": {"object": object, "objects": {"type": "array", "items": object}}}},
        {"name": "object_update", "description": "Merges fields into an object (or every object with a tag): move, turn, scale, resize, recolour, hide, retag. new_name renames.",
         "inputSchema": {"type": "object", "additionalProperties": true, "properties": update}},
        {"name": "object_remove", "description": "Removes an object, or every object with a tag.", "inputSchema": {"type": "object", "properties": target("remove")}},
        {"name": "object_array", "description": "Repeats an object count times, each copy offset (and turned) a step further: rows of pins, teeth, ribs, rivets.",
         "inputSchema": {"type": "object", "properties": {"name": {"type": "string"}, "count": {"type": "integer"}, "offset": v3("step between copies, mm"), "rotate": v3("extra degrees per copy")}, "required": ["name", "count"]}},
        {"name": "material_set", "description": "Creates or changes a material: color (linear 0-1), metallic, roughness, emission, opacity.",
         "inputSchema": {"type": "object", "properties": {"name": {"type": "string"}, "color": v3("linear RGB 0-1"), "metallic": {"type": "number"}, "roughness": {"type": "number"},
            "emission": v3("light it gives off"), "opacity": {"type": "number"}, "coat": num("clear coat 0-1 over the base"), "coat_roughness": num("default 0.03"),
            "texture": {"type": "string", "description": "a .png on the object's UVs (multiplies color)"}, "repeat": {"type": "array", "items": {"type": "number"}, "description": "[u, v] tiling"},
            "pattern": {"type": "object", "description": "{kind: brushed|spun|noise|gradient|checker, scale mm, color (second colour), amount, axis, range [from, to] for gradient}, in the object's own space"},
            "screen": {"type": "object", "description": "{file: screen.png or frames/%04d.png, fps, start (s), first (number of the first frame, 1), count (loop length), strength}: a picture or video glowing on the UVs"}}, "required": ["name"]}},
        {"name": "camera_set", "description": "Sets camera fields.", "inputSchema": {"type": "object", "properties": camera}},
        {"name": "camera_frame", "description": "Aims the camera so every visible object (or those with a tag) fills the view, from a direction.",
         "inputSchema": {"type": "object", "properties": {"tag": {"type": "string"}, "direction": v3("from the target toward the eye, default [1,-1.2,0.9]"),
            "projection": {"type": "string", "enum": ["perspective", "orthographic"]}, "margin": {"type": "number", "description": "1.1 = 10% room"}}}},
        {"name": "lights_set", "description": "Sun {direction toward the sun, angle (deg, softness), strength}, sky {color, strength}, background [r,g,b] or \"transparent\".",
         "inputSchema": {"type": "object", "properties": {"sun": {"type": "object"}, "sky": {"type": "object"}, "background": {}}}},
        {"name": "render", "description": "Renders the scene and shows it. path saves the full-size PNG; the image shown is at most max_size px.",
         "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}, "size": {"type": "array", "items": {"type": "integer"}}, "samples": {"type": "integer", "description": "default 8"},
            "max_size": {"type": "integer", "description": "default 800"}, "time": num("seconds into the timeline")}}},
        {"name": "project", "description": "World points to pixel coordinates [x right, y down] at the camera's size.",
         "inputSchema": {"type": "object", "properties": {"points": {"type": "array", "items": {"type": "array", "items": {"type": "number"}}}}, "required": ["points"]}},
        {"name": "export", "description": "Writes the visible objects (or those with a tag) as .obj (with .mtl) or binary .stl, for printing or other tools. Text is skipped.",
         "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}, "tag": {"type": "string"}}, "required": ["path"]}},
        {"name": "lamp_set", "description": "Adds or changes a lamp by name (area softbox, spot or point) beside the sun and sky; remove: true deletes it. Set the sun's strength to 0 and the sky low for a studio.",
         "inputSchema": {"type": "object", "properties": lamp, "required": ["name"]}},
        {"name": "import_info", "description": "A .glb/.gltf's nodes (name, parent, mesh or group, bounds in Z-up mm) and material names.",
         "inputSchema": {"type": "object", "properties": {"file": {"type": "string"}}, "required": ["file"]}},
        {"name": "import", "description": "Adds a .glb/.gltf as mesh objects in its own materials: the whole file, one node, or with split every mesh node (under node) as its own object, ready to explode. Other fields (position, rotation, scale, tags, material, skip, smooth) go on every object.",
         "inputSchema": {"type": "object", "additionalProperties": true, "properties": {"file": {"type": "string"}, "node": {"type": "string"}, "split": {"type": "boolean"}, "name": {"type": "string"}}, "required": ["file"]}},
        {"name": "keyframe_set", "description": "Keys any scene field over time. path: objects.<name>.position|rotation|scale|hidden, camera.eye|look|lens|fov|fstop|focus, materials.<name>.color|emission|screen.strength, lights.lamps.<name>.strength, lights.sun.strength... One key {t, value, ease} or many {keys: [{t, v, ease}]}; ease: linear (default), in, out, inout, step or [x1, y1, x2, y2]; spline: true runs a smooth curve through the keys (camera moves).",
         "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}, "t": num("seconds"), "value": {}, "ease": {}, "keys": {"type": "array", "items": {"type": "object"}}, "spline": {"type": "boolean"}}, "required": ["path"]}},
        {"name": "timeline", "description": "Lists the tracks and the timeline's length; clear: a path, or all, removes tracks.",
         "inputSchema": {"type": "object", "properties": {"clear": {"type": "string"}}}},
        {"name": "explode", "description": "Keys an assembly apart and back: each part (names or tag) moves distance mm away from the assembly's centre (or by its own offsets {name: [x,y,z]}) between times[0] and times[1], holds until times[2] and returns by times[3].",
         "inputSchema": {"type": "object", "properties": {"names": {"type": "array", "items": {"type": "string"}}, "tag": {"type": "string"}, "distance": num("mm, default 30"), "offsets": {"type": "object"},
            "times": {"type": "array", "items": {"type": "number"}}, "ease": {}}, "required": ["times"]}},
        {"name": "film", "description": "Renders the timeline from `from` to `to` (default its end) at fps into dir/frame_00001.png..., then dir/film.mp4 with ffmpeg. Shows the middle frame.",
         "inputSchema": {"type": "object", "properties": {"dir": {"type": "string"}, "fps": num("default render.fps, 30"), "from": num("s"), "to": num("s"), "samples": {"type": "integer"},
            "scale": num("size factor, e.g. 0.25 for drafts"), "mp4": {"type": "boolean"}, "resume": {"type": "boolean", "description": "skip frames already written"}}, "required": ["dir"]}},
        {"name": "beats", "description": "A song's tempo (bpm), beats, downbeats and onsets in seconds (.wav, or .mp3/.flac/... through ffmpeg).",
         "inputSchema": {"type": "object", "properties": {"file": {"type": "string"}}, "required": ["file"]}},
        {"name": "cut", "description": "Lays shots end to end on the song's beats, renders them and muxes the song into one mp4. cut: {song, bpm? (instead of a song), start_beat?, fps?, scene? (default scene file), shots: [{name, scene? file, from (s), to? (retimes the shot to end there), beats (how long, default 4), speed?}]} or a cut .json path.",
         "inputSchema": {"type": "object", "properties": {"cut": {"description": "add resume: true to keep frames already rendered"}, "out": {"type": "string", "description": "the .mp4"}, "frames": {"type": "string", "description": "frame folder, default <out>.frames"},
            "samples": {"type": "integer"}, "scale": num("size factor")}, "required": ["cut", "out"]}},
        {"name": "agentcut", "description": "Sends a rendered .mp4 (or .png) to AgentCut, the video editor: uploads it as an asset and adds it as a clip to project (made if new) at start. url and key default to AGENTCUT_URL and AGENTCUT_KEY. Returns the asset id and AgentCut's edit_video result.",
         "inputSchema": {"type": "object", "properties": {"file": {"type": "string"}, "project": {"type": "string", "description": "default Paper Mache"}, "start": {"description": "seconds, or 0:15"},
            "url": {"type": "string"}, "key": {"type": "string"}}, "required": ["file"]}}
    ])
}
