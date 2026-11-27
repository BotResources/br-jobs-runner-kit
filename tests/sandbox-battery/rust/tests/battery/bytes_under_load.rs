use std::time::Duration;

use battery::Session;
use battery::adapter::{self, path};
use battery::env::Env;
use battery::error::{BatteryError, Res};
use battery::evidence::{Checks, load_now};
use battery::guards::PodReaper;
use battery::ids::Lane;
use battery::prng::Prng;
use br_agentic_harness_contract::{Command, RunMode, RunOutcome, SandboxPort, Seed};
use sha2::{Digest, Sha256};

const FILE_BYTES: usize = 50_000_000;
const OUTPUT_BYTES: usize = 20_000_000;
const LINE: &str = "0123456789abcdefghijklmnopqrstuvwxyz-ABCDEFGHIJKLMNOPQRSTUVWXYZ\n";
const BOUND: Duration = Duration::from_secs(240);

#[derive(Clone, Copy, PartialEq)]
enum Verdict {
    Ok,
    ErrorReported,
    LossWithoutError,
}

struct Tally {
    series: String,
    kind: &'static str,
    ok: usize,
    errors: usize,
    losses: usize,
}

impl Tally {
    fn new(series: &str, kind: &'static str) -> Self {
        Self {
            series: series.to_owned(),
            kind,
            ok: 0,
            errors: 0,
            losses: 0,
        }
    }

    fn line(
        &mut self,
        env: &Env,
        iteration: usize,
        expected: usize,
        got: Option<usize>,
        hash_match: Option<bool>,
        error: Option<String>,
    ) {
        let verdict = match (&error, got, hash_match) {
            (Some(_), _, _) => Verdict::ErrorReported,
            (None, Some(n), Some(true)) if n == expected => Verdict::Ok,
            _ => Verdict::LossWithoutError,
        };
        match verdict {
            Verdict::Ok => self.ok += 1,
            Verdict::ErrorReported => self.errors += 1,
            Verdict::LossWithoutError => self.losses += 1,
        }
        let tag = match verdict {
            Verdict::Ok => "ok",
            Verdict::ErrorReported => "error-reported",
            Verdict::LossWithoutError => "LOSS-WITHOUT-ERROR",
        };
        let (load1, _) = load_now();
        let text = format!(
            "{tag} series={} kind={} iteration={iteration} expected={expected} got={} hash_match={} error={} load1={load1}",
            self.series,
            self.kind,
            got.map_or("none".to_owned(), |n| n.to_string()),
            hash_match.map_or("none".to_owned(), |m| m.to_string()),
            error.as_deref().unwrap_or("none"),
        );
        eprintln!("{text}");
        env.record(
            &format!("bytes_{}_{}_{iteration}", self.kind, self.series),
            &text,
            "line",
        );
    }

    fn summary(&self, env: &Env) -> String {
        let (load1, load5) = load_now();
        let text = format!(
            "SUMMARY series={} kind={} iterations={} ok={} error-reported={} LOSS-WITHOUT-ERROR={} load1={load1} load5={load5}",
            self.series,
            self.kind,
            self.ok + self.errors + self.losses,
            self.ok,
            self.errors,
            self.losses
        );
        eprintln!("{text}");
        env.record(
            &format!("bytes_{}_{}_summary", self.kind, self.series),
            &text,
            "line",
        );
        text
    }
}

fn series() -> String {
    std::env::var("BYTES_SERIES").unwrap_or_else(|_| "unlabelled".to_owned())
}

