//! コマンドラインとクリックを受けて図面を変えていく、1 つの図面のセッション。

use glam::Vec3;
use infycad_core::command::Command;
use infycad_core::input::PointInput;
use infycad_kernel::{Mesh, Point3, SolidId, SolidKernel};

use crate::camera::ray_triangle;

pub struct Solid {
    pub id: SolidId,
    pub mesh: Mesh,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SelectFor {
    Erase,
    Union,
    Intersect,
    SubtractFrom,
}

#[derive(Debug, Clone, PartialEq)]
enum State {
    Idle,
    BoxCorner,
    BoxOther { a: Point3 },
    BoxHeight { a: Point3, b: Point3 },
    CylinderCenter,
    CylinderRadius { center: Point3 },
    CylinderHeight { center: Point3, radius: f64 },
    SphereCenter,
    SphereRadius { center: Point3 },
    Select { purpose: SelectFor },
    SubtractTools { from: Vec<SolidId> },
}

/// 2 点が同じ高さとみなす差。
const FLAT: f64 = 1e-9;

pub struct Session<K: SolidKernel> {
    kernel: K,
    solids: Vec<Solid>,
    /// 選択中のソリッド。コマンド実行中は、そのコマンドの対象。
    selection: Vec<SolidId>,
    state: State,
    log: Vec<String>,
    last_command: Option<Command>,
    last_point: Point3,
    /// 表示内容（ソリッドか選択）が変わるたびに増える。
    version: u64,
}

impl<K: SolidKernel> Session<K> {
    pub fn new(kernel: K) -> Self {
        Self {
            kernel,
            solids: Vec::new(),
            selection: Vec::new(),
            state: State::Idle,
            log: Vec::new(),
            last_command: None,
            last_point: [0.0; 3],
            version: 0,
        }
    }

    pub fn solids(&self) -> &[Solid] {
        &self.solids
    }

    pub fn selection(&self) -> &[SolidId] {
        &self.selection
    }

    pub fn log(&self) -> &[String] {
        &self.log
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn is_idle(&self) -> bool {
        self.state == State::Idle
    }

    /// クリックを点として受け取る状態か（そうでなければオブジェクト選択）。
    pub fn wants_point(&self) -> bool {
        matches!(
            self.state,
            State::BoxCorner
                | State::BoxOther { .. }
                | State::CylinderCenter
                | State::CylinderRadius { .. }
                | State::SphereCenter
                | State::SphereRadius { .. }
        )
    }

    pub fn prompt(&self) -> &'static str {
        match self.state {
            State::Idle => "Command:",
            State::BoxCorner => "BOX Specify first corner:",
            State::BoxOther { .. } => "BOX Specify other corner:",
            State::BoxHeight { .. } => "BOX Specify height:",
            State::CylinderCenter => "CYLINDER Specify center point of base:",
            State::CylinderRadius { .. } => "CYLINDER Specify base radius:",
            State::CylinderHeight { .. } => "CYLINDER Specify height:",
            State::SphereCenter => "SPHERE Specify center point:",
            State::SphereRadius { .. } => "SPHERE Specify radius:",
            State::Select { purpose: SelectFor::SubtractFrom } => {
                "SUBTRACT Select solids to subtract from (Enter when done):"
            }
            State::Select { .. } => "Select objects (Enter when done):",
            State::SubtractTools { .. } => "SUBTRACT Select solids to subtract (Enter when done):",
        }
    }

    /// 全ソリッドの境界ボックス。
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut points = self.solids.iter().flat_map(|s| &s.mesh.positions).map(to_vec3);
        let first = points.next()?;
        Some(points.fold((first, first), |(min, max), p| (min.min(p), max.max(p))))
    }

    /// 視線に最初に当たるソリッド。
    pub fn pick(&self, origin: Vec3, dir: Vec3) -> Option<SolidId> {
        let mut best: Option<(f32, SolidId)> = None;
        for solid in &self.solids {
            for tri in solid.mesh.indices.chunks_exact(3) {
                let corners = [0, 1, 2].map(|i| to_vec3(&solid.mesh.positions[tri[i] as usize]));
                if let Some(t) = ray_triangle(origin, dir, corners)
                    && best.is_none_or(|(nearest, _)| t < nearest) {
                        best = Some((t, solid.id));
                    }
            }
        }
        best.map(|(_, id)| id)
    }

    /// Esc: 実行中のコマンドを中止する。待機中なら選択を解除する。
    pub fn cancel(&mut self) {
        if self.state != State::Idle {
            self.say("*Cancel*");
        }
        self.state = State::Idle;
        self.clear_selection();
    }

