# Security Policy

## Supported Versions

LysineOS is currently in early development (Phase 0). Security support will be
formally established once the first stable release is available.

| Version | Supported |
| ------- | --------- |
| < 1.0   | Development only -- no security guarantees |

## Reporting a Vulnerability

We take security vulnerabilities seriously. If you discover a security
vulnerability in LysineOS, please report it responsibly.

**Do not** report security vulnerabilities through public GitHub issues.

Instead, please report them via:

- **Email**: security@lysine-os.org
- **GitHub Security Advisory**: Use the
  [Security Advisories](https://github.com/lysine-os/lysine-os/security/advisories/new)
  feature on GitHub

Please include the following information:

- Type of vulnerability (e.g., buffer overflow, privilege escalation, injection)
- Full paths of source files related to the vulnerability
- Step-by-step instructions to reproduce
- Potential impact of the vulnerability
- Any possible mitigations you have identified

We will acknowledge your report within 48 hours and aim to provide a detailed
response within 7 days.

## Security Architecture Highlights

LysineOS is designed with security as a core principle:

- **Memory safety**: Core components written in Rust
- **Build sandboxing**: `membrane` sandbox uses Linux namespaces, cgroups, and
  seccomp
- **Package integrity**: SHA-256 content addressing and GPG/Minisign signatures
- **System snapshots**: Btrfs-based `mitosis` snapshots enable atomic rollback
- **Wayland security**: Native Wayland protocol provides client isolation

## Build Security

- All packages are built in isolated sandboxes (`membrane`)
- Source tarballs are verified via SHA-256 hashes
- Packages support GPG/Minisign signature verification
- Build environments are reproducible
