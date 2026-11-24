<p align="center">
  <img src="assets/logo.svg" alt="" width="180">
</p>

<h1 align="center">DNS Switcher</h1>

<p align="center">
  Change your Windows DNS in one click and find out which resolver answers fastest from your network.
</p>

<p align="center">
  <a href="https://github.com/Devzinh/dns-switcher/releases">Download</a> &nbsp;&nbsp; <a href="#how-it-works">How it works</a> &nbsp;&nbsp; <a href="#roadmap">Roadmap</a>
</p>

<p align="center">
  <b>English</b> &nbsp;|&nbsp; <a href="README.pt-BR.md">Português</a>
</p>

<br>

A single executable for Windows 10 and 11. No installer, no telemetry, nothing left running in the background.

<p align="center">
  <img src="assets/query.en.svg" alt="DNS query in hex and the reply time of each resolver" width="100%">
</p>

Above is the exact packet the **Find fastest** button sends. The same packet goes to every resolver, and the first one to reply gets selected.

## What it does

<img src="assets/screenshot.png" alt="DNS Switcher window in simple mode" width="380" align="right">

**Simple mode.** Five well-known resolvers, each with one line saying what it's for. Pick one and click **Apply DNS**. The **Automatic** button gives you back your provider's DNS.

**Advanced mode.** 47 resolvers in three groups: general use, malware blocking and family filtering. It also has IPv6, a custom server and network adapter selection.

**Find fastest** measures every server on screen, **including your provider's own DNS** (the one your router hands out), and selects the best. "Best" means the time to resolve a name plus the time to reach the server that DNS sends you to, so a resolver that answers fast but points you to another country doesn't win. When your provider's DNS is the best, the app tells you to keep it. **Flush DNS** clears the Windows cache, so the change takes effect right away. The window also shows which DNS your adapter is using right now.

<br clear="right">

<img src="assets/screenshot-advanced.png" alt="Advanced mode with median, p95, jitter, loss, cold lookup and anycast site for each resolver" width="100%">

In advanced mode each server gets its own row of measurements:

| Column | Meaning |
|---|---|
| Median | Middle value of 5 lookups of a popular name, already in the resolver's cache |
| p95 | The slowest of those lookups. A big gap from the median means the server is inconsistent |
| Jitter | Average change between one lookup and the next |
| Loss | Lookups with no reply within 500 ms |
| Cold | One lookup of a random name the resolver has never seen, so it has to ask the authoritative servers. This is what you feel on sites you visit for the first time |
| CDN | Time to connect to the servers this resolver sends you to for three big CDNs (Akamai, Microsoft, Amazon), median of the three. Measured from Brazil, resolvers in Europe sent all three to servers about 200 ms away while nearby ones got 4–12 ms. Shown in red from 80 ms |
| Site | Which anycast location answered, when the resolver reports it (for example `gru14` is Cloudflare in São Paulo) |

## How it works

The app only uses tools that ship with Windows, plus a UDP socket.

| Step | What happens |
|---|---|
| List adapters | Reads `netsh interface ipv4 show interfaces` by column, so it works with Windows in any language. Skips loopback. |
| Measure | Builds DNS queries by hand (RFC 1035) and sends them over UDP to port 53: 5 lookups of `www.google.com` 80 ms apart, one lookup of a random name under `google.com`, a CHAOS TXT `id.server` query, and A lookups of `www.akamai.com`, `www.microsoft.com` and `www.amazon.com` followed by a TCP connection to port 443 of each returned address. A reply only counts if it carries the same transaction id and the response flag. Each lookup has a 500 ms limit, and 6 servers are measured at a time. |
| Rank | Servers with at most 20% loss come first, ordered by median lookup time plus CDN connect time, then by p95. Your provider's DNS (from DHCP) is measured too. |
| Apply | `netsh interface ipv4 set dnsservers … static` for the primary server and `add dnsservers … index=2` for the secondary (`ipv6` family for IPv6). All or nothing: the previous servers are read from the registry first, the result is read back afterwards, and if any step fails the adapter is put back exactly as it was. |
| Restore | Switches IPv4 and IPv6 back to DHCP. |
| Clear cache | `ipconfig /flushdns`. |

Everything that touches the network or the system runs off the UI thread, which gets the result back through a channel. The window never freezes.

### App and service

```
 DNS Switcher app (your user, no admin)          DNS Switcher Service (LocalSystem)
 window, tray icon, measurements        ──────►  validates the request, then runs
                                    named pipe   netsh / ipconfig from System32
```

Measuring needs no special rights, so the app does it itself. Applying DNS, restoring DHCP and flushing the cache go to the service over the local pipe `\\.\pipe\DnsSwitcherService`:

