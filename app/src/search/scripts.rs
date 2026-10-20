//! Extensions are script commands: any executable file in the Extensions
//! folder with metadata comments at the top. The common script-command
//! format is read too (`@raycast.title` alongside `@glint.title`):
//!
//! ```sh
//! #!/bin/bash
//! # @glint.title Say Hello
//! # @glint.mode fullOutput        (silent | compact | fullOutput | inline | list)
//! # @glint.icon hand.wave          (an SF Symbol name, or an image path)
//! # @glint.description Prints a greeting
//! # @glint.argument1 { "type": "text", "placeholder": "Name" }
//! echo "Hello $1"
//! ```
//!
//! `list` mode is Glint's own: the script gets the search text as `$1` and
//! prints JSON items, which Glint shows as a searchable list:
//! `[{"title": "...", "subtitle": "...", "icon": "flame", "accessory": "...",
//!    "copy": "...", "open": "https://...", "paste": "..."}]`

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::paths;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Runs, then the launcher closes.
    Silent,
    /// The last line of output shows as a toast.
    Compact,
    /// Output fills the launcher.
    FullOutput,
    /// First line of output becomes the item's subtitle in search.
    Inline,
    /// JSON items, searchable.
    List,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Argument {
    pub placeholder: String,
    pub optional: bool,
    pub secure: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Script {
    pub path: PathBuf,
    pub title: String,
    pub mode: Mode,
    pub icon: Option<String>,
    pub description: Option<String>,
    pub package: Option<String>,
    pub keywords: Vec<String>,
    pub arguments: Vec<Argument>,
    pub needs_confirmation: bool,
    pub current_directory: Option<PathBuf>,
}

impl Script {
    pub fn id(&self) -> String {
        format!("script:{}", self.path.display())
    }
}

/// Every script in the Extensions folder (and one level of subfolders, so a
/// cloned script-commands repo works).
pub fn scan() -> Vec<Script> {
    let mut out = Vec::new();
    visit(&paths::extensions_dir(), true, &mut out);
    out
}

fn visit(dir: &Path, recurse: bool, out: &mut Vec<Script>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.')) {
            continue;
        }
        if p.is_dir() {
            if recurse {
                visit(&p, false, out);
            }
        } else if let Ok(text) = read_head(&p)
            && let Some(s) = parse(&p, &text)
        {
            out.push(s);
        }
    }
}

/// The first 8 KB, where the metadata lives.
fn read_head(p: &Path) -> std::io::Result<String> {
    let mut buf = vec![0u8; 8192];
    let n = std::fs::File::open(p)?.read(&mut buf)?;
    buf.truncate(n);
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Reads the `@raycast.*` / `@glint.*` metadata. None if there's no title.
pub fn parse(path: &Path, text: &str) -> Option<Script> {
    let mut s = Script {
        path: path.to_path_buf(),
        title: String::new(),
        mode: Mode::FullOutput,
        icon: None,
        description: None,
        package: None,
        keywords: Vec::new(),
        arguments: Vec::new(),
        needs_confirmation: false,
        current_directory: None,
    };
    let mut args: Vec<(u8, Argument)> = Vec::new();
    for line in text.lines().take(80) {
        let Some(at) = line.find("@raycast.").map(|i| i + 9).or_else(|| line.find("@glint.").map(|i| i + 7)) else { continue };
        let rest = &line[at..];
        let (key, value) = rest.split_once(char::is_whitespace).map_or((rest, ""), |(k, v)| (k, v.trim()));
        let value = value.trim_end_matches("-->").trim();
        match key {
            "title" => s.title = value.to_string(),
            "mode" => {
                s.mode = match value {
                    "silent" => Mode::Silent,
                    "compact" => Mode::Compact,
                    "inline" => Mode::Inline,
                    "list" => Mode::List,
                    _ => Mode::FullOutput,
                }
            }
            "icon" if !value.is_empty() => s.icon = Some(value.to_string()),
            "description" => s.description = Some(value.to_string()),
            "packageName" => s.package = Some(value.to_string()),
            "keywords" => s.keywords = value.split(',').map(|k| k.trim().to_lowercase()).filter(|k| !k.is_empty()).collect(),
            "needsConfirmation" => s.needs_confirmation = value == "true",
            "currentDirectoryPath" if !value.is_empty() => s.current_directory = Some(PathBuf::from(expand(value))),
            k if k.starts_with("argument") => {
                if let (Ok(n), Ok(a)) = (k[8..].parse::<u8>(), serde_json::from_str::<RawArg>(value)) {
                    args.push((n, Argument { placeholder: a.placeholder, optional: a.optional, secure: a.secure || a.kind == "password" }));
                }
            }
            _ => {}
        }
    }
    args.sort_by_key(|(n, _)| *n);
    s.arguments = args.into_iter().map(|(_, a)| a).take(3).collect();
    (!s.title.is_empty()).then_some(s)
}

#[derive(Deserialize)]
struct RawArg {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    placeholder: String,
    #[serde(default)]
    optional: bool,
    #[serde(default, rename = "percentEncoded")]
    _percent: bool,
    #[serde(default)]
    secure: bool,
}

fn expand(p: &str) -> String {
    p.strip_prefix('~').map_or_else(|| p.to_string(), |rest| format!("{}{rest}", paths::home().display()))
}

#[derive(Debug)]
pub struct Output {
    pub text: String,
    pub ok: bool,
}

/// Runs a script with its arguments; output is stdout+stderr with ANSI codes
/// stripped. Killed after `timeout`.
pub fn run(s: &Script, args: &[String], timeout: Duration) -> Output {
    ensure_executable(&s.path);
    let dir = s.current_directory.clone().or_else(|| s.path.parent().map(Path::to_path_buf)).unwrap_or_else(paths::home);
    let mut cmd = Command::new(&s.path);
    cmd.args(args).current_dir(dir).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    // Scripts expect a login-ish PATH (Homebrew, etc.), not launchd's minimal one.
    cmd.env("PATH", format!("/opt/homebrew/bin:/usr/local/bin:{}", std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin:/usr/sbin:/sbin".into())));
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Output { text: format!("Couldn't run {}: {e}", s.path.display()), ok: false },
    };
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let out_t = std::thread::spawn(move || {
        let mut b = String::new();
        if let Some(o) = stdout.as_mut() {
            let _ = o.read_to_string(&mut b);
        }
        b
    });
    let err_t = std::thread::spawn(move || {
        let mut b = String::new();
        if let Some(e) = stderr.as_mut() {
            let _ = e.read_to_string(&mut b);
        }
        b
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => break None,
        }
    };
    let mut text = out_t.join().unwrap_or_default();
    let err = err_t.join().unwrap_or_default();
    if !err.trim().is_empty() {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&err);
    }
    let ok = status.is_some_and(|s| s.success());
    if status.is_none() {
        text.push_str(&format!("\nStopped after {} seconds.", timeout.as_secs()));
    }
    Output { text: strip_ansi(&text), ok }
}

/// Scripts downloaded or copied in often lose the executable bit.
fn ensure_executable(p: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(m) = std::fs::metadata(p) {
        let mode = m.permissions().mode();
        if mode & 0o100 == 0 {
            let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode | 0o755));
        }
    }
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for d in chars.by_ref() {
                    if d.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// One row a `list` script printed.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ListItem {
    pub title: String,
    pub subtitle: String,
    pub icon: String,
    pub accessory: String,
    pub copy: Option<String>,
    pub open: Option<String>,
    pub paste: Option<String>,
}

