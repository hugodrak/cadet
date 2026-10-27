struct Globals {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    light: vec4<f32>,
    grid: vec4<f32>,
    grid_o: vec4<f32>,
    grid_u: vec4<f32>,
    grid_v: vec4<f32>,
    bg_top: vec4<f32>,
    bg_bottom: vec4<f32>,
    body: vec4<f32>,
    edge: vec4<f32>,
    grid_minor: vec4<f32>,
    grid_major: vec4<f32>,
    axis_x: vec4<f32>,
    axis_y: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;

fn to_gamma(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3(0.0)), vec3(1.0 / 2.2));
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return pow(c, vec3(2.2));
}

struct BgOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) t: f32,
};

@vertex
fn vs_bg(@builtin(vertex_index) i: u32) -> BgOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: BgOut;
    o.pos = vec4(uv * 2.0 - 1.0, 0.0, 1.0);
    o.t = uv.y * 0.5;
    return o;
}

@fragment
fn fs_bg(in: BgOut) -> @location(0) vec4<f32> {
    return vec4(mix(g.bg_bottom.rgb, g.bg_top.rgb, clamp(in.t, 0.0, 1.0)), 1.0);
}

struct MeshOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

@vertex
fn vs_mesh(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>) -> MeshOut {
    var o: MeshOut;
    o.pos = g.view_proj * vec4(p, 1.0);
    o.world = p;
    o.normal = n;
    return o;
}

@fragment
fn fs_mesh(in: MeshOut) -> @location(0) vec4<f32> {
    let n = normalize(in.normal);
    let v = normalize(g.eye.xyz - in.world);
    let base = to_linear(g.body.rgb);
    let l = g.light.xyz;
    let hemi = mix(0.6, 1.0, n.z * 0.5 + 0.5);
    let key = max(dot(n, l), 0.0);
    let spec = pow(max(dot(n, normalize(v + l)), 0.0), 64.0) * 0.1;
    let c = base * (0.36 * hemi + 0.72 * key) + vec3(spec);
    return vec4(to_gamma(c), 1.0);
}

@vertex
fn vs_edge(@location(0) p: vec3<f32>) -> @builtin(position) vec4<f32> {
    return g.view_proj * vec4(mix(p, g.eye.xyz, 0.002), 1.0);
}

@fragment
fn fs_edge() -> @location(0) vec4<f32> {
    return g.edge;
}

struct GridOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) world: vec3<f32>,
};

fn plane_uv(w: vec3<f32>) -> vec2<f32> {
    let d = w - g.grid_o.xyz;
    return vec2(dot(d, g.grid_u.xyz), dot(d, g.grid_v.xyz));
}

@vertex
fn vs_grid(@builtin(vertex_index) i: u32) -> GridOut {
    let corners = array(vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, 1.0));
    let uv = corners[i] * g.grid.y + floor(plane_uv(g.eye.xyz) / g.grid.x) * g.grid.x;
    let w = g.grid_o.xyz + g.grid_u.xyz * uv.x + g.grid_v.xyz * uv.y;
    var o: GridOut;
    o.pos = g.view_proj * vec4(w, 1.0);
    o.world = w;
    return o;
}

fn lines(p: vec2<f32>, step: f32) -> f32 {
    let q = p / step;
    let d = abs(fract(q - 0.5) - 0.5) / fwidth(q);
    return 1.0 - min(min(d.x, d.y), 1.0);
}

@fragment
fn fs_grid(in: GridOut) -> @location(0) vec4<f32> {
    let p = plane_uv(in.world);
    let fade = 1.0 - smoothstep(g.grid.y * 0.25, g.grid.y, length(p - plane_uv(g.eye.xyz)));
    var c = mix(vec4(g.grid_minor.rgb, g.grid_minor.a * lines(p, g.grid.x)), g.grid_major, lines(p, g.grid.x * 10.0));
    let fw = fwidth(p);
    c = mix(c, g.axis_x, 1.0 - min(abs(p.y) / fw.y, 1.0));
    c = mix(c, g.axis_y, 1.0 - min(abs(p.x) / fw.x, 1.0));
    return vec4(c.rgb, c.a * fade);
}
