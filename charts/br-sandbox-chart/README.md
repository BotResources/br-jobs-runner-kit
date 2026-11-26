# br-sandbox-chart

The Helm chart of the sandbox. One installation per cluster. It renders everything that is declarative: namespaces, quotas, admission policy, network policies, warm pools, roles, the keeper and the network check. Licence: Apache-2.0. The chart is released by the workflow `chart-release`; it is not a library of the Libraries registry.

It does not claim a pod, execute a command, save or restore (the library `br-runner-sandbox`), size a pool or delete a save (the library `br-sandbox-keeper`). It creates no bucket, no Secret and no credential value.

## What a render holds

| Object | Scope | Gate |
|---|---|---|
| Namespace `{prefix}-system`, ConfigMap `br-sandbox-config` of the system namespace | once per installation | always |
| Namespace `{prefix}-{client}-{mode}`, ServiceAccount `sandbox` (no token, no Role), ResourceQuota `br-sandbox-quota`, LimitRange `br-sandbox-limits`, ConfigMap `br-sandbox-config` | each (client, mode) | always |
| ReplicaSet `warm-{client}-{mode}` | each (client, mode) | always; it has no `spec.replicas`, so a pool starts with one replica (the ReplicaSet default) until the keeper runs |
| NetworkPolicy `default-deny`, `egress-full` or `egress-allow` | each (client, mode) | always |
| Namespace `{sandbox namespace}-egress`, NetworkPolicies `default-deny`, `ingress-tunnel` and `egress-proxy`, ResourceQuota `br-sandbox-egress-quota` | each (client, mode) that uses a proxy | mode `allow`; mode `full` when `credential_set` is not empty |
| ConfigMaps `br-sandbox-proxy-config` and `br-sandbox-proxy-ca`, Deployment `br-sandbox-proxy` and Service `br-sandbox-proxy` | each egress namespace | `egress.enabled` |
| ConfigMap `br-sandbox-ca` (the CA certificate, not secret) | the sandbox namespace of each (client, mode) that uses a proxy | `egress.enabled` |
| ValidatingAdmissionPolicy and binding `{prefix}-{client}-{mode}-pod` | each (client, mode) | always |
| Role and RoleBinding `br-sandbox-runner` | each (client, mode) | always |
| Role and RoleBinding `br-sandbox-keeper` | each (client, mode), sandbox namespace | `keeper.enabled` |
| Keeper Deployment, ServiceAccount, lease Role, ClusterRole `namespaces` read, NetworkPolicies `default-deny` and `egress-keeper` | system namespace | `keeper.enabled` |
| ServiceAccount, ClusterRole and Role `br-sandbox-netcheck`, ConfigMap `br-sandbox-netcheck`, Job `br-sandbox-netcheck-{run_id}`, CronJob, NetworkPolicy `egress-netcheck` | system namespace and each (client, mode) | `network_check.enabled` |

The system namespace is `{prefix}-system`. It carries the label `br.sandbox/system=true` and not `br.sandbox/managed`, so the Kyverno exclusion does not touch it.

## Layout: two namespaces for a mode with a proxy

A (client, mode) that uses a proxy has two namespaces. Mode `allow` always has them; mode `full` has them when `clients.<client>.modes.full.credential_set` is not empty. Mode `none` and mode `full` without a credential set have the sandbox namespace only.

| | Sandbox namespace `{prefix}-{client}-{mode}` | Egress namespace `{prefix}-{client}-{mode}-egress` |
|---|---|---|
| Holds | the sandbox pods, the warm pool, the CA certificate (not secret) | the proxy pods, the credential Secret, the CA key Secret, the sealed Secrets of the proxy |
| Label | `br.sandbox/managed=true`, `br.sandbox/mode`, `br.sandbox/client` | `br.sandbox/egress=true`, `br.sandbox/mode`, `br.sandbox/client`; never `br.sandbox/managed` |
| Pod Security | `baseline` enforced, `restricted` warn and audit | the same |
| Role and RoleBinding | `br-sandbox-runner`, `br-sandbox-keeper`, `br-sandbox-netcheck` | none |
| Policies | `default-deny`, `egress-allow` or `egress-full` | `default-deny`, `ingress-tunnel`, `egress-proxy` |
| Quota | `br-sandbox-quota` and `br-sandbox-limits` | `br-sandbox-egress-quota` (pods only, `egress.quota.pods`) |
| Admission policy | one per namespace, `role=sandbox` pods only | none |

No identity of the chart (the runner, the keeper, the network check, the sandbox ServiceAccount) has a Role or a RoleBinding in an egress namespace, so none of them reaches a proxy pod by `pods/exec` or `pods` patch, and none reads a Secret there. The controller of the proxy Deployment is the only identity that makes or changes a pod there. The one cluster-wide right on pods is the read (`get`, `list`) of the network check ClusterRole; no identity has a cluster-wide right on `secrets`. A runner namespace that equals an egress namespace fails the render with `BR_SANDBOX_SHARED_NAMESPACE`.

The egress namespace does not carry `br.sandbox/managed=true`, on purpose: the admission policy of a sandbox namespace binds on that label and refuses every pod that is not `role=sandbox`, so it would refuse the proxy pods. The consequence for a Kyverno exclusion (or any policy that selects the managed namespaces): the exclusion text that names `br.sandbox/managed=true` does not cover the egress namespace. The cluster default network policy (the generated policy per namespace with allow rules for DNS, intra-namespace traffic and ingress controllers) also acts in the egress namespace: without an exclusion that selects `br.sandbox/egress=true`, its allow rules add to `ingress-tunnel` and `egress-proxy`, so the statements above (ingress only from the sandbox pods, no cluster DNS) are not what the cluster enforces there. Any other exclusion the project needs there (user namespaces, runtime class) selects `br.sandbox/egress=true` too. The egress namespace has no `hostUsers: false` requirement from the chart; the proxy change sets its own pod security. A consumer that lists namespaces by `br.sandbox/client` or `br.sandbox/mode` also sees the egress namespaces and selects `br.sandbox/managed=true` to get the sandbox ones.