fn iterations() -> Res<usize> {
    let Ok(text) = std::env::var("BYTES_ITERATIONS") else {
        return Ok(10);
    };
    match text.parse::<usize>() {
        Ok(count) if count >= 1 => Ok(count),
        _ => Err(BatteryError::BadVariable {
            name: "BYTES_ITERATIONS",
            value: text,
            expected: "a whole number of 1 or more",
        }),
    }
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn pattern(length: usize) -> Vec<u8> {
    LINE.as_bytes()
        .iter()
        .copied()
        .cycle()
        .take(length)
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_read_of_a_50_mb_file_never_loses_bytes_without_an_error() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let sandbox = adapter::standard(env).await?;
    let mut checks = Checks::new(env);
    let (_, turn) = Lane::new()?.first()?;
    let opened = adapter::open(&sandbox, turn, Seed::empty()).await?;
    let mut reaper = PodReaper::new(env);
    reaper.track(opened.box_id.to_string());
    let content = Prng::new(20_261_127).bytes(FILE_BYTES);
    let expected = digest(&content);
    let target = path("probe/big.bin")?;
    adapter::bounded("the write of the 50 MB file", BOUND, async {
        Ok(sandbox
            .write(&opened.box_id, &target, content.clone())
            .await?)
    })
    .await?;
    let count = iterations()?;
    let mut tally = Tally::new(&series(), "read");
    for iteration in 1..=count {
        let outcome = adapter::bounded("the read of the 50 MB file", BOUND, async {
            Ok(sandbox.read(&opened.box_id, &target).await)
        })
        .await?;
        match outcome {
            Ok(bytes) => {
                let same = digest(&bytes) == expected;
                tally.line(
                    env,
                    iteration,
                    FILE_BYTES,
                    Some(bytes.len()),
                    Some(same),
                    None,
                );
            }
            Err(error) => tally.line(
                env,
                iteration,
                FILE_BYTES,
                None,
                None,
                Some(format!("{error:?}")),
            ),
        }
    }
    let text = tally.summary(env);
    checks.expect(
        "read-never-loses-bytes-without-an-error",
        tally.losses == 0 && tally.ok >= 1,
        text,
    );
    adapter::release(&sandbox, &opened.box_id).await?;
    checks.finish()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_foreground_run_never_loses_output_bytes_without_an_error() -> Res {
    let session = Session::start().await?;
    let env = &session.env;
    let sandbox = adapter::standard(env).await?;
    let mut checks = Checks::new(env);
    let (_, turn) = Lane::new()?.first()?;
    let opened = adapter::open(&sandbox, turn, Seed::empty()).await?;
    let mut reaper = PodReaper::new(env);
    reaper.track(opened.box_id.to_string());
    let expected_bytes = pattern(OUTPUT_BYTES);
    let expected = digest(&expected_bytes);
    let count = iterations()?;
    let body = format!("yes '{}' | head -c {OUTPUT_BYTES}", LINE.trim_end());
    for (kind, text) in [("stdout", body.clone()), ("stderr", format!("{body} >&2"))] {
        let command =
            Command::new(text).map_err(|e| battery::error::data("command", e.to_string()))?;
        let mut tally = Tally::new(&series(), kind);
        for iteration in 1..=count {
            let outcome = adapter::bounded("running the large output", BOUND, async {
                Ok(sandbox
                    .run(&opened.box_id, &command, RunMode::Foreground)
                    .await)
            })
            .await?;
            match outcome {
                Ok(RunOutcome::Finished(result)) => {
                    let (captured, other) = if kind == "stdout" {
                        (&result.stdout, &result.stderr)
                    } else {
                        (&result.stderr, &result.stdout)
                    };
                    if result.exit_code != 0 {
                        tally.line(
                            env,
                            iteration,
                            OUTPUT_BYTES,
                            Some(captured.len()),
                            Some(digest(captured) == expected),
                            Some(format!("exit code {}", result.exit_code)),
                        );
                    } else {
                        let same = digest(captured) == expected && other.is_empty();
                        tally.line(
                            env,
                            iteration,
                            OUTPUT_BYTES,
                            Some(captured.len()),
                            Some(same),
                            None,
                        );
                    }
                }
                Ok(RunOutcome::Started(_)) => tally.line(
                    env,
                    iteration,
                    OUTPUT_BYTES,
                    None,
                    None,
                    Some("a foreground run gave a process id".to_owned()),
                ),
                Err(error) => tally.line(
                    env,
                    iteration,
                    OUTPUT_BYTES,
                    None,
                    None,
                    Some(format!("{error:?}")),
                ),
            }
        }
        let text = tally.summary(env);
        checks.expect(
            &format!("run-{kind}-never-loses-bytes-without-an-error"),
            tally.losses == 0 && tally.ok >= 1,
            text,
        );
    }
    adapter::release(&sandbox, &opened.box_id).await?;
    checks.finish()
}
