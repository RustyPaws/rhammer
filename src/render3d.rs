//! OpenGL 3D viewport renderer, driven through egui's paint callback.

use eframe::glow::{self, HasContext};
use glam::{Mat4, Vec3};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub nrm: [f32; 3],
    pub uv: [f32; 2],
    pub col: [f32; 4],
}

pub struct Batch {
    pub material: String,
    pub verts: Vec<Vertex>,
}

#[derive(Default)]
pub struct Scene {
    pub world_version: u64,
    pub batches: Vec<Batch>,
    /// Studio model meshes; rebuilt every frame while animations play.
    pub models_version: u64,
    pub model_batches: Vec<Batch>,
    pub overlay_version: u64,
    /// Flat coloured translucent triangles (selection tint etc).
    pub overlay_tris: Vec<Vertex>,
    pub lines: Vec<Vertex>,
    /// Outlines that belong to the world mesh (point entity boxes).
    pub world_lines: Vec<Vertex>,
    /// RGBA textures waiting for upload: (material, w, h, data, alpha mode).
    pub uploads: Vec<(String, u32, u32, Vec<u8>, u8)>,
    pub mvp: Mat4,
    pub lighting: bool,
    pub cull: bool,
    pub wireframe: bool,
    pub clear: [f32; 3],
}

/// GPU buffers mirroring one `Scene`.
struct Bufs {
    world: Vec<(String, glow::Buffer, i32)>,
    world_version: u64,
    models: Vec<(String, glow::Buffer, i32)>,
    models_version: u64,
    overlay_version: u64,
    tris: Option<(glow::Buffer, i32)>,
    lines: Option<(glow::Buffer, i32)>,
}

impl Default for Bufs {
    fn default() -> Self {
        Bufs { world: vec![], world_version: u64::MAX, models: vec![], models_version: u64::MAX, overlay_version: u64::MAX, tris: None, lines: None }
    }
}

pub struct Gpu {
    prog: glow::Program,
    vao: glow::VertexArray,
    white: glow::Texture,
    /// Shared by every scene, so the model viewer reuses textures the 3D view uploaded.
    textures: HashMap<String, (glow::Texture, u8)>,
    u_mvp: Option<glow::UniformLocation>,
    u_tex: Option<glow::UniformLocation>,
    u_use_tex: Option<glow::UniformLocation>,
    u_light: Option<glow::UniformLocation>,
    u_amode: Option<glow::UniformLocation>,
}

/// Which scene a paint callback draws.
#[derive(Clone, Copy)]
pub enum Which {
    Main,
    Preview,
}

#[derive(Default)]
pub struct Shared {
    pub scene: Scene,
    /// The model viewer's scene.
    pub preview: Scene,
    gpu: Option<Gpu>,
    bufs: [Bufs; 2],
}

pub type SharedRef = Arc<Mutex<Shared>>;

const VS: &str = r#"#version 330 core
layout(location=0) in vec3 pos;
layout(location=1) in vec3 nrm;
layout(location=2) in vec2 uv;
layout(location=3) in vec4 col;
uniform mat4 mvp;
out vec3 vn; out vec2 vuv; out vec4 vcol;
void main(){ gl_Position = mvp*vec4(pos,1.0); vn=nrm; vuv=uv; vcol=col; }
"#;

const FS: &str = r#"#version 330 core
in vec3 vn; in vec2 vuv; in vec4 vcol;
uniform sampler2D tex; uniform int use_tex; uniform int light; uniform int amode;
out vec4 o;
void main(){
  vec4 c = vcol;
  if(use_tex==1){
    vec4 t = texture(tex, vuv);
    c.rgb *= t.rgb;
    // texture alpha is only transparency when the VMT says so (amode 1 = alphatest, 2 = translucent)
    if(amode==1){ if(t.a < 0.5) discard; }
    else if(amode==2) c.a *= t.a;
  }
  if(light==1){
    float l = 0.55 + 0.45*abs(dot(normalize(vn), normalize(vec3(0.35,0.5,0.8))));
    c.rgb *= l;
  }
  o = c;
}
"#;

unsafe fn as_bytes<T>(v: &[T]) -> &[u8] {
    std::slice::from_raw_parts(v.as_ptr() as *const u8, std::mem::size_of_val(v))
}

