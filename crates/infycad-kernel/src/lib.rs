//! ソリッドカーネルの抽象。アプリ側はこの trait だけを使い、
//! SMLib (NVIDIA usd-brep) の型はバックエンドの外に出さない。

pub type Point3 = [f64; 3];

/// バックエンドが保持する B-rep ソリッドのハンドル。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SolidId(pub u64);

/// 表示用の三角形メッシュ。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    pub positions: Vec<Point3>,
    pub normals: Vec<Point3>,
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum KernelError {
    /// このバックエンドでは未実装。
    Unsupported(&'static str),
    InvalidSolid(SolidId),
    /// カーネルが操作に失敗した（理由つき）。
    Failed(String),
}

impl std::fmt::Display for KernelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(op) => write!(f, "unsupported operation: {op}"),
            Self::InvalidSolid(id) => write!(f, "invalid solid: {}", id.0),
            Self::Failed(msg) => write!(f, "kernel error: {msg}"),
        }
    }
}

impl std::error::Error for KernelError {}

pub type Result<T> = std::result::Result<T, KernelError>;

/// AutoCAD の 3D ソリッドコマンドに対応する操作。
pub trait SolidKernel {
    /// BOX: `corner` を最小点とする直方体。
    fn make_box(&mut self, corner: Point3, size: Point3) -> Result<SolidId>;
    /// CYLINDER: 底面中心 `base`、+Z 方向の円柱。
    fn make_cylinder(&mut self, base: Point3, radius: f64, height: f64) -> Result<SolidId>;
    /// SPHERE
    fn make_sphere(&mut self, center: Point3, radius: f64) -> Result<SolidId>;
    /// UNION
    fn union(&mut self, a: SolidId, b: SolidId) -> Result<SolidId>;
    /// SUBTRACT: `a` から `b` を引く。
    fn subtract(&mut self, a: SolidId, b: SolidId) -> Result<SolidId>;
    /// INTERSECT
    fn intersect(&mut self, a: SolidId, b: SolidId) -> Result<SolidId>;
    /// 体積（相対精度 1e-7 程度）。
    fn volume(&self, solid: SolidId) -> Result<f64>;
    /// 表示用に三角形分割する。フラットシェーディング用に、三角形ごとに頂点と面法線を持つ。
    /// `chord_tolerance` は弦誤差の上限。
    fn tessellate(&self, solid: SolidId, chord_tolerance: f64) -> Result<Mesh>;
    fn delete(&mut self, solid: SolidId) -> Result<()>;
}