**Where the secrets live.** The credential Secret, the CA key Secret and every sealed Secret of the proxy are created in the egress namespace by the deploy tool; the chart renders none. The CA certificate (not secret) stays in the sandbox namespace, for the sandbox pods: the chart renders it there from `clients.<client>.modes.<mode>.proxy.ca.certificate`, and a copy in the egress namespace for the proxy. The Secrets to create in an egress namespace: the one named by `proxy.ca.key_secret` (item `ca.key`), the one named by `credential_set.secret_ref` (one item per `secrets[].key`), and the one named by `egress.proxy.reload.key_secret` (item `token`). The pull secrets of `pull_secrets` must exist in the egress namespace too: an image pull reads the Secret of the namespace of the pod.

**Name.** `{sandbox namespace}-egress` is a working name. It comes from the helper `br-sandbox-chart.egress.namespace`, the only place that writes the suffix; a change of the pattern is a change of that helper. A name over 63 characters fails the render with `BR_SANDBOX_NAME_TOO_LONG`, and a name equal to another namespace of the render (the system namespace, a sandbox namespace, a runner namespace) fails with `BR_SANDBOX_SHARED_NAMESPACE`. One egress namespace belongs to one (client, mode): two clients never share one.

**What the egress namespace renders.** Its Namespace, its default-deny, its two allow rules and its quota; with `egress.enabled` also the proxy objects, through the slot `br-sandbox-chart.slot.egress.objects`, which receives the root, the namespace item and the name of the egress namespace (`dict "root" . "ns" $ns "namespace" $name`). A proxy Deployment lives in an egress namespace and never in a sandbox namespace. With `egress.enabled: false` no proxy pod exists, so a sandbox of a mode with a proxy has no egress at all (fail closed).

**Rules of the egress namespace.** `default-deny` on every pod, both directions. `ingress-tunnel`: proxy pods take traffic only from the pods `br.sandbox/role=sandbox` of the sandbox namespace of the same (client, mode) (namespace and pod selector in the same item), TCP on `egress.proxy.port`; no other port, so the reload port of the proxy is reached by no sandbox. `egress-proxy`: proxy pods reach `0.0.0.0/0` (and `::/0` with `dual_stack`) except three groups: the internal ranges (`network.internal_cidrs`), the pod and service networks, and the node, node public and load balancer addresses. The rule names no pod and no namespace, and no cluster DNS: no cluster DNS is reachable from the egress namespace, so the proxy resolves names through `network.public_resolvers` (`dnsPolicy: None`).

## Values

Required keys have no default: a missing key stops the render with `BR_SANDBOX_MISSING_VALUE{key=...}`. Keys are snake_case; keys that are Kubernetes fields keep the Kubernetes name (`runtimeClassName`, `runAsUser`, `runAsGroup`, `terminationGracePeriodSeconds`, `imagePullSecrets` names, resource maps). `values.schema.json` has `additionalProperties: false` on every block with fixed keys: a key outside its place fails the render. Maps keyed by a name are checked entry by entry (`clients`, `clients.<client>.modes`, `pool.node_selector` with string values). One object takes any key: `keeper.settings`. All durations are in seconds. This table is the contract for the library `keeper`.