    /// コマンドラインで確定された 1 行。空文字列は Enter だけを押したことを表す。
    pub fn submit_text(&mut self, text: &str) {
        let text = text.trim();
        self.say(format!("{} {text}", self.prompt()));
        match self.state.clone() {
            State::Idle => {
                let command = if text.is_empty() { self.last_command } else { Command::lookup(text) };
                match command {
                    Some(command) => self.start(command),
                    None if text.is_empty() => {}
                    None => self.say(format!("Unknown command \"{text}\".")),
                }
            }
            State::Select { purpose } => {
                if text.is_empty() {
                    self.finish_selection(purpose);
                } else if text.eq_ignore_ascii_case("ALL") {
                    self.selection = self.solids.iter().map(|s| s.id).collect();
                    self.version += 1;
                } else {
                    self.say("Click solids to select them, type ALL, or press Enter to finish.");
                }
            }
            State::SubtractTools { from } => {
                if text.is_empty() {
                    self.finish_subtract(from);
                } else {
                    self.say("Click solids to select them, or press Enter to finish.");
                }
            }
            _ => match PointInput::parse(text) {
                Some(PointInput::Distance(value)) => self.give_distance(value),
                Some(input) => match input.resolve(self.last_point) {
                    Some(point) => self.give_point(point),
                    None => self.say("Invalid point."),
                },
                None => self.say("Invalid input. Enter a point (x,y,z or @dx,dy,dz) or a number."),
            },
        }
    }

    /// ビューポートで作業平面上の点がクリックされた。
    pub fn click_point(&mut self, point: Point3) {
        if self.wants_point() {
            self.give_point(point);
        }
    }

    /// ビューポートでソリッド（または何もない所）がクリックされた。
    pub fn click_solid(&mut self, hit: Option<SolidId>) {
        match (&self.state, hit) {
            (State::Idle | State::Select { .. } | State::SubtractTools { .. }, Some(id)) => {
                // SUBTRACT の 2 組目では、引かれる側は選べない。
                if matches!(&self.state, State::SubtractTools { from } if from.contains(&id)) {
                    return;
                }
                match self.selection.iter().position(|s| *s == id) {
                    Some(index) => {
                        self.selection.remove(index);
                    }
                    None => self.selection.push(id),
                }
                self.version += 1;
            }
            (State::Idle, None) => self.clear_selection(),
            _ => {}
        }
    }

    fn start(&mut self, command: Command) {
        self.last_command = Some(command);
        // 先に選んでからコマンドを打った場合は、その選択を対象にする。
        let preselected = !self.selection.is_empty();
        let select = |session: &mut Self, purpose| {
            session.state = State::Select { purpose };
            if preselected {
                session.finish_selection(purpose);
            }
        };
        match command {
            Command::Box => self.begin(State::BoxCorner),
            Command::Cylinder => self.begin(State::CylinderCenter),
            Command::Sphere => self.begin(State::SphereCenter),
            Command::Erase => select(self, SelectFor::Erase),
            Command::Union => select(self, SelectFor::Union),
            Command::Intersect => select(self, SelectFor::Intersect),
            Command::Subtract => select(self, SelectFor::SubtractFrom),
            other => self.say(format!("{} is not implemented yet.", other.name())),
        }
    }

    fn begin(&mut self, state: State) {
        self.clear_selection();
        self.state = state;
    }

    fn give_point(&mut self, p: Point3) {
        match self.state.clone() {
            State::BoxCorner => {
                self.last_point = p;
                self.state = State::BoxOther { a: p };
            }
            State::BoxOther { a } => {
                if (p[0] - a[0]).abs() < FLAT || (p[1] - a[1]).abs() < FLAT {
                    return self.say("The corners must differ in both X and Y.");
                }
                self.last_point = p;
                if (p[2] - a[2]).abs() < FLAT {
                    self.state = State::BoxHeight { a, b: p };
                } else {
                    self.make_box(a, p);
                }
            }
            State::CylinderCenter => {
                self.last_point = p;
                self.state = State::CylinderRadius { center: p };
            }
            State::SphereCenter => {
                self.last_point = p;
                self.state = State::SphereRadius { center: p };
            }
            // 半径は中心からの距離でも指定できる。
            State::CylinderRadius { center } | State::SphereRadius { center } => {
                self.give_distance((to_vec3(&p) - to_vec3(&center)).length() as f64);
            }
            _ => self.say("A number is required."),
        }
    }

    fn give_distance(&mut self, value: f64) {
        if !matches!(self.state, State::BoxHeight { .. }) && value <= 0.0 {
            return self.say("Requires a positive value.");
        }
        match self.state.clone() {
            State::BoxHeight { a, b } => {
                if value == 0.0 {
                    return self.say("Requires a nonzero value.");
                }
                self.make_box(a, [b[0], b[1], a[2] + value]);
            }
            State::CylinderRadius { center } => self.state = State::CylinderHeight { center, radius: value },
            State::CylinderHeight { center, radius } => {
                let result = self.kernel.make_cylinder(center, radius, value);
                self.finish_with("CYLINDER", result, &[]);
            }
            State::SphereRadius { center } => {
                let result = self.kernel.make_sphere(center, value);
                self.finish_with("SPHERE", result, &[]);
            }
            _ => self.say("A point is required."),
        }
    }

