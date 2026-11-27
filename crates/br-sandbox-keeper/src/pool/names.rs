pub const REPLICASET_PREFIX: &str = "warm-";
pub const QUOTA_NAME: &str = "br-sandbox-quota";
pub const ADMISSION_BINDING_NAME: &str = "br-sandbox-pod";
pub const POD_TEMPLATE_KEY: &str = "pod-template.json";

pub fn pool_id(namespace: &str) -> &str {
    namespace
}

pub fn replicaset_name(namespace: &str) -> String {
    format!("{REPLICASET_PREFIX}{namespace}")
}

pub fn template_hash_key(prefix: impl std::fmt::Display) -> String {
    format!("{prefix}/template-hash")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pool_id_is_the_name_of_the_namespace_and_the_replicaset_is_warm_and_the_namespace() {
        assert_eq!(pool_id("sbx-acme-full"), "sbx-acme-full");
        assert_eq!(replicaset_name("sbx-acme-full"), "warm-sbx-acme-full");
    }

    #[test]
    fn the_template_hash_annotation_follows_the_label_prefix() {
        assert_eq!(template_hash_key("br.sandbox"), "br.sandbox/template-hash");
        assert_eq!(template_hash_key("acme.sbx"), "acme.sbx/template-hash");
    }
}
