# Code signing policy

Windows releases of DNS Switcher (`dns_switcher.exe` and `DNS-Switcher-Setup-<version>.exe`) are built by [GitHub Actions](.github/workflows/release.yml) on GitHub-hosted runners, from this public repository only.

Free code signing provided by [SignPath.io](https://about.signpath.io), certificate by [SignPath Foundation](https://signpath.org).

Releases are not signed yet: the project is applying to SignPath Foundation, and signing starts with the first release built after approval. Releases up to and including 0.1.1 are unsigned. Every release, signed or not, lists SHA-256 hashes, and files built by GitHub Actions carry a [build provenance attestation](https://docs.github.com/actions/security-for-github-actions/using-artifact-attestations) you can check:

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
- There is no telemetry, no update check and no account.

## What the installer changes

The setup program, which asks for administrator rights once:

- installs the app to `Program Files\DNS Switcher`, with a Start menu entry;
- registers and starts **DNS Switcher Service** (`services.msc`), which changes DNS settings on the app's behalf;
- if you keep the option checked, adds `DNS Switcher` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` to start the app in the notification area when you sign in.

Uninstall from **Settings → Apps**. Uninstalling stops and removes the service and the sign-in entry. It doesn't change your current DNS: click **Automatic** first to go back to your provider's DNS.
