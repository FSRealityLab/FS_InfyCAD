# FS_InfyCAD

NVIDIA が公開した SMLib カーネル（[usd-brep](https://github.com/NVIDIA-Omniverse/usd-brep)、Apache-2.0）を使う、AutoCAD に近い操作感の 2D/3D CAD。Rust で実装する。

## 方針

- **操作感は AutoCAD に合わせる**: リボン、下部のコマンドライン、短縮コマンド（`L` `C` `CO` `EXT` …）、`@dx,dy` / `@距離<角度` の座標入力、オブジェクトスナップ、グリップ、UCS、ViewCube。
- **カーネルは trait の裏に置く**: アプリは `infycad-kernel::SolidKernel` だけを使い、SMLib の型は外に出さない。
- **参考実装**: [MiniCAD](https://github.com/CHMOSE023/MiniCAD)（MIT、C++）。同じ SMLib を使う AutoCAD 風 CAD。構成と機能の参考にするだけで、コードは取り込まない。

## 構成

| クレート | 役割 |
|---|---|
| `crates/infycad-core` | UI 非依存の中核。コマンド表と短縮名、コマンドライン入力の解釈 |
| `crates/infycad-kernel` | ソリッドカーネルの trait（BOX / CYLINDER / SPHERE / UNION / SUBTRACT / INTERSECT / 三角形分割） |

未着手: カーネルのバックエンド、ドキュメント（図形・画層・元に戻す）、アプリ（ウィンドウと 3D 表示）。

## 開発

```bash
cargo test
```
