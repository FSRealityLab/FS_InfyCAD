//! コマンドラインに打ち込まれた座標・距離の解釈（AutoCAD の記法）。

/// コマンド実行中の 1 回分の入力。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PointInput {
    /// `x,y` / `x,y,z`: 現在の UCS での絶対座標。
    Absolute([f64; 3]),
    /// `@dx,dy` / `@dx,dy,dz`: 直前の点からの相対座標。
    Relative([f64; 3]),
    /// `@dist<angle`: 直前の点からの極座標（角度は度、X 軸から反時計回り）。
    Polar { distance: f64, angle_deg: f64 },
    /// 数値のみ: カーソル方向への距離（直接距離入力）や半径など。
    Distance(f64),
}

impl PointInput {
    pub fn parse(text: &str) -> Option<PointInput> {
        let text = text.trim();
        let (relative, body) = match text.strip_prefix('@') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        if let Some((dist, angle)) = body.split_once('<') {
            if !relative {
                return None;
            }
            return Some(PointInput::Polar {
                distance: number(dist)?,
                angle_deg: number(angle)?,
            });
        }
        if body.contains(',') {
            let parts: Vec<f64> = body.split(',').map(number).collect::<Option<_>>()?;
            let xyz = match parts[..] {
                [x, y] => [x, y, 0.0],
                [x, y, z] => [x, y, z],
                _ => return None,
            };
            return Some(if relative {
                PointInput::Relative(xyz)
            } else {
                PointInput::Absolute(xyz)
            });
        }
        if relative {
            return None;
        }
        number(body).map(PointInput::Distance)
    }

    /// 直前の点 `last` を基準に絶対座標へ。`Distance` は方向が要るので `None`。
    pub fn resolve(self, last: [f64; 3]) -> Option<[f64; 3]> {
        match self {
            PointInput::Absolute(p) => Some(p),
            PointInput::Relative(d) => Some([last[0] + d[0], last[1] + d[1], last[2] + d[2]]),
            PointInput::Polar { distance, angle_deg } => {
                let a = angle_deg.to_radians();
                Some([last[0] + distance * a.cos(), last[1] + distance * a.sin(), last[2]])
            }
            PointInput::Distance(_) => None,
        }
    }
}

fn number(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_autocad_notations() {
        assert_eq!(PointInput::parse("10,20"), Some(PointInput::Absolute([10.0, 20.0, 0.0])));
        assert_eq!(PointInput::parse("1, 2, 3"), Some(PointInput::Absolute([1.0, 2.0, 3.0])));
        assert_eq!(PointInput::parse("@5,-2.5"), Some(PointInput::Relative([5.0, -2.5, 0.0])));
        assert_eq!(
            PointInput::parse("@100<45"),
            Some(PointInput::Polar { distance: 100.0, angle_deg: 45.0 })
        );
        assert_eq!(PointInput::parse("12.5"), Some(PointInput::Distance(12.5)));
    }

    #[test]
    fn rejects_malformed_input() {
        for bad in ["", "@", "1,2,3,4", "1,", "abc", "100<45", "@5", "inf", "@1<x"] {
            assert_eq!(PointInput::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn resolves_against_last_point() {
        let last = [1.0, 1.0, 2.0];
        assert_eq!(PointInput::Relative([2.0, 3.0, 0.0]).resolve(last), Some([3.0, 4.0, 2.0]));
        let p = PointInput::Polar { distance: 2.0, angle_deg: 90.0 }.resolve(last).unwrap();
        assert!((p[0] - 1.0).abs() < 1e-12 && (p[1] - 3.0).abs() < 1e-12 && p[2] == 2.0);
        assert_eq!(PointInput::Distance(1.0).resolve(last), None);
    }
}
