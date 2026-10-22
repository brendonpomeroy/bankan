use std::path::Path;

// rust-embed needs web/dist to exist at compile time. If the web app hasn't
// been built yet, drop in a placeholder so `cargo build` still works.
fn main() {
    let dist = Path::new("web/dist");
    if !dist.join("index.html").exists() {
        std::fs::create_dir_all(dist).expect("create web/dist");
        std::fs::write(
            dist.join("index.html"),
            "<!doctype html><title>BanKan</title><p>The web UI has not been built. Run <code>make web</code> (or <code>npm --prefix web run build</code>) and rebuild.</p>",
        )
        .expect("write placeholder index.html");
    }
    println!("cargo:rerun-if-changed=web/dist");
}
