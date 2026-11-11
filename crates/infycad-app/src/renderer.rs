//! OpenGL でソリッドとグリッドを描く。egui の描画コールバックから呼ばれる。

use eframe::glow::{self, HasContext as _};

/// 1 フレームに描く内容。`version` が変わったときだけ頂点を送り直す。
pub struct Scene {
    pub version: u64,
    /// 三角形の頂点: 位置 3 + 法線 3。
    pub triangles: Vec<f32>,
    /// (最初の頂点, 頂点数, 色)。ソリッドごと。
    pub solids: Vec<(i32, i32, [f32; 3])>,
    /// 線分の頂点（位置 3 + 未使用の法線 3）。
    pub lines: Vec<f32>,
    /// (最初の頂点, 頂点数, 色)。
    pub line_groups: Vec<(i32, i32, [f32; 3])>,
}

pub struct Renderer {
    program: glow::Program,
    triangles: (glow::VertexArray, glow::Buffer),
    lines: (glow::VertexArray, glow::Buffer),
    uploaded: Option<u64>,
}

const VERTEX: &str = r#"
layout(location = 0) in vec3 a_pos;
layout(location = 1) in vec3 a_normal;
uniform mat4 u_view_proj;
out vec3 v_normal;
void main() {
    v_normal = a_normal;
    gl_Position = u_view_proj * vec4(a_pos, 1.0);
}
"#;

const FRAGMENT: &str = r#"
precision mediump float;
in vec3 v_normal;
uniform vec3 u_color;
uniform float u_lit;
out vec4 out_color;
void main() {
    // 法線の向きに依存しないよう両面で照らす。
    float diffuse = abs(dot(normalize(v_normal), normalize(vec3(0.35, -0.55, 0.75))));
    float shade = mix(1.0, 0.35 + 0.65 * diffuse, u_lit);
    out_color = vec4(u_color * shade, 1.0);
}
"#;

impl Renderer {
    pub fn new(gl: &glow::Context) -> Result<Self, String> {
        let header = if gl.version().is_embedded { "#version 300 es\n" } else { "#version 330 core\n" };
        // SAFETY: eframe が作った現在の GL コンテキストに対する通常の呼び出し。
        unsafe {
            let program = gl.create_program()?;
            let mut shaders = Vec::new();
            for (kind, source) in [(glow::VERTEX_SHADER, VERTEX), (glow::FRAGMENT_SHADER, FRAGMENT)] {
                let shader = gl.create_shader(kind)?;
                gl.shader_source(shader, &format!("{header}{source}"));
                gl.compile_shader(shader);
                if !gl.get_shader_compile_status(shader) {
                    return Err(gl.get_shader_info_log(shader));
                }
                gl.attach_shader(program, shader);
                shaders.push(shader);
            }
            gl.link_program(program);
            if !gl.get_program_link_status(program) {
                return Err(gl.get_program_info_log(program));
            }
            for shader in shaders {
                gl.detach_shader(program, shader);
                gl.delete_shader(shader);
            }
            let triangles = Self::vertex_array(gl)?;
            let lines = Self::vertex_array(gl)?;
            Ok(Self { program, triangles, lines, uploaded: None })
        }
    }

    unsafe fn vertex_array(gl: &glow::Context) -> Result<(glow::VertexArray, glow::Buffer), String> {
        // SAFETY: 呼び出し側と同じ GL コンテキスト。
        unsafe {
            let vao = gl.create_vertex_array()?;
            let vbo = gl.create_buffer()?;
            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, 24, 0);
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(1, 3, glow::FLOAT, false, 24, 12);
            gl.bind_vertex_array(None);
            Ok((vao, vbo))
        }
    }

    /// `viewport` は (左, 下から, 幅, 高さ) のピクセル。
    pub fn paint(&mut self, gl: &glow::Context, scene: &Scene, view_proj: &[f32; 16], viewport: [i32; 4]) {
        // SAFETY: egui の描画コールバックの中で、同じ GL コンテキストに対して呼ばれる。
        unsafe {
            if self.uploaded != Some(scene.version) {
                for (buffer, data) in [(self.triangles.1, &scene.triangles), (self.lines.1, &scene.lines)] {
                    let bytes = std::slice::from_raw_parts(data.as_ptr().cast::<u8>(), std::mem::size_of_val(&data[..]));
                    gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer));
                    gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::DYNAMIC_DRAW);
                }
                self.uploaded = Some(scene.version);
            }

            let [x, y, w, h] = viewport;
            gl.viewport(x, y, w, h);
            gl.enable(glow::SCISSOR_TEST);
            gl.scissor(x, y, w, h);
            gl.enable(glow::DEPTH_TEST);
            gl.depth_func(glow::LEQUAL);
            gl.depth_mask(true);
            gl.clear(glow::DEPTH_BUFFER_BIT);
            gl.disable(glow::BLEND);
            gl.disable(glow::CULL_FACE);

            gl.use_program(Some(self.program));
            let uniform = |name| gl.get_uniform_location(self.program, name);
            gl.uniform_matrix_4_f32_slice(uniform("u_view_proj").as_ref(), false, view_proj);
            let (color, lit) = (uniform("u_color"), uniform("u_lit"));

            // 面と同じ深さの線（グリッド）がちらつかないよう、面を少し奥へ。
            gl.enable(glow::POLYGON_OFFSET_FILL);
            gl.polygon_offset(1.0, 1.0);
            gl.uniform_1_f32(lit.as_ref(), 1.0);
            gl.bind_vertex_array(Some(self.triangles.0));
            for (first, count, rgb) in &scene.solids {
                gl.uniform_3_f32_slice(color.as_ref(), rgb);
                gl.draw_arrays(glow::TRIANGLES, *first, *count);
            }
            gl.disable(glow::POLYGON_OFFSET_FILL);

            gl.uniform_1_f32(lit.as_ref(), 0.0);
            gl.bind_vertex_array(Some(self.lines.0));
            for (first, count, rgb) in &scene.line_groups {
                gl.uniform_3_f32_slice(color.as_ref(), rgb);
                gl.draw_arrays(glow::LINES, *first, *count);
            }

            gl.bind_vertex_array(None);
            gl.use_program(None);
            gl.disable(glow::DEPTH_TEST);
            gl.enable(glow::BLEND);
        }
    }
}
