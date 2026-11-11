//! `SolidKernel` の SMLib 実装（NVIDIA usd-brep の C++ カーネルを C シム経由で呼ぶ）。

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Mutex, MutexGuard, Once};

use infycad_kernel::{KernelError, Mesh, Point3, Result, SolidId, SolidKernel};

unsafe extern "C" {
    fn infy_init();
    fn infy_brep_free(brep: *mut c_void);
    fn infy_box(corner: *const f64, size: *const f64, out: *mut *mut c_void) -> i32;
    fn infy_cylinder(base: *const f64, radius: f64, height: f64, out: *mut *mut c_void) -> i32;
    fn infy_sphere(center: *const f64, radius: f64, out: *mut *mut c_void) -> i32;
    fn infy_boolean(op: i32, a: *const c_void, b: *const c_void, out: *mut *mut c_void) -> i32;
    fn infy_volume(brep: *const c_void, accuracy: f64, out: *mut f64) -> i32;
    fn infy_tessellate(
        brep: *const c_void,
        chord: f64,
        positions: *mut *mut f64,
        normals: *mut *mut f64,
        triangles: *mut usize,
    ) -> i32;
    fn infy_free(p: *mut c_void);
}

/// SMLib はプロセス全体で状態を共有し、並行に呼べない。すべての呼び出しをこのロックで直列化する。
static SMLIB: Mutex<()> = Mutex::new(());
static INIT: Once = Once::new();

fn lock() -> MutexGuard<'static, ()> {
    let guard = SMLIB.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    // SAFETY: ロックを保持しているので、初期化は他の SMLib 呼び出しと重ならない。
    INIT.call_once(|| unsafe { infy_init() });
    guard
}

fn check(op: &str, rc: i32) -> Result<()> {
    match rc {
        0 => Ok(()),
        -1 => Err(KernelError::Failed(format!("{op}: C++ exception"))),
        code => Err(KernelError::Failed(format!("{op}: SmApiStatus {code}"))),
    }
}

/// SMLib の B-rep を所有するカーネル。保持しているソリッドは drop 時に解放する。
#[derive(Default)]
pub struct SmlibKernel {
    solids: HashMap<SolidId, *mut c_void>,
    next_id: u64,
}

impl SmlibKernel {
    pub fn new() -> Self {
        Self::default()
    }

    fn get(&self, id: SolidId) -> Result<*mut c_void> {
        self.solids.get(&id).copied().ok_or(KernelError::InvalidSolid(id))
    }

    fn insert(&mut self, brep: *mut c_void) -> SolidId {
        self.next_id += 1;
        let id = SolidId(self.next_id);
        self.solids.insert(id, brep);
        id
    }

    /// 正の有限値でなければエラー。
    fn positive(op: &str, values: &[f64]) -> Result<()> {
        if values.iter().all(|v| v.is_finite() && *v > 0.0) {
            Ok(())
        } else {
            Err(KernelError::Failed(format!("{op}: dimensions must be positive")))
        }
    }

    fn create(
        &mut self,
        op: &str,
        make: impl FnOnce(*mut *mut c_void) -> i32,
    ) -> Result<SolidId> {
        let mut brep = std::ptr::null_mut();
        let _guard = lock();
        check(op, make(&mut brep))?;
        if brep.is_null() {
            return Err(KernelError::Failed(format!("{op}: no result")));
        }
        Ok(self.insert(brep))
    }

    fn boolean(&mut self, op: &str, code: i32, a: SolidId, b: SolidId) -> Result<SolidId> {
        let (pa, pb) = (self.get(a)?, self.get(b)?);
        // SAFETY: pa と pb はこのカーネルが所有する有効な SmBrep。シムは複製に対して演算する。
        self.create(op, |out| unsafe { infy_boolean(code, pa, pb, out) })
    }
}

impl SolidKernel for SmlibKernel {
    fn make_box(&mut self, corner: Point3, size: Point3) -> Result<SolidId> {
        Self::positive("BOX", &size)?;
        // SAFETY: どちらも 3 要素の配列を指す。
        self.create("BOX", |out| unsafe { infy_box(corner.as_ptr(), size.as_ptr(), out) })
    }

