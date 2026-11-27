use std::io::Write;
use std::path::Path;
use std::process::Command;

use crate::env::Env;
use crate::error::{BatteryError, Res};

fn clean(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c == '\t' || c == '\n' || c == '\r' {
                ' '
            } else {
                c
            }
        })
        .collect()
}

fn try_append(dir: &Path, file: &str, line: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut handle = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(file))?;
    handle.write_all(line.as_bytes())
}

pub fn append(dir: &Path, file: &str, line: &str) {
    if let Err(error) = try_append(dir, file, line) {
        panic!(
            "the evidence file {} in {} cannot be written: {error}",
            file,
            dir.display()
        );
    }
}

pub fn now_utc() -> String {
    Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|text| text.trim().to_owned())
        .unwrap_or_default()
}

pub fn load_now() -> (String, String) {
    let raw = Command::new("sysctl")
        .args(["-n", "vm.loadavg"])
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .or_else(|| std::fs::read_to_string("/proc/loadavg").ok())
        .unwrap_or_default();
    let numbers: Vec<&str> = raw
        .split_whitespace()
        .filter(|part| *part != "{" && *part != "}")
        .collect();
    let at = |index: usize| numbers.get(index).copied().unwrap_or("").to_owned();
    (at(0), at(1))
}

impl Env {
    pub fn record(&self, name: &str, value: impl std::fmt::Display, unit: &str) {
        let (load1, load5) = load_now();
        let line = format!(
            "{}\t{}\t{}\t{}\t{}\t{}\n",
            clean(name),
            clean(&value.to_string()),
            clean(unit),
            load1,
            load5,
            now_utc()
        );
        append(&self.evidence, "measurements.tsv", &line);
    }

    pub fn check(&self, name: &str, pass: bool, detail: &str) {
        let verdict = if pass { "pass" } else { "fail" };
        let line = format!(
            "{}\t{}\t{}\t{}\n",
            clean(name),
            verdict,
            clean(detail),
            now_utc()
        );
        append(&self.evidence, "checks.tsv", &line);
        eprintln!("check {name}: {verdict} {detail}");
    }
}

pub struct Checks<'a> {
    env: &'a Env,
    seen: usize,
    failed: Vec<String>,
}

impl<'a> Checks<'a> {
    pub fn new(env: &'a Env) -> Self {
        Self {
            env,
            seen: 0,
            failed: Vec::new(),
        }
    }

    pub fn expect(&mut self, name: &str, pass: bool, detail: impl AsRef<str>) {
        self.seen += 1;
        self.env.check(name, pass, detail.as_ref());
        if !pass {
            self.failed
                .push(format!("{name} ({})", clean(detail.as_ref())));
        }
    }

    pub fn finish(self) -> Res {
        if self.seen == 0 {
            Err(BatteryError::NoChecks)
        } else if self.failed.is_empty() {
            Ok(())
        } else {
            Err(BatteryError::Failed {
                count: self.failed.len(),
                names: self.failed.join("; "),
            })
        }
    }
}
