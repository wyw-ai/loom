fn main() {
    println!("cargo:rerun-if-env-changed=LOOM_GITHUB_CLIENT_ID");
    println!("cargo:rerun-if-env-changed=LOOM_GOOGLE_CLIENT_ID");
    tauri_build::build()
}
