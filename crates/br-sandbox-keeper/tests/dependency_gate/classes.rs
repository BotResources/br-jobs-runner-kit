pub struct Class {
    pub name: &'static str,
    pub exact: &'static [&'static str],
    pub prefixes: &'static [&'static str],
}

pub const CLASSES: [Class; 7] = [
    Class {
        name: "http client or transport",
        exact: &[
            "reqwest",
            "ureq",
            "isahc",
            "curl",
            "curl-sys",
            "surf",
            "attohttpc",
            "minreq",
            "awc",
            "ehttp",
            "http-client",
            "h2",
            "hyper",
            "tungstenite",
            "async-tungstenite",
            "tokio-tungstenite",
            "websocket",
        ],
        prefixes: &["hyper-"],
    },
    Class {
        name: "kubernetes client",
        exact: &["kube", "kube-client", "kube-runtime", "kubert"],
        prefixes: &[],
    },
    Class {
        name: "object store client",
        exact: &["object_store", "s3", "rust-s3", "opendal"],
        prefixes: &[],
    },
    Class {
        name: "grpc client",
        exact: &["grpcio", "grpc", "tonic"],
        prefixes: &["tonic-"],
    },
    Class {
        name: "telemetry or error reporting exporter",
        exact: &[
            "telemetry",
            "cadence",
            "dogstatsd",
            "statsd",
            "newrelic",
            "honeycomb",
            "posthog",
            "segment",
            "rollbar",
            "bugsnag",
            "airbrake",
            "logtail",
        ],
        prefixes: &[
            "opentelemetry",
            "tracing-opentelemetry",
            "sentry",
            "datadog",
            "metrics-exporter",
            "tracing-loki",
            "tracing-gelf",
            "tracing-honeycomb",
            "tracing-datadog",
            "influxdb",
            "fluent",
            "gelf",
        ],
    },
    Class {
        name: "vendor sdk",
        exact: &["octocrab", "stripe", "twilio", "clickhouse"],
        prefixes: &[
            "aws-sdk-",
            "aws-smithy-",
            "aws-config",
            "rusoto",
            "google-cloud",
            "gcloud",
            "azure_",
            "azure-",
        ],
    },
    Class {
        name: "update, licence or other network protocol client",
        exact: &[
            "update-informer",
            "self_update",
            "self-replace",
            "version-check",
            "keygen-rs",
            "lexactivator",
            "async-nats",
            "nats",
            "redis",
            "sqlx",
            "tokio-postgres",
            "postgres",
            "mysql",
            "mysql_async",
            "mongodb",
            "rdkafka",
            "lapin",
            "lettre",
            "paho-mqtt",
            "rumqttc",
            "suppaftp",
            "russh",
            "ssh2",
            "libp2p",
            "quinn",
            "s2n-quic",
            "c-ares",
        ],
        prefixes: &["hickory-", "trust-dns-"],
    },
];

pub const ALLOWED: [(&str, &str); 11] = [
    ("kube", "the Kubernetes client"),
    ("kube-client", "the Kubernetes client"),
    (
        "hyper",
        "transport of the Kubernetes client, of the S3 client, and the health endpoint",
    ),
    (
        "hyper-util",
        "transport of the Kubernetes client, of the S3 client, and the health endpoint",
    ),
    (
        "hyper-rustls",
        "TLS connector of the Kubernetes client and of the S3 client",
    ),
    ("hyper-timeout", "timeout layer of the Kubernetes client"),
    ("h2", "HTTP/2 of the transport"),
    ("tungstenite", "watch socket of the Kubernetes client"),
    ("tokio-tungstenite", "watch socket of the Kubernetes client"),
    ("object_store", "the S3 client"),
    ("reqwest", "HTTP layer of the S3 client"),
];

pub const DIRECT: [&str; 23] = [
    "br-runner-sandbox",
    "bytes",
    "futures",
    "http",
    "http-body-util",
    "hyper",
    "hyper-util",
    "jiff",
    "k8s-openapi",
    "kube",
    "object_store",
    "reqwest",
    "rustls",
    "serde",
    "serde-saphyr",
    "serde_json",
    "thiserror",
    "tokio",
    "tokio-util",
    "tracing",
    "tracing-subscriber",
    "url",
    "uuid",
];

pub fn class_of(name: &str) -> Option<&'static str> {
    CLASSES
        .iter()
        .find(|class| {
            class.exact.contains(&name)
                || class.prefixes.iter().any(|prefix| name.starts_with(prefix))
        })
        .map(|class| class.name)
}

pub fn is_allowed(name: &str) -> bool {
    ALLOWED.iter().any(|(allowed, _)| *allowed == name)
}