| Key | Default | Meaning |
|---|---|---|
| `installation.namespace_prefix` | required | Prefix of every namespace, at most 22 characters. With a client name of 32 characters at most, a sandbox namespace name stays under 63; the egress namespace of a mode with a proxy adds `-egress` (7 characters) and a name over 63 fails with `BR_SANDBOX_NAME_TOO_LONG`. |
| `installation.system_namespace` | `{prefix}-system` | Keeper and network check. |
| `image` | required | One registry link of the project, with a tag or a digest. The pool and the policy use it. |
| `image_pull_policy` | computed | `Always` for a tag, `IfNotPresent` for a digest. A value overrides it. |
| `registry_mirror` | `""` | Prefix of every image: `{mirror}/{reference}`. |
| `pull_secrets` | `[]` | Names of image pull secrets, put on every pod. |
| `tools.image.{repository,digest}` | required | Image `br-sandbox-tools`, digest only. |
| `tools.mount_path`, `tools.size_limit` | `/opt/sbx`, `64Mi` | Where the agent sees `bin/bsdtar` and `bin/pause`. |
| `tools.init_resources` | 10m/16Mi/8Mi, 100m/64Mi/64Mi | Resources of the init container (cpu, memory, ephemeral-storage). |
| `tools.seed_container.{enabled,name,resources}` | `true`, `seed`, 10m/8Mi/8Mi, 100m/32Mi/32Mi | The seed container, see below. Its resources are not part of `pool.size`. |
| `seed.size_limit` | `256Mi` | Size of the `/skills` volume. |
| `runtimeClassName` | `""` | Passed to the pod; the policy requires the same value. Not measured. |
| `runAsUser`, `runAsGroup` | `10001` | Numeric user of the agent. The image needs no user entry. |
| `restore_timeout_seconds` | `30` | Read by `br-runner-sandbox`. |
| `idle_ttl`, `idle_keep_min_state_bytes` | `120`, `10000000` | Read by the keeper and `br-runner-sandbox`. |
| `warm_max_age_seconds` | `1800` | The keeper deletes a warm pod older than this. |
| `save.ttl_seconds` | required, above 0 | `BR_SANDBOX_SAVE_AGE_INVALID` otherwise. One value for the keeper and `br-runner-sandbox`. |
| `save.paths`, `save.tombstone_ttl_days` | `[/work, /home/agent]`, `365` | |
| `store.{endpoint,bucket,region,path_style,credentials_secret}` | required | Passed to the config. `credentials_secret` names a Secret of the keeper namespace with the keys `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY`; the chart renders no Secret. |
| `work.size_limit`, `home.size_limit` | `3Gi`, `512Mi` | `emptyDir` `sizeLimit` of `/work` and `/home/agent`. No other volume type exists. |
| `pool.size.{cpu,memory,disk}.{request,limit}` | 500m/2, 1Gi/2Gi, 2Gi/5Gi | The one size key of each resource. `disk` is `ephemeral-storage`. |
| `pool.{floor,max_sandboxes,refill_seconds,margin}` | 1, 20, 3, 2 | Read by the keeper. The chart does not apply `floor`: it sets no replica count, so without the keeper each pool runs one pod. |
| `pool.terminationGracePeriodSeconds`, `pool.node_selector` | `1`, `{}` | |
| `quota.extra_pods` | `4` | Pods of the quota = `max_sandboxes` + this (network check probe pod, slack; the proxy pods count in the egress namespace). |
| `network.{pod_cidr,service_cidr}` | required | |
| `network.{node_addresses,node_public_addresses,load_balancer_addresses}` | required lists | `[]` must be written on purpose; no default exists. |
| `network.public_resolvers` | required if a mode is `allow` or `full` | Nameservers of the pods of mode `full`. |
| `network.internal_cidrs` | RFC1918, 100.64/10, 169.254/16, 127/8 | |
| `network.ipv6` | `single_stack` | `dual_stack` adds `::/0` except `fc00::/7`, `fe80::/10` and the IPv6 addresses of the lists. |
| `network.api_server_addresses` | required with the keeper or the network check; at least one | Egress of the keeper and of the network check (API server). |
| `network.store_cidrs` | required with the keeper only; at least one | Egress of the keeper to the store. |
| `clients.<client>.modes.<mode>` | required | `mode` is `none`, `allow` or `full`, else `BR_SANDBOX_MODE_UNKNOWN`. A key present is a namespace. |
| `...modes.<mode>.pool` | `{}` | Overrides any key of `pool` for this namespace. |
| `...modes.<mode>.runner.{service_account,namespace}` | required | Subject of the runner Role. |
| `...modes.<mode>.allow.{domains,cidrs}` | `[]` | For mode `allow`; the `allowlist` transform of the proxy. A non-empty list fails the render with `BR_SANDBOX_EGRESS_DISABLED` while `egress.enabled` is false, and with `BR_SANDBOX_ALLOW_MODE` in a mode other than `allow`. |
| `...modes.<mode>.credential_set.{secret_ref,secrets}` | `{}` | One set per namespace, mode `allow` or `full` (a set on mode `none` fails with `BR_SANDBOX_CREDENTIAL_SET_MODE`, and with `BR_SANDBOX_EGRESS_DISABLED` while `egress.enabled` is false). `secret_ref` names a Secret of the egress namespace; the chart renders no Secret. In mode `allow` the `allowlist` transform comes first, so a bound host must also be named by `allow.domains` or `allow.cidrs`; in mode `full` the allowlist is `*`. Items of `secrets`: `placeholder` (the string the sandbox sends), `env_name` (the sandbox variable that holds it; it must not be a variable that the chart sets: `HOME`, the proxy variables, the names of `egress.ca.env_names`; `BR_SANDBOX_CREDENTIAL_ENV_RESERVED`), `key` (the item of the Secret, a file of the proxy), `hosts` (required, not empty; a name, a pattern such as `*.example.test`, or a CIDR; the entry `*` and a CIDR that ends in `/0` bind the placeholder to every host and fail with `BR_SANDBOX_CREDENTIAL_HOST_WILDCARD`), `match_headers` (default `[Authorization]`), `match_query`, `match_path`, `match_body` (default `false`), `require_tls` (default `true`), `require` (default `false`). |
| `...modes.<mode>.proxy.{cluster_ip,ca.certificate,ca.key_secret}` | required with `egress.enabled` for a mode with a proxy | `cluster_ip`: the fixed IPv4 address of the Service (see Egress proxy); an IPv6 value fails the schema, because the tunnel listener binds `0.0.0.0` only. `ca.certificate`: the PEM of the CA of this (client, mode), not secret; one or more `CERTIFICATE` blocks. A PEM block of another type (a private key) fails the schema; the chart does not parse the base64 inside a `CERTIFICATE` block. `ca.key_secret`: the Secret of the egress namespace that holds the CA key (item `ca.key`). A missing key fails with `BR_SANDBOX_MISSING_VALUE`. |
| `egress.enabled`, `egress.proxy.port` | `false`, `8888` | `enabled: true` renders the proxy objects and the sandbox side. `port` is the tunnel port of both namespaces; it must not be 8080, 8443, 9090 or 9092 (`BR_SANDBOX_PROXY_PORT_CONFLICT`). |
| `egress.proxy.image.{repository,digest}` | the public image of the product, a digest | By digest only: a tag is refused by the schema. The mirror prefix of `registry_mirror` applies. |
| `egress.proxy.{replicas,runAsUser,runAsGroup,resources}` | `1`, `65532`, `65532`, 100m/128Mi and 1/512Mi | One proxy Deployment per egress namespace. `replicas` plus one (the surge) must fit `egress.quota.pods` (`BR_SANDBOX_PROXY_QUOTA`). |
| `egress.proxy.reload.key_secret` | required with a proxy | The Secret of the egress namespace (item `token`) that holds the bearer key of the reload API. The API is enabled in each proxy pod and is reachable only inside that pod (`127.0.0.1:9092`, a constant); nothing in the chart calls it. |
| `egress.proxy.upstream_ca.{config_map,key}` | unset; `key` defaults to `ca.crt` | Optional. `config_map` names a ConfigMap of the egress namespace; the chart renders no such ConfigMap. When set, the chart mounts the key read-only at `/etc/iron-proxy/trust/` in the proxy pod and sets `SSL_CERT_FILE` on the proxy container to that file; when unset nothing is rendered. See Egress proxy. |
| `egress.ca.{mount_path,env_names}` | `/etc/br-sandbox-ca`, five names | Where the sandbox mounts the CA certificate (file `ca.crt`) and the variables that point to it: `SSL_CERT_FILE`, `REQUESTS_CA_BUNDLE`, `NODE_EXTRA_CA_CERTS`, `GIT_SSL_CAINFO`, `CURL_CA_BUNDLE`. |
| `egress.quota.pods` | `4` | Pods of the quota of an egress namespace. |
| `keeper.{enabled,image,replicas,settings,resources}`, `keeper.lease.{duration_seconds,renew_deadline_seconds,retry_period_seconds}` | `false`, required if enabled, `2`, `{}`, 50m/64Mi and 500m/256Mi, 15/10/2 | `settings` goes untouched into the system config; the library `br-sandbox-keeper` adds its keys to the schema. |
| `network_check.{enabled,image,targets,offlist_host,run_id,cron}` | `true`, required if enabled, `[]`, `offlist.invalid`, `""`, `""` | A new `run_id` starts a new Job; an empty `run_id` gives the Job name suffix `initial`. `targets` are `host:port` entries (the bus, the database). `offlist_host` is the name that the CONNECT probe sends in mode `allow`; it must not be on the allow list. |

