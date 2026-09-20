# Security policy

## Supported versions

The latest released minor version receives security fixes.

## Reporting a vulnerability

Report privately through
[GitHub security advisories](https://github.com/OWNER/typesafe-api-rs/security/advisories/new).
Please do not open a public issue for a vulnerability.

Expect an acknowledgement within three working days and an assessment within
ten. Credit is given in the advisory unless you prefer otherwise.

## Scope

This crate is an HTTP client. The things most worth reporting:

- an API key reaching logs, error messages, panic output, or `Debug` formatting;
- TLS verification being weakened or bypassed;
- a response body being able to cause unbounded memory use or a panic;
- a dependency advisory this project has not picked up.

Vulnerabilities in the TypeSafe API itself belong upstream, at
<https://typesafe.ai>.