pub fn parse_list(text: &str) -> Result<Vec<ListItem>, String> {
    let start = text.find('[').ok_or("The script didn't print a JSON list.")?;
    let end = text.rfind(']').ok_or("The script didn't print a JSON list.")?;
    serde_json::from_str::<Vec<ListItem>>(&text[start..=end]).map_err(|e| format!("Couldn't read the script's JSON: {e}"))
}

/// Template for "Create Extension".
pub fn template(title: &str, mode: &str) -> String {
    let body = match mode {
        "list" => r#"# $1 is what's typed in the search bar. Print a JSON list of items.
query="$1"
cat <<EOF
[
  { "title": "Hello, ${query:-world}", "subtitle": "Press Return to copy", "icon": "sparkles", "copy": "Hello, ${query:-world}" },
  { "title": "Open the Glint repo", "subtitle": "github.com/dyascj/glint", "icon": "link", "open": "https://github.com/dyascj/glint" }
]
EOF"#
            .to_string(),
        _ => "echo \"Hello from your new Glint extension\"".to_string(),
    };
    format!(
        "#!/bin/bash\n\n# Required parameters:\n# @glint.title {title}\n# @glint.mode {mode}\n\n# Optional parameters:\n# @glint.icon sparkles\n# @glint.description What this extension does\n# @glint.packageName My Extensions\n\n{body}\n"
    )
}

