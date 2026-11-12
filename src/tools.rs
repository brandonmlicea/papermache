use serde_json::{json, Value};

pub fn tools() -> Value {
    let v3 = |d: &str| json!({"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3, "description": d});
    let object = json!({"type": "object", "additionalProperties": true, "description":
        "An object: {name?, shape, ...shape fields, position?:[x,y,z], rotation?:[deg x,y,z], scale?:[x,y,z], material?, tags?:[..], hidden?}. \
         Shapes: box {size:[x,y,z], radius? bevel}; cylinder {radius, radius_top? (cone/frustum), height} along Z; sphere {radius} (scale makes ellipsoids); \
         torus {major, minor} in XY; rod {from, to, radius, radius_top?}; tube {points:[[x,y,z]..], radius} (a wire); plane {size:[w,h]} facing +Z; \
         text {text, size (em), align: center|left|right} flat in XY, readable from +Z; image {file: .png, width (mm), depth (mm, default 1), key [r,g,b] background to drop (default white), detail (cells a pixel, default 4)}: a picture raised into a solid, every column in its own pixel colour; mesh {file: .obj|.stl} or {vertices, faces}."});
    let target = |what: &str| json!({"name": {"type": "string", "description": format!("the object to {what}")}, "tag": {"type": "string", "description": format!("or every object with this tag")}});
    let mut update = target("change").as_object().unwrap().clone();
    update.insert("new_name".into(), json!({"type": "string"}));
    let camera = json!({"eye": v3("camera position"), "look": v3("point it looks at"), "up": v3("up hint, default [0,0,1]"),
        "projection": {"type": "string", "enum": ["perspective", "orthographic"]}, "lens": {"type": "number", "description": "focal length in mm, 36 mm sensor"},
        "fov": {"type": "number", "description": "degrees across the larger side, overrides lens"}, "width": {"type": "number", "description": "orthographic: mm across the larger side"},
        "size": {"type": "array", "items": {"type": "integer"}, "description": "[width, height] px"}});
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
            "emission": v3("light it gives off"), "opacity": {"type": "number"}}, "required": ["name"]}},
        {"name": "camera_set", "description": "Sets camera fields.", "inputSchema": {"type": "object", "properties": camera}},
        {"name": "camera_frame", "description": "Aims the camera so every visible object (or those with a tag) fills the view, from a direction.",
         "inputSchema": {"type": "object", "properties": {"tag": {"type": "string"}, "direction": v3("from the target toward the eye, default [1,-1.2,0.9]"),
            "projection": {"type": "string", "enum": ["perspective", "orthographic"]}, "margin": {"type": "number", "description": "1.1 = 10% room"}}}},
        {"name": "lights_set", "description": "Sun {direction toward the sun, angle (deg, softness), strength}, sky {color, strength}, background [r,g,b] or \"transparent\".",
         "inputSchema": {"type": "object", "properties": {"sun": {"type": "object"}, "sky": {"type": "object"}, "background": {}}}},
        {"name": "render", "description": "Renders the scene and shows it. path saves the full-size PNG; the image shown is at most max_size px.",
         "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}, "size": {"type": "array", "items": {"type": "integer"}}, "samples": {"type": "integer", "description": "default 8"},
            "max_size": {"type": "integer", "description": "default 800"}}}},
        {"name": "project", "description": "World points to pixel coordinates [x right, y down] at the camera's size.",
         "inputSchema": {"type": "object", "properties": {"points": {"type": "array", "items": {"type": "array", "items": {"type": "number"}}}}, "required": ["points"]}},
        {"name": "export", "description": "Writes the visible objects (or those with a tag) as .obj (with .mtl) or binary .stl, for printing or other tools. Text is skipped.",
         "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}, "tag": {"type": "string"}}, "required": ["path"]}}
    ])
}