Render errors: `BR_SANDBOX_MISSING_VALUE`, `BR_SANDBOX_MODE_UNKNOWN`, `BR_SANDBOX_SHARED_NAMESPACE` (two equal names among the system, sandbox and egress namespaces, or a runner namespace equal to an egress namespace), `BR_SANDBOX_NAME_TOO_LONG` (a namespace name over 63 characters), `BR_SANDBOX_SAVE_AGE_INVALID`, `BR_SANDBOX_POOL_INVALID` (floor above `max_sandboxes`), `BR_SANDBOX_EGRESS_DISABLED`, `BR_SANDBOX_CREDENTIAL_SET_MODE`, `BR_SANDBOX_ALLOW_MODE` (an `allow` list in mode `none` or `full`), `BR_SANDBOX_QUANTITY_INVALID` (a resource quantity with an unknown suffix), `BR_SANDBOX_CREDENTIAL_HOSTS_EMPTY`, `BR_SANDBOX_CREDENTIAL_PLACEHOLDER_EMPTY`, `BR_SANDBOX_CREDENTIAL_PLACEHOLDER_DUPLICATE`, `BR_SANDBOX_CREDENTIAL_ENV_DUPLICATE`, `BR_SANDBOX_CREDENTIAL_ENV_RESERVED`, `BR_SANDBOX_CREDENTIAL_HOST_WILDCARD`, `BR_SANDBOX_CREDENTIAL_KEY_DUPLICATE`, `BR_SANDBOX_CREDENTIAL_REQUIRE_TLS` (`require` with an effective `require_tls`; set `require_tls: false` to use `require`), `BR_SANDBOX_PROXY_ADDRESS_DUPLICATE`, `BR_SANDBOX_PROXY_PORT_CONFLICT`, `BR_SANDBOX_PROXY_QUOTA`.

Constants, not keys (a value that weakens one is a different chart): label prefix `br.sandbox`, `hostUsers: false`, seccomp `RuntimeDefault`, the seven capabilities, no service account token, no ingress, no port, Pod Security `baseline` enforced, `restricted` warn and audit, the loopback listeners (including the reload listener `127.0.0.1:9092`) and the mount paths of the proxy pod.

## Slots

Named templates in `templates/_slots.tpl`; the egress slots are filled, the keeper slot renders nothing yet. The pod slots receive `dict "root" . "ns" $ns` and return a YAML list.

| Slot | Called from | Content |
|---|---|---|
| `br-sandbox-chart.slot.egress.env` | the agent container | `HTTPS_PROXY`, `HTTP_PROXY` (and lower case), `NO_PROXY` empty, the CA variables, the placeholders; empty without a proxy |
| `br-sandbox-chart.slot.egress.volumes` | the pod | the CA ConfigMap `br-sandbox-ca` |
| `br-sandbox-chart.slot.egress.volume_mounts` | the agent container | the CA mount, read-only |
| `br-sandbox-chart.slot.egress.objects` | `templates/egress.yaml`, once per egress namespace, with `dict "root" . "ns" $ns "namespace" <egress namespace name>` | proxy config and CA ConfigMaps, Deployment and Service in the egress namespace, the CA ConfigMap in the sandbox namespace; empty while `egress.enabled` is false |
| `br-sandbox-chart.slot.keeper.objects` | `templates/config.yaml` | anything else the keeper needs; renders nothing |

## Egress proxy

One proxy Deployment `br-sandbox-proxy` (label `br.sandbox/role=proxy`, `egress.proxy.replicas`) in each egress namespace, with its Service, the ConfigMap `br-sandbox-proxy-config` (file `proxy.yaml`) and the ConfigMap `br-sandbox-proxy-ca`. The pod is made by the Deployment controller. It has no ServiceAccount token, no Role and no RoleBinding exist in the namespace, and the pod runs with `hostUsers: false`, a numeric non-root user, `drop ALL`, a read-only root file system, seccomp `RuntimeDefault`, `allowPrivilegeEscalation: false`, `dnsPolicy: None` with `network.public_resolvers`, and the requests and limits of `egress.proxy.resources`. The image is `egress.proxy.image` by digest, with the mirror prefix; the pull secrets apply.

