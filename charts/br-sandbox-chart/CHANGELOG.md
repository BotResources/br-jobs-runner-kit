# Changelog

## 0.1.0

First release. The chart of the sandbox: one installation per cluster, one namespace per (client, mode).

- Per (client, mode): Namespace (Pod Security `baseline` enforced, `restricted` warn and audit), ResourceQuota (size times `max_sandboxes` + `quota.extra_pods`, plus the resources of the seed container of each sandbox when it is enabled), LimitRange, ServiceAccount `sandbox` without token, the warm pool (ReplicaSet without `spec.replicas`), NetworkPolicies, admission policy and binding, runner Role, ConfigMap `br-sandbox-config`.
- A container that sets its own `seccompProfile` must set `RuntimeDefault`: the pod-level profile alone did not stop a container-level `Unconfined` or `Localhost` profile.
- Modes `none`, `allow`, `full`. `none` has only `default-deny`. `full` opens the public addresses except every internal list. `allow` opens only the pods `br.sandbox/role=proxy` of the namespace on the proxy port. The proxy itself is not rendered yet.
- The admission policy matches every pod of a managed namespace. A pod passes if it is `role=sandbox` and follows rules 1 to 9 and the label rules, or `role=proxy` and created by the user named in `admission.proxy_controller_user`.
- The sandbox pod has an init container that copies `bsdtar` and `pause` from `br-sandbox-tools`, and a second root container `seed` (key `tools.seed_container.enabled`) that writes `/skills`, which the agent mounts read-only.
- Once per installation: system Namespace, keeper Deployment, Roles and NetworkPolicies (when `keeper.enabled`), the network check Job and its ClusterRole (when `network_check.enabled`).
- Empty extension points for the egress proxy and the keeper objects: the named templates `br-sandbox-chart.slot.egress.*` and `br-sandbox-chart.slot.keeper.objects`. They render nothing.
- `egress.enabled: true` fails the render until a later change of this chart fills its slots.
- No Secret and no bucket is rendered. No object of the chart says that a response is scrubbed: the proxy has no response scrub.
