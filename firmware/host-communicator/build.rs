//! Lets the linker find SDL2 where Homebrew installs it, which is not on the
//! default search path on Apple silicon.

fn main() {
    if std::env::var_os("CARGO_FEATURE_WINDOW").is_some() {
        for dir in ["/opt/homebrew/lib", "/usr/local/lib"] {
            if std::path::Path::new(dir).join("libSDL2.dylib").exists() {
                println!("cargo:rustc-link-search=native={dir}");
            }
        }
    }
}
