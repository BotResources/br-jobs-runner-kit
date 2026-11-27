use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::env::Env;
use crate::error::{Res, data};
use crate::proc::{Kube, SHORT, run, strings, succeeded};

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_decode(text: &str) -> Res<Vec<u8>> {
    let mut out = Vec::new();
    let mut buffer = 0u32;
    let mut bits = 0;
    for byte in text
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace() && *byte != b'=')
    {
        let value = ALPHABET
            .iter()
            .position(|known| *known == byte)
            .ok_or_else(|| data("base64", "a character is not in the alphabet"))?;
        buffer = (buffer << 6) | value as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

pub struct TempContext {
    pub name: String,
    files: Vec<PathBuf>,
    clusters: Vec<String>,
    users: Vec<String>,
}

struct Minified {
    cluster: Value,
    user: Value,
    user_name: String,
}

async fn minified(env: &Env) -> Res<Minified> {
    let kube = Kube::cluster_wide(env);
    let config = kube
        .json(&[
            "config",
            "view",
            "--raw",
            "--minify",
            "--context",
            &env.context,
        ])
        .await?;
    let cluster = config["clusters"][0]["cluster"].clone();
    let user = config["users"][0]["user"].clone();
    let user_name = config["users"][0]["name"]
        .as_str()
        .ok_or_else(|| data("kubeconfig", "the user has no name"))?
        .to_owned();
    Ok(Minified {
        cluster,
        user,
        user_name,
    })
}

fn write_secret(env: &Env, name: &str, bytes: &[u8]) -> Res<PathBuf> {
    std::fs::create_dir_all(&env.work)?;
    let path = env.work.join(name);
    std::fs::write(&path, bytes)?;
    Ok(path)
}

fn path_text(path: &std::path::Path) -> String {
    path.display().to_string()
}

impl TempContext {
    pub async fn through_gate(env: &Env, name: &str, port: u16) -> Res<Self> {
        let source = minified(env).await?;
        let ca = source.cluster["certificate-authority-data"]
            .as_str()
            .ok_or_else(|| data("kubeconfig", "the cluster has no embedded authority"))?;
        let ca_path = write_secret(env, &format!("{name}.ca.pem"), &base64_decode(ca)?)?;
        let kube = Kube::cluster_wide(env);
        let server = format!("https://127.0.0.1:{port}");
        kube.run(&[
            "config",
            "set-cluster",
            name,
            &format!("--server={server}"),
            "--tls-server-name=localhost",
            &format!("--certificate-authority={}", path_text(&ca_path)),
            "--embed-certs=true",
        ])
        .await?;
        kube.run(&[
            "config",
            "set-context",
            name,
            &format!("--cluster={name}"),
            &format!("--user={}", source.user_name),
        ])
        .await?;
        Ok(Self {
            name: name.to_owned(),
            files: vec![ca_path],
            clusters: vec![name.to_owned()],
            users: Vec::new(),
        })
    }

    pub async fn impersonating(env: &Env, name: &str, subject: &str) -> Res<Self> {
        let source = minified(env).await?;
        let cert = source.user["client-certificate-data"]
            .as_str()
            .ok_or_else(|| data("kubeconfig", "the user has no embedded certificate"))?;
        let key = source.user["client-key-data"]
            .as_str()
            .ok_or_else(|| data("kubeconfig", "the user has no embedded key"))?;
        let cert_path = write_secret(env, &format!("{name}.crt"), &base64_decode(cert)?)?;
        let key_path = write_secret(env, &format!("{name}.key"), &base64_decode(key)?)?;
        let kube = Kube::cluster_wide(env);
        kube.run(&[
            "config",
            "set-credentials",
            name,
            &format!("--client-certificate={}", path_text(&cert_path)),
            &format!("--client-key={}", path_text(&key_path)),
            "--embed-certs=true",
        ])
        .await?;
        kube.run(&["config", "set", &format!("users.{name}.act-as"), subject])
            .await?;
        let cluster = minified(env).await?.cluster;
        let server = cluster["server"]
            .as_str()
            .ok_or_else(|| data("kubeconfig", "the cluster has no server"))?
            .to_owned();
        let ca = cluster["certificate-authority-data"]
            .as_str()
            .ok_or_else(|| data("kubeconfig", "the cluster has no embedded authority"))?;
        let ca_path = write_secret(env, &format!("{name}.ca.pem"), &base64_decode(ca)?)?;
        kube.run(&[
            "config",
            "set-cluster",
            name,
            &format!("--server={server}"),
            &format!("--certificate-authority={}", path_text(&ca_path)),
            "--embed-certs=true",
        ])
        .await?;
        kube.run(&[
            "config",
            "set-context",
            name,
            &format!("--cluster={name}"),
            &format!("--user={name}"),
        ])
        .await?;
        Ok(Self {
            name: name.to_owned(),
            files: vec![cert_path, key_path, ca_path],
            clusters: vec![name.to_owned()],
            users: vec![name.to_owned()],
        })
    }

    pub async fn can_i(&self, namespace: &str, verb: &str, resource: &str) -> Res<bool> {
        let args = strings(&[
            "--context",
            &self.name,
            "-n",
            namespace,
            "auth",
            "can-i",
            verb,
            resource,
        ]);
        let output = run("kubectl", &args, None, SHORT).await?;
        if output.code > 1 {
            succeeded("kubectl", &args, output)?;
            return Ok(false);
        }
        Ok(output.text() == "yes")
    }
}

impl Drop for TempContext {
    fn drop(&mut self) {
        let quiet = |args: &[&str]| {
            let _ = Command::new("kubectl")
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        };
        quiet(&["config", "delete-context", &self.name]);
        for cluster in &self.clusters {
            quiet(&["config", "delete-cluster", cluster]);
        }
        for user in &self.users {
            quiet(&["config", "delete-user", user]);
        }
        for file in &self.files {
            let _ = std::fs::remove_file(file);
        }
    }
}
