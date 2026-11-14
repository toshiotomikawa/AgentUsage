# Security policy

## Supported versions

Only the latest code on `main` is supported. Fixes are not backported.

## Reporting a vulnerability

Please do not open a public issue for a security problem. Report it privately through
GitHub instead: on this repository's **Security** tab, choose **Report a vulnerability**.

Include what you found, the steps to reproduce it, and the impact you expect. You will get an
answer as soon as possible, and credit in the fix if you want it.

## Scope

AgentUsage reads the login files that each agent's CLI keeps on your machine and sends those
tokens only over HTTPS to that vendor's own service. Reports about how it handles those
credentials, the local snapshot it serves on `127.0.0.1`, or the programs it runs are
especially welcome.