**Config of the proxy.** Standalone: the render holds no control-plane token, no tenancy block, no telemetry endpoint, no response-retry handler and no host of the vendor. The built-in DNS server is off. The tunnel listener (CONNECT) is on `egress.proxy.port`; the direct HTTP and HTTPS listeners, the metrics listener and the reload listener (`127.0.0.1:9092`, constants) bind the loopback address of the pod, so no sandbox policy reaches them (and `ingress-tunnel` opens the tunnel port only). TLS mode `mitm`: the CA certificate comes from the ConfigMap `br-sandbox-proxy-ca`, the CA key from the Secret named by `proxy.ca.key_secret`. Transforms in fixed order: `allowlist`, then `secrets` (only with a credential set). Mode `allow`: `allow.domains` and `allow.cidrs`, with or without a credential set. Mode `full` with a credential set: the pattern `*`, so every public name is allowed; the deny list still applies. `upstream_deny_cidrs` is the deny list of the `egress-proxy` policy (internal, pod, service, node, node public and load balancer lists, and the IPv6 ones with `dual_stack`) plus the loopback ranges. There is no response scrub transform.

**The proxy address in the sandbox.** A sandbox resolves nothing (resolver `127.0.0.1`, no DNS rule), so the variables hold an IP: `http://{proxy.cluster_ip}:{egress.proxy.port}`, the fixed `clusterIP` of the Service. The address must be free and inside `network.service_cidr`; the API server refuses the Service otherwise, and a change of the value replaces the Service. Two namespaces with the same address fail with `BR_SANDBOX_PROXY_ADDRESS_DUPLICATE`. The sandbox policy selects the proxy pods, not the address: it relies on the translation of the Service address to a pod address before the policy applies; that is not proved offline.

**CA flow.** The CA certificate is a value (`proxy.ca.certificate`), rendered as the ConfigMap `br-sandbox-ca` in the sandbox namespace and mounted read-only at `egress.ca.mount_path` (file `ca.crt`) in the agent container; the variables of `egress.ca.env_names` point to it. The CA key exists only in the Secret of the egress namespace, mounted in the proxy pod. No sandbox pod has a Secret volume, a `secretKeyRef` or a `secretRef`, and the admission policy refuses all three.

**Credentials.** The sandbox receives the placeholder in the variable `env_name`; the real value is the file `key` of the Secret `credential_set.secret_ref`, mounted read-only in the proxy pod only (source type `file`). The set works in mode `allow` (after the allowlist, so the host must be on the allow list too) and in mode `full` (every public name). Each placeholder is bound to its `hosts`; `require_tls` (default true) keeps a bound value off plain HTTP. `require` makes the proxy answer 403 to a bound host without the placeholder, and does not combine with `require_tls`. Git works over HTTPS with the token as placeholder; git over ssh is not offered. The swap inside a `Authorization: Basic` header is not claimed: the chart renders the rule and the offline suite cannot prove how the product treats that header.

**Limit.** A bound host that sends a credential value back shows it to the sandbox (no scrub exists; the proxy has no response scrub). Measured on the pinned proxy image: a response body that echoed the `Authorization` header carried the real value to the client. The chart claims nothing else about credentials.

**Rotation and config changes.** A change of the proxy config (an allow list, the deny list, a credential rule) or of the CA certificate of a (client, mode) changes an annotation of the pod template (`br.sandbox/proxy-config-sha256`, `br.sandbox/proxy-ca-sha256`: the SHA-256 of the rendered config and of the CA certificate), so `helm upgrade` rolls the proxy pods. The chart makes no checksum of content that it does not render: a change of the content of the credential Secret, of the CA key Secret or of the ConfigMap of `egress.proxy.upstream_ca` does not change the pod template and does not roll the pods. A Secret volume updates its files in the running pod, and the proxy re-reads a `file` source at a reload call (`POST /v1/reload`, bearer key from `egress.proxy.reload.key_secret`); for any other content the chart claims a restart only. The project rolls the pods itself, by a restart of the Deployment or by the reload call, through its own gesture. The reload listener is `127.0.0.1:9092` in the pod: only a caller inside the pod reaches it, and nothing in the chart calls it (no identity of the chart has `pods/exec` in the egress namespace). The Secret `egress.proxy.reload.key_secret` exists because the product needs a key when its API is on.

**Trust of the proxy for upstream servers.** The proxy trusts the system roots of its image. With `egress.proxy.upstream_ca` set (`config_map`: a ConfigMap of the egress namespace; `key`, default `ca.crt`), the chart mounts that key read-only at `/etc/iron-proxy/trust/` and sets `SSL_CERT_FILE` on the proxy container to the file. The project creates the ConfigMap in each egress namespace (a missing ConfigMap keeps the pod from starting); the file holds certificates, not secrets. Measured on the pinned proxy image (0.61.0): the roots of the file are added to the roots of the certificate directory of the image (`/etc/ssl/certs`); they do not replace them. A certificate authority placed only in that directory stayed trusted while `SSL_CERT_FILE` named another file; a private authority that was in neither was refused (`x509: certificate signed by unknown authority`); the file alone is enough to trust a private authority, and the public roots of the image directory stay trusted. Nothing was measured about the default bundle file of the image when `SSL_CERT_FILE` is set. Unset, nothing is rendered.

**Targets named by a name.** Measured on the pinned proxy image (0.61.0) with the config of this chart: a client sends `CONNECT git.example.test:443` with the server name `git.example.test`; the proxy resolves the name with the OS resolver of the proxy pod (`network.public_resolvers`, `dnsPolicy: None`), dials the address, and swaps the placeholder in the `Authorization` header with the value of the credential file (the audit line of the proxy says `swapped ... header:Authorization`). A project that wants a private test server needs a DNS that answers its names (a resolver of `network.public_resolvers` that knows them) and a certificate authority that the proxy trusts (`egress.proxy.upstream_ca`).