impl Gpu {
    fn new(gl: &glow::Context) -> Option<Gpu> {
        unsafe {
            let prog = gl.create_program().ok()?;
            for (ty, src) in [(glow::VERTEX_SHADER, VS), (glow::FRAGMENT_SHADER, FS)] {
                let sh = gl.create_shader(ty).ok()?;
                gl.shader_source(sh, src);
                gl.compile_shader(sh);
                if !gl.get_shader_compile_status(sh) {
                    eprintln!("shader error: {}", gl.get_shader_info_log(sh));
                    return None;
                }
                gl.attach_shader(prog, sh);
            }
            gl.link_program(prog);
            if !gl.get_program_link_status(prog) {
                eprintln!("link error: {}", gl.get_program_info_log(prog));
                return None;
            }
            let vao = gl.create_vertex_array().ok()?;
            let white = gl.create_texture().ok()?;
            gl.bind_texture(glow::TEXTURE_2D, Some(white));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                1,
                1,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(&[255, 255, 255, 255])),
            );
            Some(Gpu {
                u_mvp: gl.get_uniform_location(prog, "mvp"),
                u_tex: gl.get_uniform_location(prog, "tex"),
                u_use_tex: gl.get_uniform_location(prog, "use_tex"),
                u_light: gl.get_uniform_location(prog, "light"),
                u_amode: gl.get_uniform_location(prog, "amode"),
                prog,
                vao,
                white,
                textures: HashMap::new(),
            })
        }
    }

    unsafe fn upload_batches(gl: &glow::Context, batches: &[Batch], out: &mut Vec<(String, glow::Buffer, i32)>) {
        for (_, b, _) in out.drain(..) {
            gl.delete_buffer(b);
        }
        for b in batches {
            if let Some((buf, n)) = Self::upload_vertices(gl, &b.verts, None) {
                out.push((b.material.clone(), buf, n));
            }
        }
    }

    unsafe fn upload_vertices(gl: &glow::Context, verts: &[Vertex], old: Option<glow::Buffer>) -> Option<(glow::Buffer, i32)> {
        if let Some(b) = old {
            gl.delete_buffer(b);
        }
        if verts.is_empty() {
            return None;
        }
        let buf = gl.create_buffer().ok()?;
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(buf));
        gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, as_bytes(verts), glow::STATIC_DRAW);
        Some((buf, verts.len() as i32))
    }

    unsafe fn bind_attribs(gl: &glow::Context, buf: glow::Buffer) {
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(buf));
        let stride = std::mem::size_of::<Vertex>() as i32;
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, stride, 0);
        gl.enable_vertex_attrib_array(1);
        gl.vertex_attrib_pointer_f32(1, 3, glow::FLOAT, false, stride, 12);
        gl.enable_vertex_attrib_array(2);
        gl.vertex_attrib_pointer_f32(2, 2, glow::FLOAT, false, stride, 24);
        gl.enable_vertex_attrib_array(3);
        gl.vertex_attrib_pointer_f32(3, 4, glow::FLOAT, false, stride, 32);
    }

    fn sync(&mut self, gl: &glow::Context, scene: &mut Scene, bufs: &mut Bufs) {
        unsafe {
            for (name, w, h, data, mode) in scene.uploads.drain(..) {
                if let Ok(t) = gl.create_texture() {
                    gl.bind_texture(glow::TEXTURE_2D, Some(t));
                    gl.tex_image_2d(
                        glow::TEXTURE_2D,
                        0,
                        glow::RGBA8 as i32,
                        w as i32,
                        h as i32,
                        0,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        glow::PixelUnpackData::Slice(Some(&data)),
                    );
                    gl.generate_mipmap(glow::TEXTURE_2D);
                    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR_MIPMAP_LINEAR as i32);
                    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
                    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::REPEAT as i32);
                    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::REPEAT as i32);
                    if let Some((old, _)) = self.textures.insert(name, (t, mode)) {
                        gl.delete_texture(old);
                    }
                }
            }
            if bufs.world_version != scene.world_version {
                Self::upload_batches(gl, &scene.batches, &mut bufs.world);
                bufs.world_version = scene.world_version;
            }
            if bufs.models_version != scene.models_version {
                Self::upload_batches(gl, &scene.model_batches, &mut bufs.models);
                bufs.models_version = scene.models_version;
            }
            if bufs.overlay_version != scene.overlay_version {
                bufs.tris = Self::upload_vertices(gl, &scene.overlay_tris, bufs.tris.take().map(|t| t.0));
                bufs.lines = Self::upload_vertices(gl, &scene.lines, bufs.lines.take().map(|t| t.0));
                bufs.overlay_version = scene.overlay_version;
            }
        }
    }

    fn draw(&mut self, gl: &glow::Context, scene: &Scene, bufs: &Bufs, vp: [i32; 4]) {
        unsafe {
            gl.viewport(vp[0], vp[1], vp[2], vp[3]);
            gl.clear_color(scene.clear[0], scene.clear[1], scene.clear[2], 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
            // In Source the alpha channel of a texture carries masks (specular, self-illum, ...),
            // not transparency. Never write it to the window's framebuffer, or the compositor
            // would treat those pixels as see-through.
            gl.color_mask(true, true, true, false);
            gl.enable(glow::DEPTH_TEST);
            gl.depth_func(glow::LEQUAL);
            gl.disable(glow::CULL_FACE);
            gl.use_program(Some(self.prog));
            gl.bind_vertex_array(Some(self.vao));
            gl.uniform_matrix_4_f32_slice(self.u_mvp.as_ref(), false, &scene.mvp.to_cols_array());
            gl.uniform_1_i32(self.u_tex.as_ref(), 0);
            gl.active_texture(glow::TEXTURE0);

            // world
            gl.uniform_1_i32(self.u_light.as_ref(), scene.lighting as i32);
            gl.enable(glow::POLYGON_OFFSET_FILL);
            gl.polygon_offset(1.0, 1.0);
            if scene.wireframe {
                gl.polygon_mode(glow::FRONT_AND_BACK, glow::LINE);
            }
            gl.front_face(glow::CCW);
            gl.cull_face(glow::BACK);
            // pass 0 draws opaque and alpha-tested surfaces, pass 1 the translucent ones on top
            for pass in 0..2 {
                if pass == 1 {
                    gl.enable(glow::BLEND);
                    gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
                    gl.depth_mask(false);
                }
                for (mat, buf, n) in bufs.world.iter().chain(&bufs.models) {
                    let tex = self.textures.get(mat);
                    let mode = tex.map_or(0, |t| t.1);
                    if (mode == 2) != (pass == 1) {
                        continue;
                    }
                    // the untextured batch (entity boxes) is drawn two-sided
                    if mat.is_empty() || !scene.cull {
                        gl.disable(glow::CULL_FACE);
                    } else {
                        gl.enable(glow::CULL_FACE);
                    }
                    gl.bind_texture(glow::TEXTURE_2D, Some(tex.map_or(self.white, |t| t.0)));
                    gl.uniform_1_i32(self.u_use_tex.as_ref(), tex.is_some() as i32);
                    gl.uniform_1_i32(self.u_amode.as_ref(), mode as i32);
                    Self::bind_attribs(gl, *buf);
                    gl.draw_arrays(glow::TRIANGLES, 0, *n);
                }
            }
            gl.depth_mask(true);
            gl.disable(glow::BLEND);
            gl.uniform_1_i32(self.u_amode.as_ref(), 0);
            if scene.wireframe {
                gl.polygon_mode(glow::FRONT_AND_BACK, glow::FILL);
            }
            gl.disable(glow::POLYGON_OFFSET_FILL);
            gl.disable(glow::CULL_FACE);

            gl.bind_texture(glow::TEXTURE_2D, Some(self.white));
            gl.uniform_1_i32(self.u_use_tex.as_ref(), 0);
            gl.uniform_1_i32(self.u_light.as_ref(), 0);
            // overlay triangles (translucent)
            if let Some((buf, n)) = bufs.tris {
                gl.enable(glow::BLEND);
                gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
                gl.depth_mask(false);
                gl.enable(glow::POLYGON_OFFSET_FILL);
                gl.polygon_offset(-1.0, -1.0);
                Self::bind_attribs(gl, buf);
                gl.draw_arrays(glow::TRIANGLES, 0, n);
                gl.disable(glow::POLYGON_OFFSET_FILL);
                gl.depth_mask(true);
                gl.disable(glow::BLEND);
            }
            if let Some((buf, n)) = bufs.lines {
                Self::bind_attribs(gl, buf);
                gl.draw_arrays(glow::LINES, 0, n);
            }
            gl.color_mask(true, true, true, true);
            gl.bind_vertex_array(None);
            gl.use_program(None);
            gl.disable(glow::DEPTH_TEST);
            gl.bind_texture(glow::TEXTURE_2D, None);
        }
    }
}

