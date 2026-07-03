# Security Policy

## Reporting a Vulnerability

Do not open a public issue for a suspected vulnerability.

Use the repository security advisory flow when it is available. If that flow is unavailable, contact the maintainers privately and include:

- a clear description of the vulnerability
- affected versions or commit ranges
- reproduction steps or a proof of concept
- suggested mitigations if you have them

Please allow time for investigation and coordinated remediation before public disclosure.

## Supported Versions

During the release-candidate period, `1.0.0-rc.x` receives security fixes.
After the final `1.0.0` release, the latest released v1 minor line is
supported. The v0 line receives no fixes after final v1 availability.

DOCX files and configured S3 endpoints are trusted-input boundaries for
`v1.0.0-rc.1` because their transitive `quick-xml` versions have known
resource-exhaustion advisories. Do not expose these inputs as an
unauthenticated parsing service. This restriction will be removed when the
upstream dependency constraints permit `quick-xml 0.41.0` or newer.