**Targets named by an IP literal.** An `allow.cidrs` entry allows the `CONNECT` to an address: the render puts the entries of `allow.cidrs` into the `allowlist` transform beside the domains, and the render needs no name for it (the DNS server of the proxy is off). TLS to an IP literal does not work. Measured on the pinned proxy image (0.61.0): the proxy needs a name in the TLS server name (SNI) to make a leaf certificate, and clients send no SNI for an IP, so the proxy logs `TLS handshake for <ip>:443: no SNI provided` and the client gets a TLS internal error. Plain HTTP to an IP literal is not claimed: the sandbox checks do not use it. Use a name. The deny list of the policy is rendered into `upstream_deny_cidrs`; a test server needs an address outside it (narrow `network.internal_cidrs` for it; the proxy deny list follows the policy list).

**Not proved offline.** The suite renders and reads objects. It does not start the proxy: that the product accepts this config (a `*` pattern, an empty allow list, the listener addresses), that it checks the deny list at connect time for an address, that it makes no call to its vendor, the swap of a placeholder, the 403 and 502 answers, the translation of the Service address, the user namespace of the proxy pod, the Secret file modes, and the CEL of the admission policy are checked by the cluster tests. The behaviours measured above (name target, IP literal over TLS, the swap in the `Authorization` header, an echoed value) were measured once by hand on the pinned image; the suite does not start the proxy.

## Modes

| Mode | NetworkPolicies of the namespace | Pod DNS |
|---|---|---|
| `none` | `default-deny` only (ingress and egress, every pod) | `dnsPolicy: None`, nameserver `127.0.0.1` |
| `allow` | `default-deny` and `egress-allow`: sandbox pods reach only the pods `br.sandbox/role=proxy` of the egress namespace of the same (client, mode), TCP on `egress.proxy.port`; namespace and pod are selected in the same rule item; no DNS | same as `none` |
| `full` | `default-deny` and `egress-full`. With a credential set: the same rule as mode `allow`, nothing else, and the pod resolver is `127.0.0.1`. Without: `0.0.0.0/0` except internal, pod, service, node, node public and load balancer lists; with `dual_stack` also `::/0` except `fc00::/7`, `fe80::/10` and the IPv6 addresses. No rule names a pod or a namespace, so no cluster DNS | nameservers `network.public_resolvers` |

No policy of a sandbox namespace or of the system namespace has an ingress rule; the only ingress rule of the chart is `ingress-tunnel` of an egress namespace. The sandbox side selects `br.sandbox/role=sandbox`, never the pool label, so a claimed pod keeps its path. The only egress rule of a sandbox in a mode with a proxy leads to the proxy pods, so a sandbox has no other path (fail closed). A tool can send DNS to any public host in mode `full`; only the pod resolver is fixed.

A bound host that sends a credential value back shows it to the sandbox (no scrub exists; the proxy has no response scrub). The chart claims nothing else about credentials.

## Label contract

One prefix, `br.sandbox`. The adapter, the keeper and the policies use exactly these names.

| Name | Kind | Set by | Value |
|---|---|---|---|
| `br.sandbox/managed` | namespace label | chart | `true`; the label of the Kyverno exclusion and of the policy binding |
| `br.sandbox/egress` | namespace label | chart | `true` on an egress namespace only; never together with `br.sandbox/managed` |
| `br.sandbox/mode`, `br.sandbox/client` | namespace label | chart | mode, client (sandbox and egress namespaces) |
| `br.sandbox/role` | pod label | pod template | `sandbox` in a sandbox namespace, `proxy` in an egress namespace (set by the proxy Deployment); never changes |
| `br.sandbox/pool` | pod label | template, then claim | `warm`, then `claimed` (one way); `probe` for a check pod |
| `br.sandbox/pool-id` | pod label | template | `{client}-{mode}`; never changes |
| `br.sandbox/thread`, `br.sandbox/agent`, `br.sandbox/creds` | pod label | claim | ids |
| `br.sandbox/clean`, `br.sandbox/claimed-at`, `br.sandbox/idle-since`, `br.sandbox/state-bytes`, `br.sandbox/state-turn`, `br.sandbox/credsets` | pod annotation | `br-runner-sandbox` | |
| `br.sandbox/proxy-config-sha256`, `br.sandbox/proxy-ca-sha256` | pod annotation | chart (proxy pod template) | SHA-256 of the rendered proxy config and of the CA certificate; a change rolls the proxy pods |
| `br.sandbox/system` | pod label | chart | `keeper` or `netcheck`, in the system namespace |
| `br.sandbox/system` | namespace label | chart | `true` on the system namespace only |

The selector of the ReplicaSet is `pool=warm` and `pool-id`. The ReplicaSet has no `spec.replicas`, on purpose: the keeper is the only writer of the replica count, because a value rendered by the chart would be reset at each `helm upgrade`. A pool starts with one replica (the ReplicaSet default) until the keeper runs. The `floor` value is read by the keeper; the chart does not apply it.

## The pod

Agent container `agent`: the project image, command `{tools.mount_path}/bin/pause`, user `runAsUser`, env `HOME=/home/agent`, volumes `tools` (read-only), `/work`, `/home/agent`, `/skills` (read-only). Init container `tools-init` (tools image): `/sbx/bin/bsdtar -x -p --no-same-owner -f /sbx/tools.tar -C {tools.mount_path}`. All containers drop ALL and add `CHOWN, DAC_OVERRIDE, FOWNER, FSETID, SETUID, SETGID, KILL`. Volumes are `emptyDir` with a size limit (a `configMap` only for the check pod).

**Seed container (contract for the library `br-runner-sandbox`).** Name `seed` (key `tools.seed_container.name`), image `br-sandbox-tools`, command `/sbx/bin/pause`, root user inside the user namespace, `readOnlyRootFilesystem`, no port, no shared process namespace, same seven capabilities. The library `br-runner-sandbox` writes the seed through `pods/exec` into this container, mount path `/skills`, program `/sbx/bin/bsdtar` (in the agent container the same program is `{tools.mount_path}/bin/bsdtar`, default `/opt/sbx/bin/bsdtar`). The agent mounts `/skills` read-only, so a non-root agent cannot change the seed. `tools.seed_container.enabled: false` removes the container; then `/skills` is writable by the agent and the seed is only the weak form. Its resources are small keys of their own: whether they count in the size of the pool is open.

