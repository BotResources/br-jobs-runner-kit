use battery::adapter::Tunables;
use battery::env::Env;
use battery::error::Res;
use br_runner_sandbox::S3Store;

#[tokio::test(flavor = "multi_thread")]
async fn clients_build_when_the_process_sets_no_crypto_provider() -> Res {
    let env = Env::load()?;
    let tunables = Tunables::standard(&env);
    let config = tunables.config(&env)?;
    let store = S3Store::new(config.store());
    let sandbox = tunables.connect(&env).await;
    env.check(
        "crypto-provider-s3-client-builds",
        store.is_ok(),
        &format!("{:?}", store.as_ref().err()),
    );
    env.check(
        "crypto-provider-kube-client-builds",
        sandbox.is_ok(),
        &format!("{:?}", sandbox.as_ref().err()),
    );
    store?;
    sandbox?;
    Ok(())
}
