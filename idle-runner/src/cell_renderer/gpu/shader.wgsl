struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) fg_color: vec4<f32>,
    @location(2) bg_color: vec4<f32>,
    @location(3) @interpolate(flat) char_idx: u32,
    @location(4) @interpolate(flat) bold: u32,
    @location(5) @interpolate(flat) is_odd_row: u32,
}

struct Uniforms {
    cols: u32,
    rows: u32,
    cell_width: u32,
    cell_height: u32,
    atlas_cols: u32,
    atlas_rows: u32,
    scanlines: u32,
    padding: u32,
}

struct Cell {
    bg_color: u32,
    fg_color: u32,
    char_idx: u32,
    bold: u32,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var<storage, read> cells: array<Cell>;
@group(0) @binding(2) var atlas_tex: texture_2d<f32>;
@group(0) @binding(3) var atlas_sampler: sampler;

@vertex
fn vs_main(
    @builtin(vertex_index) vertex_idx: u32,
    @builtin(instance_index) instance_idx: u32,
) -> VertexOutput {
    let cell = cells[instance_idx];
    
    let col = instance_idx % uniforms.cols;
    let row = instance_idx / uniforms.cols;
    
    var local_pos = vec2<f32>(0.0, 0.0);
    if (vertex_idx == 1u || vertex_idx == 4u) {
        local_pos.x = 1.0;
    }
    if (vertex_idx == 2u || vertex_idx == 3u) {
        local_pos.y = 1.0;
    }
    if (vertex_idx == 5u) {
        local_pos.x = 1.0;
        local_pos.y = 1.0;
    }
    
    let px = (f32(col) + local_pos.x) * f32(uniforms.cell_width);
    let py = (f32(row) + local_pos.y) * f32(uniforms.cell_height);
    
    let content_w = f32(uniforms.cols * uniforms.cell_width);
    let content_h = f32(uniforms.rows * uniforms.cell_height);
    
    var out: VertexOutput;
    out.position = vec4<f32>(
        (px / content_w) * 2.0 - 1.0,
        1.0 - (py / content_h) * 2.0,
        0.0,
        1.0
    );
    
    out.uv = local_pos;
    
    let bg_r = f32((cell.bg_color >> 16u) & 0xFFu) / 255.0;
    let bg_g = f32((cell.bg_color >> 8u) & 0xFFu) / 255.0;
    let bg_b = f32(cell.bg_color & 0xFFu) / 255.0;
    out.bg_color = vec4<f32>(bg_r, bg_g, bg_b, 1.0);
    
    let fg_r = f32((cell.fg_color >> 16u) & 0xFFu) / 255.0;
    let fg_g = f32((cell.fg_color >> 8u) & 0xFFu) / 255.0;
    let fg_b = f32(cell.fg_color & 0xFFu) / 255.0;
    out.fg_color = vec4<f32>(fg_r, fg_g, fg_b, 1.0);
    
    out.char_idx = cell.char_idx;
    out.bold = cell.bold;
    out.is_odd_row = row % 2u;
    
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var color = in.bg_color;
    
    if (in.char_idx != 0xFFFFFFFFu) {
        let char_col = in.char_idx % uniforms.atlas_cols;
        let char_row = in.char_idx / uniforms.atlas_cols;
        
        let atlas_w = f32(uniforms.atlas_cols * uniforms.cell_width);
        let atlas_h = f32(uniforms.atlas_rows * uniforms.cell_height);
        
        let u0 = f32(char_col * uniforms.cell_width) / atlas_w;
        let v0 = f32(char_row * uniforms.cell_height) / atlas_h;
        let u1 = f32((char_col + 1u) * uniforms.cell_width) / atlas_w;
        let v1 = f32((char_row + 1u) * uniforms.cell_height) / atlas_h;
        
        let uv = vec2<f32>(
            mix(u0, u1, in.uv.x),
            mix(v0, v1, in.uv.y)
        );
        
        var alpha = textureSample(atlas_tex, atlas_sampler, uv).r;
        
        if (in.bold != 0u) {
            let offset = vec2<f32>(1.0 / atlas_w, 0.0);
            let alpha_offset = textureSample(atlas_tex, atlas_sampler, uv - offset).r;
            alpha = max(alpha, alpha_offset);
        }
        
        color = mix(color, in.fg_color, alpha);
    }
    
    if (uniforms.scanlines != 0u && in.is_odd_row != 0u) {
        color = vec4<f32>(color.rgb * 0.5, color.a);
    }
    
    return color;
}