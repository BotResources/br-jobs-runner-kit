use battery::Session;
use battery::error::Res;
use battery::evidence::Checks;

const LIST: &str = include_str!("harness_excluded.txt");

fn rows() -> Vec<Vec<&'static str>> {
    LIST.lines().map(|line| line.split('|').collect()).collect()
}

#[tokio::test]
async fn the_upstream_tests_that_need_the_memory_sandbox_are_listed_with_their_reason() -> Res {
    let session = Session::start().await?;
    let mut checks = Checks::new(&session.env);
    let all = rows();
    let harness = all
        .iter()
        .filter(|row| row[0] == "br-agentic-harness")
        .count();
    let contract = all
        .iter()
        .filter(|row| row[0] == "br-agentic-harness-contract")
        .count();
    let complete = all
        .iter()
        .all(|row| row.len() == 4 && !row[2].is_empty() && !row[3].is_empty());
    checks.expect(
        "harness-suite-every-excluded-upstream-test-has-a-name-and-a-reason",
        complete,
        format!("{harness} excluded in the harness suite and {contract} in the contract suite"),
    );
    checks.finish()
}
