//! Z 軸が上のオービットカメラ。

use glam::{Mat4, Vec2, Vec3, Vec4};

#[derive(Debug, Clone, Copy)]
pub struct Camera {
    pub target: Vec3,
    /// Z 軸まわりの角度（ラジアン）。0 で +X 側から見る。
    pub yaw: f32,
    /// 水平面からの仰角（ラジアン）。
    pub pitch: f32,
    pub distance: f32,
}

const FOV_Y: f32 = std::f32::consts::FRAC_PI_4;
const PITCH_LIMIT: f32 = 1.55;

impl Default for Camera {
    /// AutoCAD の「南東アイソメ」: +X, -Y, +Z 側から見下ろす。
    fn default() -> Self {
        Self { target: Vec3::ZERO, yaw: -std::f32::consts::FRAC_PI_4, pitch: 0.615, distance: 60.0 }
    }
}

impl Camera {
    pub fn eye(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        self.target + self.distance * Vec3::new(cp * cy, cp * sy, sp)
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        // 右手系のビュー行列: カメラは -Z を向く。
        let eye = self.eye();
        let f = (self.target - eye).normalize();
        let r = f.cross(Vec3::Z).normalize();
        let u = r.cross(f);
        let view = Mat4::from_cols(
            r.x * Vec4::X + u.x * Vec4::Y - f.x * Vec4::Z,
            r.y * Vec4::X + u.y * Vec4::Y - f.y * Vec4::Z,
            r.z * Vec4::X + u.z * Vec4::Y - f.z * Vec4::Z,
            Vec4::new(-r.dot(eye), -u.dot(eye), f.dot(eye), 1.0),
        );
        // OpenGL の透視投影: 深度は -1..1。
        let (near, far) = (self.distance * 0.01, self.distance * 100.0);
        let h = 1.0 / (FOV_Y * 0.5).tan();
        let proj = Mat4::from_cols(
            Vec4::X * (h / aspect.max(1e-3)),
            Vec4::Y * h,
            Vec4::new(0.0, 0.0, (far + near) / (near - far), -1.0),
            Vec4::Z * (2.0 * far * near / (near - far)),
        );
        proj * view
    }

    pub fn orbit(&mut self, delta_px: Vec2) {
        self.yaw -= delta_px.x * 0.008;
        self.pitch = (self.pitch + delta_px.y * 0.008).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }

    /// 画面上で `delta_px` だけドラッグした分、注視点の深さにある物がカーソルについてくるように動かす。
    pub fn pan(&mut self, delta_px: Vec2, viewport_height_px: f32) {
        let forward = (self.target - self.eye()).normalize();
        let right = forward.cross(Vec3::Z).normalize();
        let up = right.cross(forward);
        let world_per_px = 2.0 * self.distance * (FOV_Y * 0.5).tan() / viewport_height_px.max(1.0);
        self.target += (-delta_px.x * right + delta_px.y * up) * world_per_px;
    }

    /// 正の `scroll` で近づく。
    pub fn zoom(&mut self, scroll: f32) {
        self.distance = (self.distance * (-scroll * 0.002).exp()).clamp(1e-3, 1e7);
    }

    /// 境界ボックス全体が収まるようにする（ZOOM Extents）。
    pub fn frame(&mut self, min: Vec3, max: Vec3) {
        self.target = (min + max) * 0.5;
        let radius = ((max - min).length() * 0.5).max(1e-3);
        self.distance = radius / (FOV_Y * 0.5).sin() * 1.1;
    }

    /// ビューポート内の位置 `uv`（左上 0,0 〜 右下 1,1）を通る視線。(始点, 単位方向)。
    pub fn ray(&self, uv: Vec2, aspect: f32) -> (Vec3, Vec3) {
        let inverse = self.view_proj(aspect).inverse();
        let ndc = Vec2::new(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
        let near = inverse.project_point3(ndc.extend(-1.0));
        let far = inverse.project_point3(ndc.extend(1.0));
        (near, (far - near).normalize())
    }
}

/// 視線と作業平面 z = 0 の交点。平面に平行、または後ろ向きなら `None`。
pub fn ray_work_plane(origin: Vec3, dir: Vec3) -> Option<Vec3> {
    if dir.z.abs() < 1e-6 {
        return None;
    }
    let t = -origin.z / dir.z;
    (t > 0.0).then(|| origin + dir * t)
}

/// 視線と三角形の交差（Möller–Trumbore）。交点までの距離を返す。
pub fn ray_triangle(origin: Vec3, dir: Vec3, tri: [Vec3; 3]) -> Option<f32> {
    let (e1, e2) = (tri[1] - tri[0], tri[2] - tri[0]);
    let p = dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-12 {
        return None;
    }
    let s = origin - tri[0];
    let u = s.dot(p) / det;
    let q = s.cross(e1);
    let v = dir.dot(q) / det;
    let t = e2.dot(q) / det;
    (u >= 0.0 && v >= 0.0 && u + v <= 1.0 && t > 0.0).then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_ray_hits_the_target() {
        let camera = Camera { target: Vec3::new(3.0, 4.0, 0.0), ..Camera::default() };
        let (origin, dir) = camera.ray(Vec2::splat(0.5), 1.6);
        let hit = ray_work_plane(origin, dir).unwrap();
        assert!((hit - camera.target).length() < 1e-2, "{hit}");
    }

    #[test]
    fn framing_keeps_the_box_in_view() {
        let mut camera = Camera::default();
        let (min, max) = (Vec3::new(100.0, 100.0, 0.0), Vec3::new(140.0, 120.0, 30.0));
        camera.frame(min, max);
        let vp = camera.view_proj(1.0);
        for corner in 0..8 {
            let p = Vec3::new(
                if corner & 1 == 0 { min.x } else { max.x },
                if corner & 2 == 0 { min.y } else { max.y },
                if corner & 4 == 0 { min.z } else { max.z },
            );
            let ndc = vp.project_point3(p);
            assert!(ndc.abs().max_element() <= 1.0, "{p} projects to {ndc}");
        }
    }

    #[test]
    fn pan_moves_the_target_with_the_cursor() {
        let mut camera = Camera::default();
        let before = camera.ray(Vec2::new(0.4, 0.5), 1.0);
        let grabbed = ray_work_plane(before.0, before.1).unwrap();
        camera.pan(Vec2::new(100.0, 0.0), 1000.0);
        let after = camera.ray(Vec2::new(0.5, 0.5), 1.0);
        let now = ray_work_plane(after.0, after.1).unwrap();
        assert!((grabbed - now).length() < 0.05 * camera.distance, "{grabbed} vs {now}");
    }

    #[test]
    fn ray_triangle_hits_and_misses() {
        let tri = [Vec3::ZERO, Vec3::X, Vec3::Y];
        let down = -Vec3::Z;
        assert_eq!(ray_triangle(Vec3::new(0.2, 0.2, 5.0), down, tri), Some(5.0));
        assert_eq!(ray_triangle(Vec3::new(0.8, 0.8, 5.0), down, tri), None);
        assert_eq!(ray_triangle(Vec3::new(0.2, 0.2, -5.0), down, tri), None);
    }
}