    fn make_cylinder(&mut self, base: Point3, radius: f64, height: f64) -> Result<SolidId> {
        Self::positive("CYLINDER", &[radius, height])?;
        // SAFETY: base は 3 要素の配列を指す。
        self.create("CYLINDER", |out| unsafe { infy_cylinder(base.as_ptr(), radius, height, out) })
    }

    fn make_sphere(&mut self, center: Point3, radius: f64) -> Result<SolidId> {
        Self::positive("SPHERE", &[radius])?;
        // SAFETY: center は 3 要素の配列を指す。
        self.create("SPHERE", |out| unsafe { infy_sphere(center.as_ptr(), radius, out) })
    }

    fn union(&mut self, a: SolidId, b: SolidId) -> Result<SolidId> {
        self.boolean("UNION", 0, a, b)
    }

    fn subtract(&mut self, a: SolidId, b: SolidId) -> Result<SolidId> {
        self.boolean("SUBTRACT", 1, a, b)
    }

    fn intersect(&mut self, a: SolidId, b: SolidId) -> Result<SolidId> {
        self.boolean("INTERSECT", 2, a, b)
    }

    fn volume(&self, solid: SolidId) -> Result<f64> {
        let brep = self.get(solid)?;
        let mut volume = 0.0;
        let _guard = lock();
        // SAFETY: brep はこのカーネルが所有する有効な SmBrep。
        check("VOLUME", unsafe { infy_volume(brep, 1e-7, &mut volume) })?;
        Ok(volume)
    }

    fn tessellate(&self, solid: SolidId, chord_tolerance: f64) -> Result<Mesh> {
        let brep = self.get(solid)?;
        let (mut pos, mut nrm, mut count) = (std::ptr::null_mut(), std::ptr::null_mut(), 0usize);
        let _guard = lock();
        // SAFETY: brep は有効な SmBrep。成功時、シムは三角形ごとに座標 9 個と法線 3 個を
        // malloc した配列で返す（count == 0 のときは null）。
        check("TESSELLATE", unsafe {
            infy_tessellate(brep, chord_tolerance.max(0.0), &mut pos, &mut nrm, &mut count)
        })?;
        let mut mesh = Mesh::default();
        if count > 0 {
            // SAFETY: 上の契約どおりの長さ。コピーしたあと解放する。
            let (pos_s, nrm_s) = unsafe {
                (std::slice::from_raw_parts(pos, count * 9), std::slice::from_raw_parts(nrm, count * 3))
            };
            mesh.positions = pos_s.chunks_exact(3).map(|p| [p[0], p[1], p[2]]).collect();
            // 面法線を 3 頂点に複製する（フラットシェーディング）。
            mesh.normals = nrm_s.chunks_exact(3).flat_map(|n| [[n[0], n[1], n[2]]; 3]).collect();
            mesh.indices = (0..(count * 3) as u32).collect();
            // SAFETY: シムが malloc したポインタで、以後使わない。
            unsafe {
                infy_free(pos.cast());
                infy_free(nrm.cast());
            }
        }
        Ok(mesh)
    }

    fn delete(&mut self, solid: SolidId) -> Result<()> {
        let brep = self.solids.remove(&solid).ok_or(KernelError::InvalidSolid(solid))?;
        let _guard = lock();
        // SAFETY: brep はこのカーネルが所有していた SmBrep で、表から外したので二重解放しない。
        unsafe { infy_brep_free(brep) };
        Ok(())
    }
}