    fn make_box(&mut self, a: Point3, b: Point3) {
        let corner = [a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2])];
        let size = [(a[0] - b[0]).abs(), (a[1] - b[1]).abs(), (a[2] - b[2]).abs()];
        let result = self.kernel.make_box(corner, size);
        self.finish_with("BOX", result, &[]);
    }

    fn finish_selection(&mut self, purpose: SelectFor) {
        let picked = self.selection.clone();
        match purpose {
            SelectFor::Erase => {
                let count = picked.len();
                self.remove(&picked);
                self.end(format!("{count} erased."));
            }
            SelectFor::SubtractFrom if picked.is_empty() => self.end("Nothing selected."),
            SelectFor::SubtractFrom => {
                self.clear_selection();
                self.state = State::SubtractTools { from: picked };
            }
            SelectFor::Union | SelectFor::Intersect if picked.len() < 2 => {
                self.end("At least 2 solids must be selected.")
            }
            SelectFor::Union => {
                let result = self.fold(&picked, K::union);
                self.finish_with("UNION", result, &picked);
            }
            SelectFor::Intersect => {
                let result = self.fold(&picked, K::intersect);
                self.finish_with("INTERSECT", result, &picked);
            }
        }
    }

    fn finish_subtract(&mut self, from: Vec<SolidId>) {
        let tools = self.selection.clone();
        if tools.is_empty() {
            return self.end("Nothing selected.");
        }
        // AutoCAD と同じく、各組を先に和にしてから引く。
        let result = self.fold(&from, K::union).and_then(|base| {
            let tool = self.fold(&tools, K::union);
            let result = tool.and_then(|tool| {
                let result = self.kernel.subtract(base, tool);
                self.discard_temporary(tool, &tools);
                result
            });
            self.discard_temporary(base, &from);
            result
        });
        let consumed: Vec<SolidId> = from.into_iter().chain(tools).collect();
        self.finish_with("SUBTRACT", result, &consumed);
    }

    /// `ids` を左から `op` でまとめる。途中結果は解放する。1 個ならそのまま返す。
    fn fold(
        &mut self,
        ids: &[SolidId],
        op: fn(&mut K, SolidId, SolidId) -> infycad_kernel::Result<SolidId>,
    ) -> infycad_kernel::Result<SolidId> {
        let mut acc = ids[0];
        for &next in &ids[1..] {
            let result = op(&mut self.kernel, acc, next);
            self.discard_temporary(acc, ids);
            acc = result?;
        }
        Ok(acc)
    }

    /// `id` が図面のソリッド (`originals`) でなければ、カーネルから解放する。
    fn discard_temporary(&mut self, id: SolidId, originals: &[SolidId]) {
        if !originals.contains(&id) {
            let _ = self.kernel.delete(id);
        }
    }

    /// 成功なら `consumed` を図面から消して結果を加える。失敗なら図面はそのまま。
    fn finish_with(&mut self, name: &str, result: infycad_kernel::Result<SolidId>, consumed: &[SolidId]) {
        let meshed = result.and_then(|id| {
            // 元のソリッドがそのまま結果になった場合（1 個だけの和など）は作り直さない。
            if consumed.contains(&id) {
                return Ok(None);
            }
            match self.kernel.tessellate(id, 0.0) {
                Ok(mesh) => Ok(Some(Solid { id, mesh })),
                Err(error) => {
                    let _ = self.kernel.delete(id);
                    Err(error)
                }
            }
        });
        match meshed {
            Ok(Some(solid)) => {
                self.remove(consumed);
                self.solids.push(solid);
                self.end(format!("{name} done."));
            }
            Ok(None) => self.end(format!("{name} done.")),
            Err(error) => self.end(format!("{name} failed: {error}")),
        }
    }

    fn remove(&mut self, ids: &[SolidId]) {
        for id in ids {
            let _ = self.kernel.delete(*id);
        }
        self.solids.retain(|s| !ids.contains(&s.id));
        self.version += 1;
    }

    fn end(&mut self, message: impl Into<String>) {
        self.say(message);
        self.state = State::Idle;
        self.clear_selection();
    }

    fn clear_selection(&mut self) {
        if !self.selection.is_empty() {
            self.selection.clear();
            self.version += 1;
        }
    }

    fn say(&mut self, message: impl Into<String>) {
        self.log.push(message.into());
    }
}