## Runner role

The Role `br-sandbox-runner` of each sandbox namespace is the contract for the adapter. It binds the `runner.service_account` of `runner.namespace`. Rules: `pods` get, list, watch, patch, create, delete; `pods/exec` create; `replicasets` get, list, watch (the adapter reads the condition of the pool); `configmaps` get on `br-sandbox-config` only (the adapter reads the config). The keeper Role of the same namespace (with `keeper.enabled`) has `pods` get, list, watch, delete; `replicasets` get, list, watch, patch; `replicasets/status` patch; `events` create. It has no `pods` patch. No identity of the chart has a Role in an egress namespace.

## Admission policy

One policy and binding per (client, mode); values (size, runtime class, repositories) are inlined. The binding matches the namespace by `br.sandbox/managed=true` and by name. It matches every pod, `CREATE` and `UPDATE`, and fails closed.

- A pod passes if it is `role=sandbox` and follows the rules below. Any other role, or none, is refused (`BR_SANDBOX_ROLE`), `role=proxy` included: a proxy pod does not live in a sandbox namespace, it lives in the egress namespace, which the policy does not bind. The role cannot change (`BR_SANDBOX_ROLE_IMMUTABLE`).
- Rules of a sandbox pod: 1 `hostUsers` false (`USERNS_REQUIRED`); 2 no host network, PID, IPC, port (`HOST_NAMESPACE`); 3 only `emptyDir` and `configMap` volumes, no `secretKeyRef`, no `secretRef` (`VOLUME_FORBIDDEN`); 4 drop ALL, add only the seven (`CAPABILITY`); 5 no privilege, no escalation, seccomp `RuntimeDefault` on the pod and, when a container sets a profile, on that container (`PRIVILEGE`); 6 cpu, memory and ephemeral-storage requests and limits on every container, not above the size of the pool (`RESOURCES`); 7 every image from an allowed repository (project image, tools image, check image, each with the mirror) with a tag or a digest (`IMAGE`); 8 no token, runtime class equals the chart value (`TOKEN_OR_RUNTIME`); 9 on update the security contexts, resources, volumes and `hostUsers` stay (`IMMUTABLE`).
- Patch rules (`BR_SANDBOX_LABELS`): a patch changes only `br.sandbox/` keys of labels and annotations; `role` and `pool-id` never change; a claimed pod never leaves `claimed`.
- Init and seed containers are containers of the pod and follow rules 4 to 7.

The refusals of an unlabelled pod, of a `role=proxy` pod and of a relabel of the role, with the runner service account, in a sandbox namespace, are proved by a cluster test. That test is not part of the offline suite of this chart and has not been executed at this version. The CEL of the admission policy is not compiled offline, so at this version it has been neither compiled nor executed on a cluster.

## Image contract

**The agent image.** Any Linux image with `/bin/sh`, `cat` and `find`, for the architecture of the nodes. Nothing else: no agent binary, no `tar`, no user entry, no `ps`. The tools that the sandbox needs (`bsdtar`, `pause`) come from `br-sandbox-tools`. The project puts one registry link in `image`. Example: `br-sandbox-python` (Python 3.12, pandas, openpyxl, python-docx, python-pptx, LibreOffice), the test and reference image of this repository only; each project defines its image.

**A new push under one tag.** A tag pulls at each pod creation (`imagePullPolicy: Always`). A warm pod is used one time, so the pool takes the new image as it refills, and the keeper deletes a warm pod older than `warm_max_age_seconds` (default 1800). A quiet pool has the new image in 30 minutes at most. A digest is advised for production; with a digest the pull policy is `IfNotPresent`.

**The check image.** `network_check.image` needs `sh`, `kubectl`, `nc` and the POSIX tools `cat`, `sed`, `tr`, `mktemp`. It runs the check Job and the check pods. No image of this repository has them; the project or the cluster test sets the value.

## Network check

The Job `br-sandbox-netcheck-{run_id}` (and an optional CronJob) lists the nodes, services and pods through the API, builds targets (the `targets` values, a ClusterIP, a pod IP, each node address on 10250 and 6443, the API service, the cluster DNS service, a `.svc.cluster.local` name), and probes from the Job pod (the control) and from a probe pod of each namespace. The probe pod is made from the pool pod template (no token, root inside the user namespace). One JSON object per probe: `{net, mode, target, kind, expected, observed, control}`; `observed` is `refused`, `reached` or `no_result`; `control` is `ok` or `control_unavailable`. The Job fails on any `reached`, any `control_unavailable`, a missing result, a node address that is not in the three node lists, or a target class with no target. The classes are `targets` (at least one entry of `network_check.targets`), `node_kubelet`, `node_api`, `api_service`, `dns_service`, `cluster_name` and `pod` (a pod IP with a declared container port); a missing class is one object with `kind` `target_class` and `observed` `missing`. For a (client, mode) with a proxy the probe pod also sends `CONNECT` through the proxy, to each target and, in mode `allow`, to `network_check.offlist_host`; when the proxy answers the `CONNECT` with 200, the probe sends a plain HTTP request `GET /` inside the tunnel on the same connection and reads its status line and body. Measured on the pinned proxy image (0.61.0) with the config of this chart: in mode `allow` a target that is not on the list is refused at the `CONNECT` with 403; in mode `full` with a credential set (allow list `*`) the proxy answers the `CONNECT` with 200 for any target, a target of the deny list included, and applies the deny list when it dials, on the first request inside the tunnel: that request gets 502 with the body `bad gateway`, and no packet leaves for the address. A name that resolves to an address that does not answer gives the same 502. The verdict of a probe is `refused` when the `CONNECT` answers 403 or 502, or when the request inside the tunnel gets an answer 502 with the body `bad gateway`; `reached` when the request inside the tunnel gets any other HTTP answer; `no_result` otherwise (no answer, a timeout, a malformed answer, or a tunnel that answers 200 and then gives no answer to the request). One object per probe with `kind` `connect` and `control` `not_applicable` (only the proxy sends these refusals, so a refusal is its own control). The probe has no positive control: a proxy that refuses everything (a broken allow list) also passes the off-list check, so that a listed host is reached is checked by the cluster tests, not by this Job. The pod targets leave out the pods labelled `br.sandbox/role=proxy`.

