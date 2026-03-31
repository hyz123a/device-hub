use std::{env, fs::File, io, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=DEVICE_HUB_FRONTEND_BUNDLE");

    let bundle_path = match env::var_os("DEVICE_HUB_FRONTEND_BUNDLE") {
        Some(path) => validate_bundle_path(PathBuf::from(path)),
        None => discover_workspace_bundle().unwrap_or_else(write_default_bundle),
    };

    println!(
        "cargo:rustc-env=DEVICE_HUB_FRONTEND_BUNDLE={}",
        bundle_path.display()
    );
}

fn validate_bundle_path(path: PathBuf) -> PathBuf {
    if path.exists() {
        println!("cargo:rerun-if-changed={}", path.display());
        path
    } else {
        panic!(
            "DEVICE_HUB_FRONTEND_BUNDLE points to missing file: {}",
            path.display()
        );
    }
}

fn discover_workspace_bundle() -> Option<PathBuf> {
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"));
    let workspace_bundle = manifest_dir
        .parent()
        .expect("server crate should live under workspace root")
        .join("target/frontend-bundle/frontend.bundle.tar");

    workspace_bundle.exists().then(|| {
        println!("cargo:rerun-if-changed={}", workspace_bundle.display());
        workspace_bundle
    })
}

fn write_default_bundle() -> PathBuf {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR not set"));
    let bundle_path = out_dir.join("device-hub-default-frontend.tar");
    let file = File::create(&bundle_path).expect("create default frontend bundle");
    let mut builder = tar::Builder::new(file);

    append_bytes(
        &mut builder,
        "index.html",
        br#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>Device Hub</title>
  </head>
  <body>
    <div id="app">Embedded frontend placeholder</div>
    <script type="module" src="/assets/app.js"></script>
  </body>
</html>
"#,
    );
    append_bytes(
        &mut builder,
        "assets/app.js",
        b"console.log('device-hub embedded frontend placeholder');\n",
    );
    builder.finish().expect("finish default frontend bundle");

    bundle_path
}

fn append_bytes(builder: &mut tar::Builder<File>, path: &str, bytes: &[u8]) {
    let mut header = tar::Header::new_gnu();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder
        .append_data(&mut header, path, io::Cursor::new(bytes))
        .expect("append default frontend asset");
}
