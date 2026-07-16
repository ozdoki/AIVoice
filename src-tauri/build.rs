fn main() {
    tauri_build::build();

    println!("cargo:rerun-if-changed=windows-test-manifest.xml");
    println!("cargo:rerun-if-changed=windows-test-manifest.rc");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // link directiveは出さず、test用resource objectだけを生成する。
        embed_resource::compile_for(
            "windows-test-manifest.rc",
            std::iter::empty::<&str>(),
            embed_resource::NONE,
        )
        .manifest_required()
        .expect("failed to embed Common-Controls v6 manifest into Windows test executables");

        let out_dir =
            std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR is not set"));
        let raw_resource = if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
            out_dir.join("windows-test-manifest.lib")
        } else {
            out_dir.join("libwindows-test-manifest.a")
        };
        // embed-resourceのMSVC出力は.lib拡張子だがraw COFF objectなので、
        // whole-archiveでunit-test harnessへ強制リンクできる実archiveへ包む。
        cc::Build::new()
            .cargo_metadata(false)
            .object(raw_resource)
            .compile("windows_test_manifest_archive");
        println!("cargo:rustc-link-search=native={}", out_dir.display());
    }
}