The network check identity reads the pod specs of the whole cluster (a cluster-wide `get` and `list` on `pods`, `nodes`, `services` and `endpoints`) to pick its targets; its rights to create and delete pods and to read pod logs are bound in the sandbox namespaces only.

## Why

| Thing | Why it is the way it is |
|---|---|
| Namespace per (client, mode) | A mode cannot mix; the namespace checks hold by the namespace and not by a label. |
| The policy matches every pod | A pod without `role=sandbox` would escape and a runner with `pods create` could mount a Secret. |
| Proxy pods and their Secrets in a second namespace | The runner holds `pods/exec` and `pods` patch in the sandbox namespace; a proxy pod there would let a compromised runner read the credential value and the CA key. No identity of the chart has a right in the egress namespace. |
| Egress namespace without `br.sandbox/managed` | The admission policy of a sandbox namespace accepts `role=sandbox` pods only; it must not bind the proxy namespace. |
| Policy per namespace | The size and the repositories are inlined, so a pool size is a single key. |
| `hostUsers: false` fixed | Root in the pod is a high host user; the seven capabilities hold only inside. |
| `pause` from the tools mount | The agent image has no reaper; an orphan stayed alive after SIGTERM with `sleep infinity`. |
| Seed in a second container | `pods/exec` cannot choose a user; a root writer is needed so that a non-root agent sees a read-only seed. |
| No `spec.replicas` | The keeper is the only writer of the replica count; a value rendered by the chart would be reset at each `helm upgrade`. A pool starts with one replica (the ReplicaSet default) until the keeper runs; `floor` is read by the keeper, not applied by the chart. |
| Quota = size x (`max_sandboxes` + `extra_pods`) + seed container resources x `max_sandboxes` (when `tools.seed_container.enabled`) | The refill must not be refused: each sandbox pod holds the agent and the seed container, and the extra pods hold one size each. The init container does not add: a pod counts the larger of its init containers and the sum of its containers. |
| Fixed `clusterIP` for the proxy | A sandbox has no DNS, so the address in `HTTPS_PROXY` must be known at render time; an address that the cluster assigns is not. |
| Loopback listeners, one open port | Only the tunnel port is open to a sandbox; the reload, metrics and direct listeners must be reached by no pod. |
| Pattern `*` in mode `full` with a credential set | The proxy is needed for the swap; names stay open as in mode `full`, and the deny list closes the internal ranges. |
| Selects `role`, never `pool` | A claim changes `pool`; the network path must not move. |
| `default-deny` in the system namespace only with keeper or check | An empty chart adds no policy there. |
| `egress-netcheck` allows all egress | The check must try to reach internal targets; the Job holds a read role, so it runs on demand. |
| Mirror is a prefix | `{mirror}/{reference}`; a mirror that rewrites the host is done by the mirror itself. |
| Label rule covers annotations | A runner may patch only `br.sandbox/` keys; a CNI that annotates the main pod resource would need an exception (to check on a cluster; no such check has been executed at this version). |

## Images and licences

| Image | Used for | SPDX of its content |
|---|---|---|
| `br-sandbox-tools` | init and seed container | `bsdtar` (libarchive) BSD-2-Clause; musl MIT; `pause` of ours Apache-2.0; GCC runtime code statically linked GPL-3.0-or-later WITH GCC-exception-3.1 |
| `br-sandbox-python` | test and reference agent image of this repository | Python PSF-2.0; pandas BSD-3-Clause; openpyxl MIT; python-docx MIT; python-pptx MIT; LibreOffice MPL-2.0 (its repository holds the table) |
| `br-sandbox-keeper` | keeper | Apache-2.0 (ours) |
| check image | check Job | chosen by the project |
| proxy image | egress proxy, runs in the egress namespace | Apache-2.0 (iron-proxy) |

The chart itself is Apache-2.0 and ships no GPL program. Kubernetes (Apache-2.0), Helm (Apache-2.0) and Kyverno are not shipped.

## Needs of the cluster (not done by the chart)

`kubelet` in `/etc/subuid` and `/etc/subgid` on each node; the Kyverno exclusion for namespaces with `br.sandbox/managed=true` (without it mode `none` is not possible) and for namespaces with `br.sandbox/egress=true` (the default network policy of the cluster would add its allow rules there, and the egress namespace would no longer follow `ingress-tunnel` and `egress-proxy`); the bucket with its lifecycle rule; the sealed Secret of `store.credentials_secret`. Each Secret named in `pull_secrets` in every sandbox namespace and in the system namespace, and in every egress namespace (an image pull reads the Secret of the namespace of the pod; the chart renders no Secret). Policy objects need Kubernetes 1.30 or later (ValidatingAdmissionPolicy v1).

## Tests

`bash .github/scripts/test-sandbox-chart.sh` (offline: `helm template`, `yq`, `jq`) proves the checks that need no cluster. The cluster-test rows (the CEL compiles and refuses each bad pod, the three tests per mode, the namespace-to-namespace test, the userns, the label-to-policy delay) are not part of this suite and have not been executed at this version; they are to be executed on a test cluster.