fn to_vec3(p: &Point3) -> Vec3 {
    Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use infycad_smlib::SmlibKernel;
    use std::f64::consts::PI;

    fn run(lines: &[&str]) -> Session<SmlibKernel> {
        let mut session = Session::new(SmlibKernel::new());
        for line in lines {
            session.submit_text(line);
        }
        session
    }

    fn volumes(session: &Session<SmlibKernel>) -> Vec<f64> {
        session.solids.iter().map(|s| session.kernel.volume(s.id).unwrap()).collect()
    }

    fn close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() <= expected.abs() * 1e-5, "{actual} != {expected}");
    }

    #[test]
    fn box_from_two_corners_and_a_height() {
        let s = run(&["box", "10,10", "@-4,5", "-3"]);
        assert!(s.is_idle());
        close(volumes(&s)[0], 60.0);
        let (min, max) = s.bounds().unwrap();
        assert!((min - Vec3::new(6.0, 10.0, -3.0)).length() < 1e-5, "{min}");
        assert!((max - Vec3::new(10.0, 15.0, 0.0)).length() < 1e-5, "{max}");
    }

    #[test]
    fn box_from_two_3d_corners_needs_no_height() {
        let s = run(&["BOX", "0,0,0", "2,3,4"]);
        assert!(s.is_idle());
        close(volumes(&s)[0], 24.0);
    }

    #[test]
    fn cylinder_and_sphere() {
        let s = run(&["CYL", "0,0", "2", "5", "SPHERE", "10,0,0", "@3<90"]);
        let v = volumes(&s);
        close(v[0], PI * 4.0 * 5.0);
        close(v[1], 4.0 / 3.0 * PI * 27.0);
    }

    #[test]
    fn bad_input_keeps_the_prompt() {
        let mut s = run(&["BOX", "0,0", "0,5"]);
        assert_eq!(s.prompt(), "BOX Specify other corner:");
        s.submit_text("abc");
        s.submit_text("4,5");
        s.submit_text("0");
        assert_eq!(s.prompt(), "BOX Specify height:");
        s.cancel();
        assert!(s.is_idle() && s.solids().is_empty());
    }

    #[test]
    fn enter_repeats_the_last_command() {
        let s = run(&["SPHERE", "0,0", "1", ""]);
        assert_eq!(s.prompt(), "SPHERE Specify center point:");
    }

    #[test]
    fn subtract_drills_a_hole_and_replaces_both_solids() {
        let mut s = run(&["BOX", "0,0,0", "4,4,1", "CYL", "2,2,-1", "0.5", "3", "SUBTRACT"]);
        let (plate, drill) = (s.solids()[0].id, s.solids()[1].id);
        s.click_solid(Some(plate));
        s.submit_text("");
        s.click_solid(Some(plate)); // 引かれる側は 2 組目に入らない
        assert!(s.selection().is_empty());
        s.click_solid(Some(drill));
        s.submit_text("");
        assert!(s.is_idle() && s.selection().is_empty());
        let v = volumes(&s);
        assert_eq!(v.len(), 1);
        close(v[0], 16.0 - PI * 0.25);
    }

    #[test]
    fn union_uses_a_preselection() {
        let mut s = run(&["BOX", "0,0,0", "2,2,2", "BOX", "1,1,1", "3,3,3"]);
        let ids: Vec<SolidId> = s.solids().iter().map(|x| x.id).collect();
        s.click_solid(Some(ids[0]));
        s.click_solid(Some(ids[1]));
        s.submit_text("UNI");
        assert!(s.is_idle());
        let v = volumes(&s);
        assert_eq!(v.len(), 1);
        close(v[0], 15.0);
    }

    #[test]
    fn intersect_and_erase_with_all() {
        let mut s = run(&["BOX", "0,0,0", "2,2,2", "BOX", "1,1,1", "3,3,3", "IN", "ALL", ""]);
        close(volumes(&s)[0], 1.0);
        for line in ["E", "all", ""] {
            s.submit_text(line);
        }
        assert!(s.solids().is_empty() && s.is_idle());
    }

    #[test]
    fn union_needs_two_solids() {
        let s = run(&["BOX", "0,0,0", "1,1,1", "UNION", "ALL", ""]);
        assert!(s.is_idle());
        assert_eq!(s.solids().len(), 1);
        assert!(s.log().last().unwrap().contains("At least 2"));
    }

    #[test]
    fn picks_the_nearest_solid() {
        let s = run(&["BOX", "0,0,0", "1,1,1", "BOX", "0,0,5", "1,1,6"]);
        let down = Vec3::new(0.0, 0.0, -1.0);
        assert_eq!(s.pick(Vec3::new(0.5, 0.5, 10.0), down), Some(s.solids()[1].id));
        assert_eq!(s.pick(Vec3::new(0.5, 0.5, 3.0), down), Some(s.solids()[0].id));
        assert_eq!(s.pick(Vec3::new(5.0, 5.0, 10.0), down), None);
    }
}
