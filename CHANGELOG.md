# Changelog

## 0.1.3 — 2026-11-24

Improvements from feedback by the r/brdev community.

### Measurement

- **Your provider's DNS is measured too.** The DNS your router hands out (from DHCP) is now tested alongside the public resolvers. When it wins, the app recommends keeping it (automatic) instead of switching.
- **Answer quality, not only speed.** New **CDN** column: each resolver is asked for three CDN hostnames whose answer depends on where the CDN thinks you are (`www.akamai.com`, `www.microsoft.com`, `www.amazon.com`), and the time to connect to the returned servers is measured. A resolver that answers fast but sends you to another region or country no longer wins: ranking uses lookup time plus this connect time.

### Reliability

- **Applying DNS is all or nothing.** Primary and secondary are two separate `netsh` steps; if the second failed, the adapter used to be left half-configured. Now the previous servers are read first, the result is read back afterwards, and on any failure the adapter is restored exactly as it was.
- The same address as primary and secondary is treated as "no secondary" instead of failing.
- The current DNS is read from the registry instead of `netsh` text, so it no longer depends on the Windows display language.

### Local networks

- When your network announces a local domain (for example `lan` or `home`), applying a public DNS now warns that local names may stop resolving, and how to undo it.

## 0.1.2 — 2026-11-23

### Automatic updates

- The DNS Switcher service can keep the app up to date on its own: it checks this repository's latest release every six hours and installs newer versions silently, without a UAC prompt. It's an option in setup (on by default) and in the notification-area menu (**Update automatically**), and **Check for updates** checks right away.
- An update installs only if its hash list carries a valid Ed25519 signature from the release key, the installer matches the signed hash, and the version is newer than the installed one. Anything else is discarded.
- If the tray icon was open, it opens again after the update.

### Fixed

- `dns_switcher.exe` no longer needs the Visual C++ Redistributable. It used to depend on `VCRUNTIME140.dll`, so on a Windows install without it the app couldn't start, the service couldn't run, and a silent install stopped at a "DLL not found" dialog. The C runtime is now linked into the executable.
- After a silent install or upgrade (for example through winget), the notification-area icon starts again right away instead of at the next sign-in.

### Build

- First release built by GitHub Actions from the public repository, with a build provenance attestation for every file (`gh attestation verify <file> -R Devzinh/dns-switcher`).

## 0.1.1 — 2026-11-22

### Measurement

- Each resolver now gets 5 cached lookups instead of one, reported as median, p95, jitter and loss. The fastest is picked by median among servers with at most 20% loss, with p95 as the tiebreak.
- New **Cold** column: one lookup of a random, never-cached name, so the resolver has to ask the authoritative servers. This is the delay you feel on sites you visit for the first time.
- New **Site** column: which anycast location answered, from a CHAOS TXT `id.server` query (Cloudflare, Quad9, Level 3 and others report it).
- Replies are now checked for the matching transaction id and the response flag, so a stray datagram can't be counted as an answer.
- Servers are measured 6 at a time instead of one after another. Probing every server at once caused false packet loss on home routers.
- In simple mode, **Find fastest** now tests the five servers on screen and shows each one's time next to it.

### Installer, service and tray

- New setup program (`DNS-Switcher-Setup-0.1.1.exe`): installs to Program Files with a Start menu entry and an uninstaller, in English or Portuguese.
- New **DNS Switcher Service** (`services.msc`) does the privileged work. The app now runs with normal user rights and never shows a UAC prompt.
- The service only accepts requests from this computer, re-validates every address and adapter, and runs `netsh`/`ipconfig` from System32 directly, never through a shell. Adapters are addressed by interface index.
- New notification-area icon: closing the window keeps the app there, with quick switching, Automatic, Find fastest, Flush DNS and **Start with Windows**.
- `dns_switcher.exe` now carries version information (product, publisher, version, copyright).

### App

- The window shows which DNS the selected adapter is using right now.
- Without the service, `dns_switcher.exe` still works as a portable app when started as administrator.
- Larger window so the new columns fit.
- Removed Norton ConnectSafe (all three addresses no longer reply).

### Resolvers

The list grew from 37 to 47 IPv4 resolvers and from 21 to 30 IPv6. Every address was probed from a live connection before it went in.

- **Added, general use:** Gcore, Control D Unfiltered, AdGuard Unfiltered, CIRA Canadian Shield Private, DNS.SB, AliDNS, Tencent DNSPod (IPv4 only).
- **Added, malware blocking:** Quad9 Secured with ECS, Control D Malware Blocking, Control D Malware + Ads, CIRA Canadian Shield Protected.
- **Added, family:** Control D Family Friendly, CIRA Canadian Shield Family.
- **Renamed:** Verisign Public DNS and the Neustar entries are now listed as UltraDNS Public, their current operator.
- **Removed:** DNS4EU IPv6 addresses, which did not answer in testing while the same DNS4EU services over IPv4 did. The IPv4 entries stay.
- **Not added:** dns0.eu and Mullvad DNS (both shut down), UncensoredDNS and Quad101 (no reply in testing), the DNS.SB secondary IPv6 and DNSPod IPv6 (no reply or 40% loss).
- New test `tests/live_resolvers.rs` probes every listed address and fails if any of them stops answering.

## 0.1.0 — 2026-11-21

First public release.
