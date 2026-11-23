# Code signing policy

Windows releases of DNS Switcher (`dns_switcher.exe` and `DNS-Switcher-Setup-<version>.exe`) are built by [GitHub Actions](.github/workflows/release.yml) on GitHub-hosted runners, from this public repository only.

Releases are currently **not code-signed**. The project applied to [SignPath Foundation](https://signpath.org) in November 2026 and was asked to apply again once it has wider public adoption. When signing is granted, signed releases will state: "Free code signing provided by [SignPath.io](https://about.signpath.io), certificate by [SignPath Foundation](https://signpath.org)."

Until then, every release lists SHA-256 hashes, and from 0.1.2 on every file is built by GitHub Actions and carries a [build provenance attestation](https://docs.github.com/actions/security-for-github-actions/using-artifact-attestations) you can check:

```powershell
gh attestation verify DNS-Switcher-Setup-0.1.2.exe -R Devzinh/dns-switcher
```

## Team roles

| Role | Members | Responsibility |
|---|---|---|
| Authors | [Devzinh](https://github.com/Devzinh) | Commit to the repository |
| Reviewers | [Devzinh](https://github.com/Devzinh) | Review every change from people who aren't authors before it's merged |
| Approvers | [Devzinh](https://github.com/Devzinh) | Approve each signing request, one per release |

All members use multi-factor authentication on GitHub and SignPath.

Only binaries built from this repository's source by the release workflow are submitted for signing. No third-party binaries are signed.

## Privacy

This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.

In practice that means:

- **Find fastest** and the measurement columns send DNS queries (for `www.google.com`, a random name under `google.com`, and `id.server`) to the resolvers on screen, and only when you click it.
- Applying a DNS server changes your network adapter's settings, which is the point of the app. From then on your computer sends its DNS lookups to the server you picked, under that operator's privacy policy.
- If automatic updates are on (an option in setup and in the notification-area menu), the service asks `api.github.com` for this repository's latest release every six hours and downloads it from `github.com` when it's newer. **Check for updates** does the same once. Nothing else is sent.
- There is no telemetry and no account.

## What the installer changes

The setup program, which asks for administrator rights once:

- installs the app to `Program Files\DNS Switcher`, with a Start menu entry;
- registers and starts **DNS Switcher Service** (`services.msc`), which changes DNS settings on the app's behalf;
- if you keep the option checked, adds `DNS Switcher` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` to start the app in the notification area when you sign in;
- stores the automatic-update choice as `AutoUpdate` under `HKLM\SOFTWARE\DNS Switcher`. Updates are installed only when their hash list is signed with the release key (see the README's Updates section).

Uninstall from **Settings → Apps**. Uninstalling stops and removes the service and the sign-in entry. It doesn't change your current DNS: click **Automatic** first to go back to your provider's DNS.
