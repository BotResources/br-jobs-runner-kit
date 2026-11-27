use br_agentic_harness_contract::BoxId;
use br_runner_sandbox::Sandbox;

use crate::adapter::{self, Tunables};
use crate::env::Env;
use crate::error::{Res, data};

fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn tar_size_command(tools: &str, scope: &[String]) -> String {
    let roots = scope
        .iter()
        .map(|path| quoted(path.trim_start_matches('/')))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "{} --create --file - --format pax --numeric-owner --no-xattrs --no-acls --no-fflags --options hdrcharset=BINARY -C / -- {roots} | wc -c",
        quoted(tools)
    )
}

pub async fn expected_save_bytes(env: &Env, sandbox: &Sandbox, boxed: &BoxId) -> Res<u64> {
    let scope = Tunables::standard(env).scope;
    let command = tar_size_command(&env.agent_tools, &scope);
    let result = adapter::sh(sandbox, boxed, &command).await?;
    let stderr = String::from_utf8_lossy(&result.stderr).trim().to_owned();
    if result.exit_code != 0 || !stderr.is_empty() {
        return Err(data(
            "expected save size",
            format!("exit code {}, stderr {stderr}", result.exit_code),
        ));
    }
    let text = String::from_utf8_lossy(&result.stdout).trim().to_owned();
    text.parse()
        .map_err(|_| data("expected save size", format!("not a number: {text}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_names_every_scope_root_relative_to_the_filesystem_root() {
        let scope = vec!["/work".to_owned(), "/home/agent".to_owned()];
        let command = tar_size_command("/opt/bsdtar", &scope);
        assert!(command.starts_with("'/opt/bsdtar' --create --file - --format pax"));
        assert!(command.ends_with("-C / -- 'work' 'home/agent' | wc -c"));
    }
}
