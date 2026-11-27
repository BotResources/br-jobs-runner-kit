use serde_json::Value;

const KEEPER_CONFIG_MAP: &str = "br-sandbox-keeper-config";
const KEEPER_CONFIG_KEY: &str = "config.json";

pub fn keeper_document(objects: &[Value]) -> String {
    let map = objects
        .iter()
        .find(|object| {
            object["kind"] == "ConfigMap" && object["metadata"]["name"] == KEEPER_CONFIG_MAP
        })
        .expect("the chart renders the keeper ConfigMap");
    map["data"][KEEPER_CONFIG_KEY]
        .as_str()
        .expect("the keeper ConfigMap holds the key config.json")
        .to_owned()
}
