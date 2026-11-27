use br_agentic_harness_contract::FilePath;

use crate::config::AbsolutePath;

pub(crate) const WORK_DIR: &str = "/work";

const NO_METADATA: [&str; 3] = ["--no-xattrs", "--no-acls", "--no-fflags"];

fn owned<const N: usize>(items: [&str; N]) -> Vec<String> {
    items.into_iter().map(str::to_owned).collect()
}

fn extract(tools: &AbsolutePath, owner: &str, directory: &str) -> Vec<String> {
    let mut argv = owned([
        tools.as_str(),
        "--extract",
        "--file",
        "-",
        "--preserve-permissions",
        owner,
        "--numeric-owner",
    ]);
    argv.extend(owned(NO_METADATA));
    argv.extend(owned(["-C", directory]));
    argv
}

pub fn tar_create(tools: &AbsolutePath, scope: &[AbsolutePath]) -> Vec<String> {
    let mut argv = owned([
        tools.as_str(),
        "--create",
        "--file",
        "-",
        "--format",
        "pax",
        "--numeric-owner",
    ]);
    argv.extend(owned(NO_METADATA));
    argv.extend(owned(["--options", "hdrcharset=BINARY", "-C", "/", "--"]));
    argv.extend(scope.iter().map(|path| path.relative().to_owned()));
    argv
}

pub fn tar_restore(tools: &AbsolutePath) -> Vec<String> {
    extract(tools, "--same-owner", "/")
}

fn root_entry_pattern(root: &AbsolutePath) -> String {
    let mut pattern = String::from("^");
    for character in root.relative().chars() {
        if matches!(character, '\\' | '*' | '?' | '[' | ']' | '$' | '^') {
            pattern.push('\\');
        }
        pattern.push(character);
    }
    pattern.push('$');
    pattern
}

pub fn tar_restore_scoped(tools: &AbsolutePath, scope: &[AbsolutePath]) -> Vec<String> {
    let mut argv = tar_restore(tools);
    for root in scope {
        argv.push("--exclude".to_owned());
        argv.push(root_entry_pattern(root));
    }
    argv
}

pub fn tar_write(tools: &AbsolutePath) -> Vec<String> {
    extract(tools, "--no-same-owner", WORK_DIR)
}

pub fn tar_seed(seed_tools: &AbsolutePath, seed_root: &AbsolutePath) -> Vec<String> {
    extract(seed_tools, "--same-owner", seed_root.as_str())
}

pub fn run_foreground(command: &str) -> Vec<String> {
    owned(["sh", "-c", "cd /work && exec sh -c \"$1\"", "sh", command])
}

pub fn run_background(command: &str) -> Vec<String> {
    owned([
        "sh",
        "-c",
        "cd /work && { (exec sh -c \"$1\") </dev/null >/dev/null 2>&1 & echo $!; }",
        "sh",
        command,
    ])
}

pub fn read(path: &FilePath) -> Vec<String> {
    owned(["cat", &format!("{WORK_DIR}/{path}")])
}

pub fn list(under: Option<&FilePath>) -> Vec<String> {
    let folder = match under {
        Some(under) => format!("{WORK_DIR}/{under}"),
        None => WORK_DIR.to_owned(),
    };
    owned([
        "sh",
        "-c",
        "[ -e \"$1\" ] || exit 0; exec find \"$1\" -type f -print0",
        "sh",
        &folder,
    ])
}

pub fn kill(process: u32) -> Vec<String> {
    owned([
        "sh",
        "-c",
        "kill -9 \"$1\" 2>/dev/null; exit 0",
        "sh",
        &process.to_string(),
    ])
}
