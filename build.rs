fn main() {
    println!("cargo:rerun-if-changed=native/video.cpp");
    println!("cargo:rerun-if-changed=native/include");
    cc::Build::new()
        .cpp(true)
        .include("native/include")
        .file("native/video.cpp")
        .compile("svbb_video");
    println!("cargo:rerun-if-changed=assets/windows.rc");
    println!("cargo:rerun-if-changed=assets/svbb.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let root = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let kit = std::path::PathBuf::from(
        std::env::var_os("ProgramFiles(x86)").unwrap_or_else(|| "C:/Program Files (x86)".into()),
    )
    .join("Windows Kits/10/bin");
    let mut compilers: Vec<_> = std::fs::read_dir(kit)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path().join("x64/rc.exe"))
        .filter(|p| p.is_file())
        .collect();
    compilers.sort();
    let compiler = std::env::var_os("RC")
        .map(std::path::PathBuf::from)
        .or_else(|| compilers.pop());
    if let Some(compiler) = compiler {
        let output =
            std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("svbb.res");
        let status = std::process::Command::new(compiler)
            .current_dir(root)
            .arg("/nologo")
            .arg("/fo")
            .arg(&output)
            .arg("assets/windows.rc")
            .status()
            .expect("Cannot run Windows resource compiler");
        assert!(status.success(), "Application resource compilation failed");
        println!("cargo:rustc-link-arg-bin=svbb={}", output.display());
    } else {
        println!("cargo:warning=Windows SDK rc.exe not found; executable icon omitted");
    }
}
