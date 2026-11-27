use std::collections::{BTreeMap, BTreeSet};

use br_agentic_harness_contract::BoxId;
use br_runner_sandbox::Sandbox;

use crate::adapter::sh_ok;
use crate::error::{Res, data};

const BUILD: &str = r#"python3 - SEED <<'PYEOF'
import os, random, sys
rng = random.Random(int(sys.argv[1]))
base = '/work/tree'
modes = [0o644, 0o600, 0o755, 0o750, 0o444]
for folder in ['d%d' % d for d in range(25)] + ['links', 'hard']:
    os.makedirs(base + '/' + folder)
for n in range(1000):
    path = '%s/d%d/f%d' % (base, n % 25, n)
    with open(path, 'wb') as handle:
        handle.write(rng.randbytes(rng.randrange(0, 50001)))
    os.chmod(path, modes[n % 5])
with open('/work/big.bin', 'wb') as handle:
    handle.write(rng.randbytes(50000000))
for n in range(20):
    if n < 7:
        target = '../d%d/f%d' % (n, n)
    elif n < 14:
        target = '%s/d%d/f%d' % (base, n, n)
    else:
        target = 'missing%d' % n
    os.symlink(target, '%s/links/l%d' % (base, n))
    os.makedirs('%s/empty/e%d' % (base, n))
for n in range(10):
    os.link('%s/d%d/f%d' % (base, n, n), '%s/hard/h%d' % (base, n))
os.makedirs('/home/agent/.config/battery', exist_ok=True)
for n in range(5):
    with open('/home/agent/.config/battery/c%d' % n, 'wb') as handle:
        handle.write(rng.randbytes(2000 + n))
with open('/tmp/not-kept.txt', 'wb') as handle:
    handle.write(b'tmp')
stamp = 1700000000
for root in ['/work', '/home/agent']:
    for here, folders, names in os.walk(root, topdown=False):
        for name in names + folders:
            stamp += 3
            os.utime(os.path.join(here, name), (stamp, stamp), follow_symlinks=False)
PYEOF"#;

const STATE: &str = r#"python3 - SEED SIZE <<'PYEOF'
import os, random, sys
rng = random.Random(int(sys.argv[1]))
with open('/work/state.bin', 'wb') as handle:
    handle.write(rng.randbytes(int(sys.argv[2])))
os.makedirs('/work/small', exist_ok=True)
for n in range(50):
    with open('/work/small/s%d' % n, 'wb') as handle:
        handle.write(rng.randbytes(1024))
PYEOF"#;

const LIST: &str = "find /work /home/agent -mindepth 1 -printf '%p\\t%y\\t%m\\t%U\\t%G\\t%s\\t%T@\\t%l\\t%i\\t%n\\n' | sort";
const HASHES: &str = "find /work /home/agent -type f -exec sha256sum {} + | sort -k2";

pub async fn build(sandbox: &Sandbox, boxed: &BoxId, seed: u64) -> Res {
    sh_ok(sandbox, boxed, &BUILD.replace("SEED", &seed.to_string())).await?;
    Ok(())
}

pub async fn state(sandbox: &Sandbox, boxed: &BoxId, seed: u64, bytes: u64) -> Res {
    let text = STATE
        .replacen("SEED", &seed.to_string(), 1)
        .replacen("SIZE", &bytes.to_string(), 1);
    sh_ok(sandbox, boxed, &text).await?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub kind: String,
    pub mode: String,
    pub uid: String,
    pub gid: String,
    pub size: String,
    pub mtime: i64,
    pub target: String,
    pub inode: String,
    pub links: String,
}

#[derive(Debug, Clone)]
pub struct Capture {
    pub entries: BTreeMap<String, Entry>,
    pub hashes: BTreeMap<String, String>,
}

fn parse_entry(line: &str) -> Option<(String, Entry)> {
    let parts: Vec<&str> = line.split('\t').collect();
    if parts.len() != 10 {
        return None;
    }
    let mtime = parts[6].split('.').next()?.parse().ok()?;
    Some((
        parts[0].to_owned(),
        Entry {
            kind: parts[1].to_owned(),
            mode: parts[2].to_owned(),
            uid: parts[3].to_owned(),
            gid: parts[4].to_owned(),
            size: parts[5].to_owned(),
            mtime,
            target: parts[7].to_owned(),
            inode: parts[8].to_owned(),
            links: parts[9].to_owned(),
        },
    ))
}

pub async fn capture(sandbox: &Sandbox, boxed: &BoxId) -> Res<Capture> {
    let listing = sh_ok(sandbox, boxed, LIST).await?;
    let entries: BTreeMap<String, Entry> = listing.lines().filter_map(parse_entry).collect();
    if entries.is_empty() {
        return Err(data("tree capture", "the listing holds no entry"));
    }
    let hashes = sh_ok(sandbox, boxed, HASHES).await?;
    let hashes = hashes
        .lines()
        .filter_map(|line| line.split_once("  "))
        .map(|(hash, path)| (path.to_owned(), hash.to_owned()))
        .collect();
    Ok(Capture { entries, hashes })
}

fn groups(capture: &Capture) -> BTreeSet<Vec<String>> {
    let mut by_inode: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (path, entry) in &capture.entries {
        if entry.kind == "f" && entry.links != "1" {
            by_inode.entry(&entry.inode).or_default().push(path.clone());
        }
    }
    by_inode.into_values().collect()
}

pub fn executable_bits(capture: &Capture) -> BTreeMap<String, bool> {
    capture
        .entries
        .iter()
        .filter(|(_, entry)| entry.kind == "f")
        .map(|(path, entry)| {
            let mode = u32::from_str_radix(&entry.mode, 8).unwrap_or(0);
            (path.clone(), mode & 0o111 != 0)
        })
        .collect()
}

pub fn diff(before: &Capture, after: &Capture, mtime_slack: i64) -> Vec<String> {
    let mut found = Vec::new();
    let paths: BTreeSet<&String> = before.entries.keys().chain(after.entries.keys()).collect();
    for path in paths {
        match (before.entries.get(path), after.entries.get(path)) {
            (Some(left), Some(right)) => {
                let same = left.kind == right.kind
                    && left.mode == right.mode
                    && left.uid == right.uid
                    && left.gid == right.gid
                    && left.size == right.size
                    && left.target == right.target
                    && left.links == right.links
                    && (left.mtime - right.mtime).abs() <= mtime_slack;
                if !same {
                    found.push(format!("{path}: {left:?} against {right:?}"));
                }
            }
            (Some(_), None) => found.push(format!("{path}: missing after the restore")),
            (None, Some(_)) => found.push(format!("{path}: new after the restore")),
            (None, None) => {}
        }
    }
    if before.hashes != after.hashes {
        found.push("the content hashes differ".to_owned());
    }
    if groups(before) != groups(after) {
        found.push("the hard link groups differ".to_owned());
    }
    found
}