pub fn paint_callback(shared: SharedRef, rect: eframe::egui::Rect) -> eframe::egui::PaintCallback {
    paint_callback_for(shared, rect, Which::Main)
}

pub fn paint_callback_for(shared: SharedRef, rect: eframe::egui::Rect, which: Which) -> eframe::egui::PaintCallback {
    let cb = eframe::egui_glow::CallbackFn::new(move |info, painter| {
        let gl = painter.gl();
        let mut guard = shared.lock().unwrap();
        let sh = &mut *guard;
        if sh.gpu.is_none() {
            sh.gpu = Gpu::new(gl);
        }
        if let Some(gpu) = sh.gpu.as_mut() {
            // textures queued on either scene go into the shared pool
            let mut ups = std::mem::take(&mut sh.preview.uploads);
            sh.scene.uploads.append(&mut ups);
            let [b0, b1] = &mut sh.bufs;
            let (scene, bufs) = match which {
                Which::Main => (&mut sh.scene, b0),
                Which::Preview => {
                    gpu.sync(gl, &mut sh.scene, b0);
                    (&mut sh.preview, b1)
                }
            };
            gpu.sync(gl, scene, bufs);
            let vp = info.viewport_in_pixels();
            gpu.draw(gl, scene, bufs, [vp.left_px, vp.from_bottom_px, vp.width_px, vp.height_px]);
        }
    });
    eframe::egui::PaintCallback { rect, callback: Arc::new(cb) }
}

