#import bevy_pbr::pbr_fragment::pbr_input_from_standard_material
#import bevy_pbr::pbr_functions::alpha_discard

#ifdef PREPASS_PIPELINE
#import bevy_pbr::prepass_io::{VertexOutput, FragmentOutput}
#import bevy_pbr::pbr_deferred_functions::deferred_output
#else
#import bevy_pbr::forward_io::{VertexOutput, FragmentOutput}
#import bevy_pbr::pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing}
#endif

@group(2) @binding(120) var<uniform> white_replacement: vec4<f32>;

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr = pbr_input_from_standard_material(in, is_front);

    var base_color = pbr.material.base_color;

    let is_white = 
        base_color.r > 0.95 &&
        base_color.g > 0.95 &&
        base_color.b > 0.95 &&
        base_color.a > 0.95;

    if (is_white) {
        base_color = white_replacement;
    }

    pbr.material.base_color = base_color;

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr);
    out.color = main_pass_post_lighting_processing(pbr, out.color);

    return out;
}