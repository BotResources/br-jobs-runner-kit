use std::collections::BTreeSet;

use br_agentic_harness_contract::{Seed, SeedEntry};
use br_runner_sandbox::Sandbox;
use sha2::{Digest, Sha256};

use crate::adapter::{path, sh_ok};
use crate::error::{Res, data};
use crate::prng::Prng;
use br_agentic_harness_contract::BoxId;

pub struct SeedSet {
    pub seed: Seed,
    pub expected: Vec<(String, String)>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn place(index: usize) -> String {
    match index % 3 {
        0 => format!("f{index}.bin"),
        1 => format!("a{}/f{index}.bin", index % 5),
        _ => format!("a{}/b{}/f{index}.bin", index % 5, index % 4),
    }
}

pub fn make(random_seed: u64, files: usize, file_bytes: usize, last_bytes: usize) -> Res<SeedSet> {
    let mut prng = Prng::new(random_seed);
    let mut entries = Vec::new();
    let mut folders = BTreeSet::new();
    let mut expected = Vec::new();
    for index in 0..files {
        let name = place(index);
        let size = if index + 1 == files {
            last_bytes
        } else {
            file_bytes
        };
        let bytes = prng.bytes(size);
        expected.push((name.clone(), sha256_hex(&bytes)));
        let mut parent = String::new();
        let parts: Vec<&str> = name.split('/').collect();
        for part in &parts[..parts.len() - 1] {
            if !parent.is_empty() {
                parent.push('/');
            }
            parent.push_str(part);
            folders.insert(parent.clone());
        }
        entries.push(SeedEntry::file(path(&name)?, 0o644, bytes));
    }
    for folder in folders {
        entries.push(SeedEntry::directory(path(&folder)?, 0o755));
    }
    expected.sort();
    let seed = Seed::new(entries).map_err(|error| data("seed", error.to_string()))?;
    Ok(SeedSet { seed, expected })
}

pub fn standard() -> Res<SeedSet> {
    make(11, 50, 200_000, 199_000)
}

pub fn stress() -> Res<SeedSet> {
    make(12, 500, 100_000, 99_000)
}

pub async fn present(
    sandbox: &Sandbox,
    boxed: &BoxId,
    seed_root: &str,
    set: &SeedSet,
) -> Res<bool> {
    let out = sh_ok(
        sandbox,
        boxed,
        &format!("find {seed_root} -type f -exec sha256sum {{}} + | sort -k2"),
    )
    .await?;
    let mut found: Vec<(String, String)> = out
        .lines()
        .filter_map(|line| {
            let (hash, name) = line.split_once("  ")?;
            let relative = name.strip_prefix(&format!("{seed_root}/"))?;
            Some((relative.to_owned(), hash.to_owned()))
        })
        .collect();
    found.sort();
    Ok(found == set.expected)
}
