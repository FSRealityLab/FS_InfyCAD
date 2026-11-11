// リリース版の Windows ではコンソール窓を出さない。
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod renderer;

use std::sync::{Arc, Mutex};

use eframe::egui;
use glam::{Vec2, Vec3};
use infycad_app::camera::{Camera, ray_work_plane};
use infycad_app::session::Session;
use infycad_smlib::SmlibKernel;
use renderer::{Renderer, Scene};

const BACKGROUND: egui::Color32 = egui::Color32::from_rgb(33, 40, 48);
const SOLID_COLOR: [f32; 3] = [0.62, 0.68, 0.74];
const SELECTED_COLOR: [f32; 3] = [0.25, 0.60, 1.0];
const GRID_HALF_CELLS: i32 = 10;
const GRID_SPACING: f32 = 10.0;

fn main() -> eframe::Result {
    // `--run "BOX;0,0,0;10,10,10"`: 起動時に実行するコマンドライン入力（`;` 区切り）。
    let mut args = std::env::args().skip(1);
    let mut script = Vec::new();
    while let Some(arg) = args.next() {
        if arg == "--run" {
            script = args.next().unwrap_or_default().split(';').map(str::to_owned).collect();
        }
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 800.0]).with_title("FS InfyCAD"),
        depth_buffer: 24,
        ..Default::default()
    };
    eframe::run_native("FS InfyCAD", options, Box::new(move |cc| Ok(Box::new(App::new(cc, &script)?))))
}

struct App {
    session: Session<SmlibKernel>,
    camera: Camera,
    renderer: Arc<Mutex<Renderer>>,
    scene: Arc<Scene>,
    input: String,
    /// 作業平面上のカーソル位置（ステータス表示用）。
    cursor: Option<Vec3>,
    /// 次のフレームでコマンドラインにフォーカスを戻す。
    refocus: bool,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, script: &[String]) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let gl = cc.gl.as_ref().ok_or("OpenGL context is not available")?;
        let renderer = Renderer::new(gl)?;
        let mut session = Session::new(SmlibKernel::new());
        for line in script {
            session.submit_text(line);
        }
        let mut camera = Camera::default();
        if let Some((min, max)) = session.bounds() {
            camera.frame(min, max);
        }
        let scene = Arc::new(build_scene(&session));
        Ok(Self { session, camera, renderer: Arc::new(Mutex::new(renderer)), scene, input: String::new(), cursor: None, refocus: true })
    }

    fn ribbon(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong("3D");
            ui.separator();
            for (label, command) in [
                ("Box", "BOX"),
                ("Cylinder", "CYLINDER"),
                ("Sphere", "SPHERE"),
                ("Union", "UNION"),
                ("Subtract", "SUBTRACT"),
                ("Intersect", "INTERSECT"),
                ("Erase", "ERASE"),
            ] {
                if ui.button(label).clicked() {
                    self.refocus = true;
                    if !self.session.is_idle() {
                        self.session.cancel();
                    }
                    self.session.submit_text(command);
                }
            }
            ui.separator();
            if ui.button("Zoom Extents").clicked() {
                self.zoom_extents();
            }
        });
    }

    fn zoom_extents(&mut self) {
        match self.session.bounds() {
            Some((min, max)) => self.camera.frame(min, max),
            None => self.camera = Camera::default(),
        }
    }

    fn command_line(&mut self, ui: &mut egui::Ui) {
        let log = self.session.log();
        for line in &log[log.len().saturating_sub(4)..] {
            ui.label(egui::RichText::new(line).monospace().weak());
        }
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(self.session.prompt()).monospace().strong());
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.input)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(f32::INFINITY),
            );
            // AutoCAD と同じく、Enter でもスペースでも確定する。
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let space = self.input.ends_with(' ');
            if enter || space {
                let text = std::mem::take(&mut self.input);
                if text.trim().eq_ignore_ascii_case("Z") || text.trim().eq_ignore_ascii_case("ZOOM") {
                    self.zoom_extents();
                } else {
                    self.session.submit_text(&text);
                }
            }
            // 打った文字がコマンドラインに入るよう、確定やクリックのあとにフォーカスを戻す。
            // 毎フレーム要求すると再描画が止まらず、入力イベントが届かなくなる。
            if std::mem::take(&mut self.refocus) || edit.lost_focus() {
                edit.request_focus();
            }
        });
        ui.horizontal(|ui| {
            let text = match self.cursor {
                Some(p) => format!("{:.2}, {:.2}, {:.2}", p.x, p.y, p.z),
                None => String::new(),
            };
            ui.label(egui::RichText::new(text).monospace().weak());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new("Wheel: zoom   Middle drag: pan   Shift+Middle / Right drag: orbit   Esc: cancel").weak());
            });
        });
    }

    fn viewport(&mut self, ui: &mut egui::Ui) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        ui.painter().rect_filled(rect, 0.0, BACKGROUND);
        let aspect = rect.width() / rect.height().max(1.0);

        let shift = ui.input(|i| i.modifiers.shift);
        let delta = response.drag_delta();
        let delta = Vec2::new(delta.x, delta.y);
        let middle = response.dragged_by(egui::PointerButton::Middle);
        if response.dragged_by(egui::PointerButton::Secondary) || (middle && shift) {
            self.camera.orbit(delta);
        } else if middle {
            self.camera.pan(delta, rect.height());
        }
        if response.hovered() {
            self.camera.zoom(ui.input(|i| i.smooth_scroll_delta().y));
        }

        let ray = response.hover_pos().map(|pos| {
            let uv = (pos - rect.min) / rect.size();
            self.camera.ray(Vec2::new(uv.x, uv.y), aspect)
        });
        self.cursor = ray.and_then(|(origin, dir)| ray_work_plane(origin, dir));
        if response.clicked() || response.drag_stopped() {
            self.refocus = true;
        }
        if response.clicked_by(egui::PointerButton::Primary)
            && let Some((origin, dir)) = ray {
                if self.session.wants_point() {
                    if let Some(p) = self.cursor {
                        self.session.click_point([p.x as f64, p.y as f64, 0.0]);
                    }
                } else {
                    let hit = self.session.pick(origin, dir);
                    self.session.click_solid(hit);
                }
            }

        if self.scene.version != self.session.version() {
            self.scene = Arc::new(build_scene(&self.session));
        }
        let (renderer, scene) = (self.renderer.clone(), self.scene.clone());
        let view_proj = self.camera.view_proj(aspect).to_cols_array();
        let callback = egui_glow_callback(move |info, gl| {
            let v = info.viewport_in_pixels();
            if let Ok(mut renderer) = renderer.lock() {
                renderer.paint(gl, &scene, &view_proj, [v.left_px, v.from_bottom_px, v.width_px, v.height_px]);
            }
        });
        ui.painter().add(egui::PaintCallback { rect, callback: Arc::new(callback) });
    }
}