impl Drop for SmlibKernel {
    fn drop(&mut self) {
        let _guard = lock();
        for (_, brep) in self.solids.drain() {
            // SAFETY: 所有している SmBrep を 1 回だけ解放する。
            unsafe { infy_brep_free(brep) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() <= expected.abs() * 1e-5, "{actual} != {expected}");
    }

    #[test]
    fn primitives_have_exact_volumes() {
        let mut k = SmlibKernel::new();
        let b = k.make_box([0.0; 3], [1.0, 2.0, 3.0]).unwrap();
        let c = k.make_cylinder([0.0; 3], 0.5, 2.0).unwrap();
        let s = k.make_sphere([0.0; 3], 1.5).unwrap();
        close(k.volume(b).unwrap(), 6.0);
        close(k.volume(c).unwrap(), PI * 0.25 * 2.0);
        close(k.volume(s).unwrap(), 4.0 / 3.0 * PI * 1.5f64.powi(3));
    }

    #[test]
    fn booleans_keep_their_operands() {
        let mut k = SmlibKernel::new();
        let a = k.make_box([0.0; 3], [2.0, 2.0, 2.0]).unwrap();
        let b = k.make_box([1.0, 1.0, 1.0], [2.0, 2.0, 2.0]).unwrap();
        let (u, d, i) = (k.union(a, b).unwrap(), k.subtract(a, b).unwrap(), k.intersect(a, b).unwrap());
        close(k.volume(u).unwrap(), 15.0);
        close(k.volume(d).unwrap(), 7.0);
        close(k.volume(i).unwrap(), 1.0);
        close(k.volume(a).unwrap(), 8.0);
        close(k.volume(b).unwrap(), 8.0);
    }

    #[test]
    fn drilled_plate() {
        let mut k = SmlibKernel::new();
        let plate = k.make_box([0.0; 3], [4.0, 4.0, 1.0]).unwrap();
        let drill = k.make_cylinder([2.0, 2.0, -1.0], 0.5, 3.0).unwrap();
        let result = k.subtract(plate, drill).unwrap();
        close(k.volume(result).unwrap(), 16.0 - PI * 0.25);
    }

    #[test]
    fn tessellates_a_box_into_twelve_triangles() {
        let mut k = SmlibKernel::new();
        let b = k.make_box([1.0, 2.0, 3.0], [1.0, 1.0, 1.0]).unwrap();
        let mesh = k.tessellate(b, 0.01).unwrap();
        assert_eq!(mesh.indices.len(), 36);
        assert_eq!(mesh.positions.len(), 36);
        assert_eq!(mesh.normals.len(), 36);
        for (p, n) in mesh.positions.iter().zip(&mesh.normals) {
            for axis in 0..3 {
                let min = [1.0, 2.0, 3.0][axis];
                assert!(p[axis] >= min - 1e-9 && p[axis] <= min + 1.0 + 1e-9);
            }
            // 軸に平行な外向き単位法線: 頂点はその法線の向く面の上にある。
            let axis = (0..3).find(|&a| n[a].abs() > 0.5).unwrap();
            close(n[axis].abs(), 1.0);
            let face = [1.0, 2.0, 3.0][axis] + if n[axis] > 0.0 { 1.0 } else { 0.0 };
            assert!((p[axis] - face).abs() < 1e-9, "normal {n:?} at {p:?} is not outward");
        }
    }

    #[test]
    fn tessellates_a_sphere_within_tolerance() {
        let mut k = SmlibKernel::new();
        let s = k.make_sphere([0.0; 3], 1.0).unwrap();
        let mesh = k.tessellate(s, 0.001).unwrap();
        assert!(mesh.positions.len() > 100);
        for p in &mesh.positions {
            let r = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
            assert!((r - 1.0).abs() < 1e-6, "vertex off the sphere: r = {r}");
        }
    }

    #[test]
    fn rejects_bad_input_and_stale_ids() {
        let mut k = SmlibKernel::new();
        assert!(k.make_box([0.0; 3], [1.0, 0.0, 1.0]).is_err());
        assert!(k.make_sphere([0.0; 3], f64::NAN).is_err());
        let b = k.make_box([0.0; 3], [1.0; 3]).unwrap();
        k.delete(b).unwrap();
        assert_eq!(k.volume(b), Err(KernelError::InvalidSolid(b)));
        assert_eq!(k.delete(b), Err(KernelError::InvalidSolid(b)));
    }
}