- The pipe accepts SYSTEM and Administrators, plus users signed in to this computer. Connections from the network are refused.
- A request is one line with a fixed set of commands. Addresses must parse as IP addresses of the requested family. The adapter must exist, and the service looks up its interface index itself.
- Commands are started from `System32` by absolute path, never through a shell, with only validated addresses and a number as arguments.
- The service path is quoted in the service configuration.

The app writes one registry value, and only if you turn on **Start with Windows**: `DNS Switcher` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`. The DNS configuration itself is kept by Windows.

```
src/
  main.rs       UI (egui), simple and advanced modes
  tray.rs       notification-area icon, menu and Start with Windows
  service.rs    the Windows service and its pipe, plus the app's client side
  ipc.rs        request format and validation
  update.rs     release lookup, signature and hash checks
  updater.rs    automatic updates inside the service
  sys.rs        netsh, ipconfig and adapters
  probe.rs      DNS queries over UDP, reply parsing and statistics
  providers.rs  IPv4 and IPv6 resolver list, by group
installer/
  dns-switcher.iss   Inno Setup script
```

## Installation

Download **`DNS-Switcher-Setup-<version>.exe`** from [Releases](https://github.com/Devzinh/dns-switcher/releases) and run it. Setup asks for administrator rights once, and installs:

- the app in `Program Files\DNS Switcher`, with a Start menu entry and an uninstaller in **Settings → Apps**;
- **DNS Switcher Service**, which you can see in `services.msc`. It's the only part that changes network settings, so the app itself opens without a UAC prompt;
- optionally, an entry that starts DNS Switcher in the notification area when you sign in;
- optionally, automatic updates (see [Updates](#updates)).

Closing the window keeps DNS Switcher in the notification area. Click the icon to open it, or right-click for quick switching, **Automatic**, **Find fastest** and **Start with Windows**. **Quit** in that menu closes it.

Uninstalling from **Settings → Apps** stops and removes the service and the sign-in entry. It doesn't change your current DNS; click **Automatic** first if you want your provider's DNS back.

The executable and setup aren't digitally signed yet, so SmartScreen may warn you: click **More info** and then **Run anyway**. See the [code signing policy](CODE_SIGNING.md) for how releases are built and will be signed. Each release lists SHA-256 hashes, and `dns_switcher.exe` carries version information you can check in **Properties → Details**.

### Without installing

`dns_switcher.exe` also runs on its own. Without the service it can measure resolvers, but to apply DNS it has to be started with **Run as administrator**.

### Building

```powershell
git clone https://github.com/Devzinh/dns-switcher.git
cd dns-switcher
cargo build --release
& "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe" installer\dns-switcher.iss
```

This builds `target\release\dns_switcher.exe` and `installer\Output\DNS-Switcher-Setup-<version>.exe`. It requires stable Rust ([rustup.rs](https://rustup.rs)) and [Inno Setup 6](https://jrsoftware.org/isinfo.php) for the installer.

Run the tests with `cargo test`. `cargo test -- --ignored --nocapture` also probes 1.1.1.1 over the network.

What changed in each version is in the [changelog](CHANGELOG.md).

## Updates

With **Keep DNS Switcher up to date automatically** checked in setup, or **Update automatically** on in the notification-area menu, the DNS Switcher service checks this repository's latest release every six hours and installs newer versions on its own, without a UAC prompt. **Check for updates** in the same menu checks right away.

An update is installed only if every check passes:

- It comes from a published GitHub release of this repository, over HTTPS, and its version is newer than the installed one, so an older release can never be pushed as an update.
- The release's `SHA256SUMS.txt` carries an Ed25519 signature made by the release workflow with a key kept as a GitHub Actions secret. The app has the public key ([`update-key.pub`](update-key.pub)) built in and rejects any other signature.
- The downloaded installer matches the signed hash.
- The installer is saved under `Program Files\DNS Switcher\updates`, which only administrators can write to, so it can't be swapped between the check and the install.

If the tray icon was open, it opens again after the update. Updates contact only `api.github.com` and `github.com`, and send nothing but the request itself.

## Included resolvers

| Group | Purpose | Examples |
|---|---|---|
| Default | General use | Cloudflare, Google, Quad9, Gcore, Control D, AdGuard, OpenDNS, NextDNS, CIRA Canadian Shield, DNS4EU, DNS.SB, AliDNS |
| Secure | Blocks malware and phishing | Cloudflare 1.1.1.2, Quad9 with ECS, Control D, CIRA Protected, CleanBrowsing Secure, DNS4EU Protective |
| Family Safe | Blocks adult content | Cloudflare 1.1.1.3, OpenDNS FamilyShield, Control D Family, CIRA Family, AdGuard Family |

The full list is in [`src/providers.rs`](src/providers.rs). Every address on it answered in a live test before release. Services change hands over time (Verisign and Neustar public DNS are now UltraDNS Public), and a server that stops replying shows 100% loss and is never picked.

To check the whole list from your own connection:

```powershell
cargo test --release --test live_resolvers -- --ignored --nocapture
```

## Roadmap

### AI-powered Fastest DNS

Today, **Find fastest** times a fixed list. The goal is for it to answer the question that matters: *will this DNS make my connection better, from here, right now?*

- [ ] You use your own API key (Claude, OpenAI and others). It's stored in Windows Credential Manager and never passes through any project server.
- [ ] Searches the web for this year's resolvers: new services, changed IPs, shut-down services and changes to data-logging policies. The built-in list becomes just a starting point.
- [ ] Locates your connection: public IP, carrier (ASN) and region.
- [x] Finds out which of the resolver's servers is actually answering. The same IP exists in many countries (anycast), and a `CHAOS TXT id.server` query reveals which one replied. *(0.1.1; NSID, RFC 5001, still to do)*
- [ ] Checks whether the resolver passes part of your IP on to CDNs (EDNS Client Subnet, RFC 7871). That decides which YouTube or Netflix server you get sent to.
- [x] Measures each server several times and computes median, p95, variation and loss. Also separates cached replies from full lookups, using a random domain. *(0.1.1)*
- [ ] Produces a score with an explanation: plain language for beginners, and the numbers for anyone who wants to check.
- [ ] Keeps working exactly as today for anyone who doesn't provide a key.

### Network and security

- [ ] Encrypted DNS (DoH and DoT), using Windows 11's native support
- [ ] Check whether the resolver validates DNSSEC
- [ ] Detect when the ISP intercepts queries or redirects nonexistent domains
- [ ] Detect DNS leaks while a VPN is on

### Everyday use

- [x] Tray icon with quick switching *(0.1.1)*
- [ ] Per-network profiles (home, work, public Wi-Fi)
- [x] Installer and a Windows service, so the app runs without administrator rights *(0.1.1)*
- [ ] Sign the executable and installer (for example through [SignPath Foundation](https://signpath.org), which signs open-source projects for free)
- [ ] Publish to winget (`winget install DnsSwitcher`)
- [ ] Interface in Portuguese, English and Spanish
- [ ] Command line for scripts: `dns_switcher --apply cloudflare --adapter "Wi-Fi"`

## FAQ

<details>
<summary>Isn't my provider's DNS usually the best?</summary>
<br>
Often, yes: it's close to you and CDNs know where its users are. That's why **Find fastest** measures it alongside the public resolvers and recommends keeping it when it wins. Switching makes sense when it's slow or unreliable, or when you want malware or family filtering.
</details>

<details>
<summary>Will names on my local network still work?</summary>
<br>
Names that only your router knows (for example `printer.lan` or `nas.home`) resolve through your router's DNS. A public resolver doesn't know them, so they can stop working after you switch. When your network announces a local domain, the app warns you after applying. **Automatic** brings everything back. If you rely on local names, a better setup is to change the DNS in the router instead, so it keeps answering for the local ones.
</details>

<details>
<summary>Does changing DNS increase download speed?</summary>
<br>
Not directly. DNS only translates names into addresses. The gain is in the time to the first connection with each site. Also, depending on the resolver, the CDN may send you to a closer server, and that affects streaming and gaming.
</details>

<details>
<summary>Why isn't the fastest one for me the fastest for someone else?</summary>
<br>
Public resolvers use anycast: the same IP exists in many places around the world. Which one serves you depends on the route your carrier picks, not on distance on a map. That's why the measurement has to run on your connection.
</details>

<details>
<summary>Does the app collect any data?</summary>
<br>
No. The only thing it sends over the network is the test queries to the resolvers you're measuring.
</details>

<details>
<summary>How do I undo it?</summary>
<br>
Click <b>Automatic</b> in simple mode or <b>Default</b> in advanced mode. That's equivalent to <code>netsh interface ipv4 set dnsservers name="Wi-Fi" dhcp</code>, plus the same command for IPv6.
</details>

## Contributing

To add or fix a resolver, edit `src/providers.rs` and include a link to the service's official page in the pull request. Run `cargo fmt` and `cargo clippy` before opening the PR.

When reporting a bug, include your Windows version and language, and paste the output of `netsh interface ipv4 show interfaces`.

## Code signing policy

Releases are not code-signed yet. From 0.1.2 on they're built by GitHub Actions from this repository, and each file carries a build provenance attestation you can verify with `gh attestation verify <file> -R Devzinh/dns-switcher`. The project will apply again to SignPath Foundation for free code signing once it has wider adoption. Team roles, the privacy statement and what the installer changes are in [CODE_SIGNING.md](CODE_SIGNING.md).

## License

[MIT](LICENSE)
