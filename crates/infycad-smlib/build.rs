//! third_party/usd-brep の SMLib と SM_API を静的ライブラリにし、C シムと一緒にリンクする。

use std::path::{Path, PathBuf};

fn sources(dir: &Path, skip: &[&str]) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| {
            panic!(
                "{}: {e}\nSMLib のソースがありません。`git submodule update --init --depth 1` を実行してください。",
                dir.display()
            )
        })
        .map(|entry| entry.unwrap().path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "cpp"))
        .filter(|p| !skip.contains(&p.file_name().unwrap().to_str().unwrap()))
        .collect();
    files.sort();
    files
}

fn build(name: &str, files: &[PathBuf], includes: &[&Path]) {
    let mut b = cc::Build::new();
    b.cpp(true).std("c++17").warnings(false).flag_if_supported("-w");
    // MSVC: SmBrep.cpp などが大きい / ソースは UTF-8 / Windows.h の min・max マクロを止める。
    // SM_API_EXPORTS が無いと SM_API の宣言が dllimport になり、静的リンクできない。
    b.flag_if_supported("/bigobj").flag_if_supported("/utf-8").flag_if_supported("/EHsc");
    if std::env::var("CARGO_CFG_WINDOWS").is_ok() {
        b.define("NOMINMAX", None).define("SM_API_EXPORTS", None).define("_CRT_SECURE_NO_WARNINGS", None);
    }
    for inc in includes {
        b.include(inc);
    }
    b.files(files).compile(name);
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let source = manifest.join("../../third_party/usd-brep/source");
    let smlib = source.join("SMLib");
    let sm_api = source.join("SM_API");
    println!("cargo:rerun-if-changed=shim/shim.cpp");
    println!("cargo:rerun-if-changed={}", source.display());

    // リンク順: 依存する側を先にコンパイルする（cc は compile した順に link 指定を出す）。
    // SM_API/inc を SMLib/inc より前に置く: 両方に StdAfx.h がある。
    let api_inc = [sm_api.join("inc"), smlib.join("inc")];
    let api_inc: Vec<&Path> = api_inc.iter().map(PathBuf::as_path).collect();
    build("infycad_shim", &[manifest.join("shim/shim.cpp")], &api_inc);
    // SmDllMain.cpp は DLL 用、StdAfx.cpp はプリコンパイル済みヘッダー用。
    build("sm_api", &sources(&sm_api.join("src"), &["StdAfx.cpp"]), &api_inc);
    build(
        "smlib",
        &sources(&smlib.join("src"), &["StdAfx.cpp", "SmDllMain.cpp"]),
        &[&smlib.join("inc")],
    );
}
