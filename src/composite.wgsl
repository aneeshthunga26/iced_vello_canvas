@group(0) @binding(0) var image: texture_2d<f32>;

struct Mapping {
    uv: vec4<f32>,
    destination: vec4<f32>,
}
@group(0) @binding(1) var<uniform> mapping: Mapping;

struct Vertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex fn vertex(@builtin(vertex_index) index: u32) -> Vertex {
    // Six vertices bound each retained tile exactly; padded pixels only feed filtering.
    let corners = array<vec2<f32>, 6>(
        vec2(0., 0.), vec2(1., 0.), vec2(0., 1.),
        vec2(0., 1.), vec2(1., 0.), vec2(1., 1.),
    );
    let local = corners[index];
    let position = mapping.destination.xy + local * mapping.destination.zw;
    return Vertex(
        vec4(position * vec2(2., -2.) + vec2(-1., 1.), 0., 1.),
        mapping.uv.xy + local * mapping.uv.zw,
    );
}

fn load_pixel(pixel: vec2<i32>) -> vec4<f32> {
    let size = vec2<i32>(textureDimensions(image));
    let color = textureLoad(image, clamp(pixel, vec2(0), size - vec2(1)), 0);
    // Convert before filtering, keeping interpolation premultiplied in Iced's space.
    var srgb = color.rgb;
    if (SOURCE_PREMULTIPLIED && color.a > 0.) {
        srgb /= color.a;
    }
    if (LINEAR_TARGET && color.a > 0.) {
        let linear = select(
            pow((srgb + vec3(0.055)) / 1.055, vec3(2.4)),
            srgb / 12.92,
            srgb <= vec3(0.04045),
        );
        return vec4(linear * color.a, color.a);
    }
    return vec4(srgb * color.a, color.a);
}

@fragment fn fragment(vertex: Vertex) -> @location(0) vec4<f32> {
    let p = vertex.uv * vec2<f32>(textureDimensions(image)) - vec2(0.5);
    let base = vec2<i32>(floor(p));
    let f = fract(p);
    return mix(
        mix(load_pixel(base), load_pixel(base + vec2(1, 0)), f.x),
        mix(load_pixel(base + vec2(0, 1)), load_pixel(base + vec2(1, 1)), f.x),
        f.y,
    );
}
