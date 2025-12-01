#import bevy_pbr::pbr_fragment::pbr_input_from_standard_material
#import bevy_pbr::pbr_functions::alpha_discard

#ifdef PREPASS_PIPELINE
#import bevy_pbr::prepass_io::{VertexOutput, FragmentOutput}
#import bevy_pbr::pbr_deferred_functions::deferred_output
#else
#import bevy_pbr::forward_io::{VertexOutput, FragmentOutput}
#import bevy_pbr::pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing}
#endif

struct LineData {
    line_start: vec2<f32>,
    line_end: vec2<f32>,
    line_width: f32,
    highlight_color: vec4<f32>,
};

struct CircleData {
	circle_center: vec2<f32>,
	circle_inner_radius: f32,
	circle_outer_radius: f32,
	highlight_color: vec4<f32>,
};

@group(2) @binding(100) var<storage, read> lines: array<LineData>;
@group(2) @binding(101) var<uniform> line_count: u32;

@group(2) @binding(102) var<storage, read> circles: array<CircleData>;
@group(2) @binding(103) var<uniform> circle_count: u32;

@group(2) @binding(104) var grass_texture: texture_2d<f32>;
@group(2) @binding(105) var grass_sampler: sampler;

@group(2) @binding(106) var stone_texture: texture_2d<f32>;
@group(2) @binding(107) var stone_sampler: sampler;

@group(2) @binding(108) var snow_texture: texture_2d<f32>;
@group(2) @binding(109) var snow_sampler: sampler;

@group(2) @binding(110) var<uniform> height_factors: vec2<f32>;
@group(2) @binding(111) var<uniform> repeat_factor: f32;

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    let y = in.world_position.y;
    let uv = in.uv * repeat_factor;

    let grass_color = textureSample(grass_texture, grass_sampler, uv);
    let stone_color = textureSample(stone_texture, stone_sampler, uv);
    let snow_color = textureSample(snow_texture, snow_sampler, uv);

    var base_color: vec4<f32>;
    
    let smooth_factor = 10.0;

    if (y < height_factors.x) {
        base_color = grass_color;
    } else if (y < height_factors.y) {
        let t = smoothstep(height_factors.x, height_factors.x + smooth_factor, y);
        base_color = mix(grass_color, stone_color, t);
    } else {
        let t = smoothstep(height_factors.y, height_factors.y + smooth_factor, y);
        base_color = mix(stone_color, snow_color, t);
    }

    var pbr = pbr_input_from_standard_material(in, is_front);
    pbr.material.base_color = alpha_discard(pbr.material, base_color);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr);
    out.color = main_pass_post_lighting_processing(pbr, out.color);

    let point = pbr.world_position.xz;

    for (var i = 0u; i < line_count; i = i + 1u) {
        let line = lines[i];
        let start = line.line_start;
        let end = line.line_end;
        let width = line.line_width;
        let color = line.highlight_color;

        let line_length = distance(start, end);
        if (line_length > 0.0) {
            let dir = normalize(end - start);
            let proj_point = start + clamp(dot(point - start, dir), 0.0, line_length) * dir;
            let dist_to_line = distance(point, proj_point);

            if (dist_to_line < width * 0.5) {
                out.color = color;
                break;
            }
        }
    }

    for (var i = 0u; i < circle_count; i = i + 1u) {
        let circle = circles[i];
        let center = circle.circle_center;
        let inner_radius = circle.circle_inner_radius;
        let outer_radius = circle.circle_outer_radius;
        let color = circle.highlight_color;

        let dist = distance(point, center);
        if (dist >= inner_radius && dist <= outer_radius) {
            out.color = color;
            break;
        }
    }

    return out;
}