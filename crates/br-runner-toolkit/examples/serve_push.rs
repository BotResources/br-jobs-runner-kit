#[path = "shared/serve.rs"]
pub mod serve;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    serve::main_named("serve_push").await
}
