use crate::Manifest;
#[cfg(not(target_os = "windows"))]
use std::process::{Command, Stdio};
use std::{
    fs, io,
    path::{Path, PathBuf},
};
/// Makes `@sidedoor/sdk` importable from the plugin, and gives editors a
/// tsconfig, without the plugin having to install anything.
pub(crate) fn prepare(dir: &Path, sdk: &Path) -> io::Result<()> {
    let scope = dir.join("node_modules").join("@sidedoor");
    let link = scope.join("sdk");
    #[cfg(unix)]
    match fs::symlink_metadata(&link) {
        // A link into an app that moved or was deleted. A link to another
        // working SDK, such as a checkout of this repo, is left alone.
        Ok(metadata) if metadata.is_symlink() && !link.join("package.json").exists() => {
            fs::remove_file(&link)?;
            std::os::unix::fs::symlink(sdk, &link)?;
        }
        // Linked already, or installed by the plugin itself.
        Ok(_) => {}
        Err(_) => {
            fs::create_dir_all(&scope)?;
            std::os::unix::fs::symlink(sdk, &link)?;
        }
    }
    #[cfg(target_os = "windows")]
    install_sdk_copy(sdk, &link)?;
    let tsconfig = dir.join("tsconfig.json");
    if !tsconfig.exists() {
        fs::write(&tsconfig, TSCONFIG)?;
    }
    Ok(())
}

// Windows directory symlinks normally require Developer Mode or administrator
// privileges. Keep a host-owned SDK copy instead, refreshing it on every load.
// A package installed by the plugin author is never overwritten.
#[cfg(any(target_os = "windows", test))]
pub(crate) fn install_sdk_copy(sdk: &Path, destination: &Path) -> io::Result<()> {
    const MARKER: &str = ".sidedoor-sdk";
    if destination.exists() && !destination.join(MARKER).is_file() {
        return Ok(());
    }
    fn copy(source: &Path, destination: &Path) -> io::Result<()> {
        fs::create_dir_all(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            let target = destination.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                copy(&entry.path(), &target)?;
            } else if entry.file_type()?.is_file() {
                fs::copy(entry.path(), target)?;
            }
        }
        Ok(())
    }
    fs::create_dir_all(destination)?;
    // Mark ownership before copying so an interrupted install can be repaired.
    fs::write(destination.join(MARKER), b"Managed by Sidedoor\n")?;
    copy(&sdk.join("src"), &destination.join("src"))?;
    fs::copy(sdk.join("package.json"), destination.join("package.json"))?;
    Ok(())
}

pub const TSCONFIG: &str = r#"{
  "compilerOptions": {
    "target": "ESNext",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "jsx": "react-jsx",
    "jsxImportSource": "@sidedoor/sdk",
    "strict": true,
    "noEmit": true,
    "skipLibCheck": true
  }
}
"#;

/// A folder name from a display name: "My Widget!" → "my-widget".
pub(crate) fn slug(name: &str) -> String {
    let mut slug = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        "widget".into()
    } else {
        slug.into()
    }
}

/// Creates a working plugin named `name` in `dir`, ready to edit.
pub fn create(dir: &Path, name: &str) -> io::Result<Manifest> {
    let name = match name.trim() {
        "" => "My Widget",
        name => name,
    };
    let base = slug(name);
    let mut folder = dir.join(&base);
    let mut n = 2;
    while folder.exists() {
        folder = dir.join(format!("{base}-{n}"));
        n += 1;
    }
    fs::create_dir_all(&folder)?;
    fs::write(folder.join("tsconfig.json"), TSCONFIG)?;
    let title = serde_json::to_string(name).map_err(io::Error::other)?;
    fs::write(folder.join("index.tsx"), TEMPLATE.replace("TITLE", &title))?;
    Manifest::read(&folder).ok_or_else(|| io::Error::other("the new plugin can't be read"))
}

const TEMPLATE: &str = r#"import { Button, Card, Text, definePlugin, useState } from "@sidedoor/sdk";

// Save this file and the card reloads. The @sidedoor/sdk README lists
// every element, component and style prop.
export default definePlugin({
  name: TITLE,
  icon: "sparkles",
  settings: {
    greeting: { title: "Greeting", type: "text", default: "Hello" },
  },

  card({ settings }) {
    const [count, setCount] = useState(0);
    return (
      <Card title={TITLE} accessory={`${count} clicks`}>
        <Text secondary>{`${settings.greeting}! Edit index.tsx to make this yours.`}</Text>
        <div flex gap={8}>
          <Button variant="primary" label="Click me" on_click={() => setCount(count + 1)} />
          <Button label="Reset" on_click={() => setCount(0)} />
        </div>
      </Card>
    );
  },
});
"#;

/// The SDK shipped inside the app bundle, or the repository's when run
/// with `cargo run`.
pub fn sdk_dir() -> PathBuf {
    platform::bundled_resource("sdk")
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../sdk"))
}

/// Finds Bun. Apps opened from Finder don't get the shell's `PATH`, so ask
/// a login shell first, then try the usual install locations.
pub fn find_bun() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("SIDEDOOR_BUN") {
        return Some(PathBuf::from(path));
    }
    if let Some(bundled) = std::env::current_exe()
        .ok()
        .and_then(|exe| {
            Some(
                exe.parent()?
                    .join(if cfg!(windows) { "bun.exe" } else { "bun" }),
            )
        })
        .filter(|path| path.is_file())
    {
        return Some(bundled);
    }
    #[cfg(not(target_os = "windows"))]
    let shell = if cfg!(target_os = "macos") {
        "/bin/zsh".to_string()
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())
    };
    #[cfg(not(target_os = "windows"))]
    let from_shell = Command::new(shell)
        .args(["-lc", "command -v bun"])
        .stderr(Stdio::null())
        .output()
        .ok()
        .and_then(|output| {
            let path = String::from_utf8(output.stdout).ok()?;
            let path = PathBuf::from(path.trim());
            path.is_file().then_some(path)
        });
    #[cfg(not(target_os = "windows"))]
    if from_shell.is_some() {
        return from_shell;
    }
    let name = if cfg!(windows) { "bun.exe" } else { "bun" };
    if let Some(path) = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(name))
            .find(|path| path.is_file())
    }) {
        return Some(path);
    }
    let home = PathBuf::from(
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).unwrap_or_default(),
    );
    [
        home.join(".bun/bin").join(name),
        PathBuf::from("/opt/homebrew/bin/bun"),
        PathBuf::from("/usr/local/bin/bun"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}