fn egui_glow_callback(
    paint: impl Fn(egui::PaintCallbackInfo, &eframe::glow::Context) + Send + Sync + 'static,
) -> eframe::egui_glow::CallbackFn {
    eframe::egui_glow::CallbackFn::new(move |info, painter| paint(info, painter.gl()))
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.session.cancel();
            self.input.clear();
        }
        egui::Panel::top("ribbon").show(ui, |ui| self.ribbon(ui));
        egui::Panel::bottom("command_line").show(ui, |ui| self.command_line(ui));
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| self.viewport(ui));
    }
}

fn build_scene(session: &Session<SmlibKernel>) -> Scene {
    let mut scene = Scene {
        version: session.version(),
        triangles: Vec::new(),
        solids: Vec::new(),
        lines: Vec::new(),
        line_groups: Vec::new(),
    };
    for solid in session.solids() {
        let first = (scene.triangles.len() / 6) as i32;
        for index in &solid.mesh.indices {
            let (p, n) = (solid.mesh.positions[*index as usize], solid.mesh.normals[*index as usize]);
            scene.triangles.extend(p.iter().chain(&n).map(|v| *v as f32));
        }
        let color = if session.selection().contains(&solid.id) { SELECTED_COLOR } else { SOLID_COLOR };
        scene.solids.push((first, (scene.triangles.len() / 6) as i32 - first, color));
    }

    let mut group = |points: &[[f32; 3]], color| {
        let first = (scene.lines.len() / 6) as i32;
        for p in points {
            scene.lines.extend([p[0], p[1], p[2], 0.0, 0.0, 1.0]);
        }
        scene.line_groups.push((first, points.len() as i32, color));
    };
    let extent = GRID_HALF_CELLS as f32 * GRID_SPACING;
    let mut grid = Vec::new();
    for i in -GRID_HALF_CELLS..=GRID_HALF_CELLS {
        let t = i as f32 * GRID_SPACING;
        grid.extend([[t, -extent, 0.0], [t, extent, 0.0], [-extent, t, 0.0], [extent, t, 0.0]]);
    }
    group(&grid, [0.22, 0.26, 0.31]);
    // UCS アイコンの色: X 赤、Y 緑、Z 青。
    group(&[[0.0; 3], [extent, 0.0, 0.0]], [0.85, 0.25, 0.25]);
    group(&[[0.0; 3], [0.0, extent, 0.0]], [0.30, 0.75, 0.30]);
    group(&[[0.0; 3], [0.0, 0.0, extent * 0.5]], [0.30, 0.50, 0.95]);
    scene
}
