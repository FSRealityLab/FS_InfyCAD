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
| `crates/infycad-kernel` | ソリッドカーネルの trait（BOX / CYLINDER / SPHERE / UNION / SUBTRACT / INTERSECT / 体積 / 三角形分割） |
| `crates/infycad-smlib` | その trait の SMLib 実装。NVIDIA の C++ カーネルを静的にビルドし、C シム（`shim/shim.cpp`）経由で呼ぶ |
| `third_party/usd-brep` | NVIDIA usd-brep 1.0.1 の git サブモジュール（Apache-2.0、無改変）。使うのは `source/SMLib` と `source/SM_API` だけ |

未着手: ドキュメント（図形・画層・元に戻す）、アプリ（ウィンドウと 3D 表示）。

## 開発

C++17 コンパイラ（GCC か Clang）が必要。SMLib の初回ビルドは 1 分ほどかかる。

```bash
git submodule update --init --depth 1
```

```bash
cargo test
```

SMLib はプロセス全体で状態を共有し並行に呼べないため、`infycad-smlib` がすべての呼び出しを 1 つのロックで直列化している。