/// Free-fly camera (Source coordinates: Z up).
#[derive(Clone, Debug)]
pub struct Camera {
    pub pos: glam::DVec3,
    pub yaw: f64,   // degrees, 0 = +X
    pub pitch: f64, // degrees, positive = looking up
    pub fov: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Camera { pos: glam::DVec3::new(-256.0, -256.0, 192.0), yaw: 45.0, pitch: -20.0, fov: 90.0 }
    }
}

impl Camera {
    pub fn forward(&self) -> glam::DVec3 {
        let (y, p) = (self.yaw.to_radians(), self.pitch.to_radians());
        glam::DVec3::new(y.cos() * p.cos(), y.sin() * p.cos(), p.sin())
    }
    pub fn right(&self) -> glam::DVec3 {
        self.forward().cross(glam::DVec3::Z).normalize()
    }
    pub fn matrices(&self, aspect: f32) -> Mat4 {
        let eye = Vec3::new(self.pos.x as f32, self.pos.y as f32, self.pos.z as f32);
        let f = self.forward();
        let dir = Vec3::new(f.x as f32, f.y as f32, f.z as f32);
        let view = Mat4::look_to_rh(eye, dir, Vec3::Z);
        // `fov` is the horizontal field of view (like Hammer); GL wants the vertical one.
        let aspect = aspect.max(0.01);
        let vfov = 2.0 * ((self.fov.to_radians() * 0.5).tan() / aspect).atan();
        let proj = Mat4::perspective_rh_gl(vfov, aspect, 4.0, 32768.0);
        proj * view
    }
    /// World-space ray through a normalised device position (x,y in -1..1, y up).
    pub fn ray(&self, aspect: f32, ndc: (f32, f32)) -> (glam::DVec3, glam::DVec3) {
        let inv = self.matrices(aspect).inverse();
        let p = inv * glam::Vec4::new(ndc.0, ndc.1, 1.0, 1.0);
        let p = p.truncate() / p.w;
        let o = self.pos;
        let d = (glam::DVec3::new(p.x as f64, p.y as f64, p.z as f64) - o).normalize();
        (o, d)
    }
    pub fn project(&self, aspect: f32, p: glam::DVec3) -> Option<(f32, f32)> {
        let m = self.matrices(aspect);
        let c = m * glam::Vec4::new(p.x as f32, p.y as f32, p.z as f32, 1.0);
        if c.w <= 0.0 {
            return None;
        }
        Some((c.x / c.w, c.y / c.w))
    }
}
