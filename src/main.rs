#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
fn main() {
    if let Err(e) = run() {
        eprintln!("SVBB: {e:#}");
        let _ = std::fs::write(
            std::env::temp_dir().join("svbb-launch-error.txt"),
            format!("{e:#}"),
        );
    }
}
fn run() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let arg = args.next();
    if arg.as_deref() == Some("--render") {
        let path = std::path::PathBuf::from(
            args.next()
                .ok_or_else(|| anyhow::anyhow!("--render PROJECT OUTPUT"))?,
        );
        let output = std::path::PathBuf::from(
            args.next()
                .ok_or_else(|| anyhow::anyhow!("--render PROJECT OUTPUT"))?,
        );
        let p = svbb::persistence::load(&path)?;
        let rt = svbb::media::Runtime::discover()?;
        svbb::render::export(
            &rt,
            &p,
            &svbb::render::RenderSettings::project(&p),
            &output,
            &std::sync::atomic::AtomicBool::new(false),
            |n| eprintln!("{:.0}%", n * 100.),
        )?;
        return Ok(());
    }
    svbb::ui::run(arg.map(std::path::PathBuf::from))
}
