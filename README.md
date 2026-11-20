# br-jobs-runner-kit

> [!IMPORTANT]
> **This repository is maintained for BotResources and its authorized clients.**
> It is published under Apache-2.0 and made available read-only for visibility and
> reuse. The Apache-2.0 license governs your rights to use, modify, and fork the code;
> the rest of this notice describes our operational stance, not a legal
> restriction.
>
> **We do not accept external pull requests, issues, or support requests.**
> Issues and Discussions are disabled. PRs from accounts that are not on the
> internal contributor allowlist will be closed without review. Forks are
> permitted by Apache-2.0 and we do not (and cannot) prevent them; we simply do not
> monitor, support, or accept contributions from forks outside the BR
> commercial relationship.
>
> - Clients with a commercial relationship: contact your BR account manager.
> - Security reports: see [SECURITY.md](SECURITY.md) (private email channel).
> - This is not a community-supported project. No support is provided through
>   GitHub.

Harness library for building runners compatible with
[svc-jobs](https://github.com/BotResources/svc-jobs).

Multi-crate workspace. Each registered library has its own version and is
released under the tag `{library}/v{version}`; `br-jobs-runner-core` and
`br-jobs-runner-kit` are frozen at 0.1.2 (tag `v0.1.2`). Apache-2.0 — see
[LICENSE](LICENSE).
