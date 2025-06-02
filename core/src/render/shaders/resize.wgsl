@group(0) @binding(0)
var input_texture: texture_2d<f32>;

@group(0) @binding(1)
var output_texture: texture_storage_2d<rgba8unorm, write>;

@group(0) @binding(2)
var<uniform> params: vec4<f32>; // [input_width, input_height, output_width, output_height]

// Increased workgroup size for better GPU utilization
@compute @workgroup_size(16, 16, 1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let output_size = vec2<u32>(u32(params[2]), u32(params[3]));
    let input_size = vec2<f32>(params[0], params[1]);

    // Early return for out-of-bounds
    if (global_id.x >= output_size.x || global_id.y >= output_size.y) {
        return;
    }

    // Pre-compute scaling factors instead of division per pixel
    let scale_x = input_size.x / f32(output_size.x);
    let scale_y = input_size.y / f32(output_size.y);
    
    // Calculate source position with pre-computed scales
    let src_x = f32(global_id.x) * scale_x;
    let src_y = f32(global_id.y) * scale_y;
    
    // For small downscaling, use nearest neighbor for speed
    if scale_x < 1.2 && scale_y < 1.2 {
        let src_pixel = vec2<u32>(u32(src_x), u32(src_y));
        let color = textureLoad(input_texture, src_pixel, 0);
        textureStore(output_texture, global_id.xy, color);
        return;
    }
    
    // For larger scaling factors, use a simplified bilinear approach
    let src_pos_floor = vec2<u32>(u32(src_x), u32(src_y));
    let fract_x = src_x - f32(src_pos_floor.x);
    let fract_y = src_y - f32(src_pos_floor.y);
    
    // Get source coordinates with bounds checking
    let x1 = src_pos_floor.x;
    let y1 = src_pos_floor.y;
    let x2 = min(x1 + 1, u32(input_size.x) - 1);
    let y2 = min(y1 + 1, u32(input_size.y) - 1);
    
    // Fetch the four pixels
    let sample00 = textureLoad(input_texture, vec2<u32>(x1, y1), 0);
    let sample10 = textureLoad(input_texture, vec2<u32>(x2, y1), 0);
    let sample01 = textureLoad(input_texture, vec2<u32>(x1, y2), 0);
    let sample11 = textureLoad(input_texture, vec2<u32>(x2, y2), 0);

    // Skip full interpolation if a pixel is completely transparent
    if (sample00.a < 0.01 && sample10.a < 0.01 && sample01.a < 0.01 && sample11.a < 0.01) {
        textureStore(output_texture, global_id.xy, vec4<f32>(0.0, 0.0, 0.0, 0.0));
        return;
    }

    // Simple bilinear interpolation
    let top = mix(sample00, sample10, fract_x);
    let bottom = mix(sample01, sample11, fract_x);
    let final_color = mix(top, bottom, fract_y);
    
    // Write to output
    textureStore(output_texture, global_id.xy, final_color);
}