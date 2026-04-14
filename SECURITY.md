# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | Yes       |

## Reporting a Vulnerability

If you discover a security vulnerability in modde, please report it responsibly:

1. **Do not** open a public issue
2. Email the maintainer directly or use Codeberg's private reporting feature
3. Include a description of the vulnerability, steps to reproduce, and potential impact

We will acknowledge receipt within 48 hours and aim to release a fix within 7 days for critical issues.

## Scope

modde handles:
- Nexus Mods API keys (stored via system keyring or sops-nix)
- Local filesystem operations (symlinks, file copies)
- Network requests to mod hosting services

Security-relevant areas include API key storage, archive extraction (zip-slip prevention), and symlink handling (path traversal prevention).