/// Example extensions, one per mode, in Extensions/Samples.
const SAMPLES: [(&str, &str); 4] = [
    (
        "say-hello.sh",
        r#"#!/bin/bash

# @glint.title Say Hello
# @glint.mode fullOutput
# @glint.icon hand.wave
# @glint.packageName Glint Samples
# @glint.description Greets you by name
# @glint.argument1 { "type": "text", "placeholder": "Your name" }

echo "Hello, ${1:-friend}!"
echo
echo "This is a full-output extension. Edit me in:"
echo "$0"
"#,
    ),
    (
        "recent-downloads.js",
        r#"#!/usr/bin/osascript -l JavaScript

// @glint.title Recent Downloads
// @glint.mode list
// @glint.icon arrow.down.circle
// @glint.packageName Glint Samples
// @glint.description Your newest downloads, searchable

ObjC.import('Foundation')
function run(argv) {
  const q = (argv[0] || '').toLowerCase()
  const dir = $.NSHomeDirectory().js + '/Downloads'
  const fm = $.NSFileManager.defaultManager
  const names = ObjC.deepUnwrap(fm.contentsOfDirectoryAtPathError(dir, null)) || []
  const items = names
    .filter(n => !n.startsWith('.') && n.toLowerCase().includes(q))
    .map(n => {
      const attrs = fm.attributesOfItemAtPathError(dir + '/' + n, null)
      return { n, t: attrs.isNil() ? 0 : attrs.fileModificationDate.timeIntervalSince1970 }
    })
    .sort((a, b) => b.t - a.t)
    .slice(0, 40)
    .map(({ n }) => ({ title: n, subtitle: '~/Downloads', icon: 'doc', open: 'file://' + encodeURI(dir + '/' + n), copy: dir + '/' + n }))
  return JSON.stringify(items)
}
"#,
    ),
    (
        "copy-date.sh",
        r#"#!/bin/bash

# @glint.title Copy Today's Date
# @glint.mode compact
# @glint.icon calendar
# @glint.packageName Glint Samples
# @glint.keywords today, iso

today=$(date +%F)
printf "%s" "$today" | pbcopy
echo "Copied $today"
"#,
    ),
    (
        "toggle-wifi.sh",
        r#"#!/bin/bash

# @glint.title Toggle Wi-Fi
# @glint.mode compact
# @glint.icon wifi
# @glint.packageName Glint Samples
# @glint.keywords wireless, network

dev=$(networksetup -listallhardwareports | awk '/Wi-Fi/{getline; print $2}')
if networksetup -getairportpower "$dev" | grep -q On; then
  networksetup -setairportpower "$dev" off && echo "Wi-Fi off"
else
  networksetup -setairportpower "$dev" on && echo "Wi-Fi on"
fi
"#,
    ),
];

/// Writes the samples (skipping any that exist). Returns how many were written.
pub fn install_samples() -> std::io::Result<usize> {
    use std::os::unix::fs::PermissionsExt;
    let dir = paths::extensions_dir().join("Samples");
    std::fs::create_dir_all(&dir)?;
    let mut n = 0;
    for (name, body) in SAMPLES {
        let p = dir.join(name);
        if !p.exists() {
            std::fs::write(&p, body)?;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755))?;
            n += 1;
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_raycast_and_glint_metadata() {
        let raycast = "#!/bin/bash\n# @raycast.schemaVersion 1\n# @raycast.title Toggle Wi-Fi\n# @raycast.mode compact\n# @raycast.icon 📶\n# @raycast.packageName System\n# @raycast.argument1 { \"type\": \"text\", \"placeholder\": \"on/off\", \"optional\": true }\n# @raycast.argument2 { \"type\": \"password\", \"placeholder\": \"Secret\" }\necho hi\n";
        let s = parse(Path::new("/x/wifi.sh"), raycast).unwrap();
        assert_eq!(s.title, "Toggle Wi-Fi");
        assert_eq!(s.mode, Mode::Compact);
        assert_eq!(s.icon.as_deref(), Some("📶"));
        assert_eq!(s.package.as_deref(), Some("System"));
        assert_eq!(s.arguments.len(), 2);
        assert!(s.arguments[0].optional && !s.arguments[0].secure);
        assert!(s.arguments[1].secure);

        let glint = "#!/usr/bin/env python3\n# @glint.title Search Notes\n# @glint.mode list\n";
        assert_eq!(parse(Path::new("/x/n.py"), glint).unwrap().mode, Mode::List);
        assert!(parse(Path::new("/x/none.sh"), "#!/bin/sh\necho no metadata\n").is_none());
        let js = "// @raycast.title JS One\n// @raycast.mode silent\n";
        assert_eq!(parse(Path::new("/x/a.js"), js).unwrap().mode, Mode::Silent);
    }

    #[test]
    fn samples_parse_in_every_mode() {
        let modes: Vec<Mode> = SAMPLES.iter().map(|(n, b)| parse(Path::new(n), b).expect("sample has metadata").mode).collect();
        assert_eq!(modes, [Mode::FullOutput, Mode::List, Mode::Compact, Mode::Compact]);
    }

    #[test]
    fn list_output_parses() {
        let items = parse_list("noise\n[{\"title\":\"A\",\"copy\":\"a\"},{\"title\":\"B\",\"open\":\"https://x\"}]\n").unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].copy.as_deref(), Some("a"));
        assert!(parse_list("nothing").is_err());
    }

    #[test]
    fn runs_scripts_and_template_parses() {
        let dir = std::env::temp_dir().join(format!("glint-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("hello.sh");
        std::fs::write(&p, template("Hello", "fullOutput")).unwrap();
        let s = parse(&p, &std::fs::read_to_string(&p).unwrap()).unwrap();
        let out = run(&s, &[], Duration::from_secs(5));
        assert!(out.ok, "{}", out.text);
        assert_eq!(out.text.trim(), "Hello from your new Glint extension");
        let l = dir.join("list.sh");
        std::fs::write(&l, template("List", "list")).unwrap();
        let s = parse(&l, &std::fs::read_to_string(&l).unwrap()).unwrap();
        let items = parse_list(&run(&s, &["CJ".into()], Duration::from_secs(5)).text).unwrap();
        assert_eq!(items[0].title, "Hello, CJ");
        assert_eq!(strip_ansi("\u{1b}[31mred\u{1b}[0m"), "red");
        let _ = std::fs::remove_dir_all(dir);
    }
}
