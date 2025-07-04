struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) tex_coord: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coord: vec2<f32>,
};

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = vec4<f32>(input.position, 0.0, 1.0);
    output.tex_coord = input.tex_coord;
    return output;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Cursor detection (using special UV values)
    if (in.tex_coord.x < 0.0 && in.tex_coord.y < 0.0) {
        return vec4<f32>(1.0, 1.0, 1.0, 1.0);
    }
    
    let color = textureSample(tex, samp, in.tex_coord);
    return vec4<f32>(1.0, 1.0, 1.0, color.a);
}