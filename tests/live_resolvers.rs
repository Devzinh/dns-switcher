use dns_switcher::probe;
use dns_switcher::providers::{GROUPS, QUICK};
use std::collections::BTreeSet;

#[test]
#[ignore]
fn every_listed_resolver_answers() {
    let mut addrs = BTreeSet::new();
    let lists = GROUPS
        .iter()
        .flat_map(|g| [(g.title, g.ipv4), (g.title, g.ipv6)])
        .chain([("Simple mode", QUICK)]);
    for (title, list) in lists {
        for p in list {
            for a in [p.dns1, p.dns2] {
                if !a.is_empty() {
                    addrs.insert((a.to_string(), format!("{} ({title})", p.name)));
                }
            }
        }
    }
    let have_v6 = probe::probe("2001:4860:4860::8888").median().is_some();
    let addrs: Vec<_> = addrs.into_iter().filter(|(a, _)| have_v6 || !a.contains(':')).collect();

    let results: Vec<_> = addrs
        .chunks(8)
        .map(|chunk| {
            let chunk = chunk.to_vec();
            std::thread::spawn(move || {
                chunk.into_iter().map(|(a, n)| { let r = probe::probe(&a); (a, n, r) }).collect::<Vec<_>>()
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .flat_map(|h| h.join().unwrap())
        .collect();

    let mut dead = Vec::new();
    for (addr, name, r) in &results {
        let median = r.median().map_or("-".into(), |m| format!("{m} ms"));
        println!("{addr:<22} {median:>7}  loss {:>3}%  {name}", r.loss_pct());
        if r.median().is_none() {
            dead.push(format!("{addr} {name}"));
        }
    }
    println!("{} addresses checked, ipv6 {}", results.len(), if have_v6 { "included" } else { "skipped" });
    assert!(dead.is_empty(), "no reply from:\n{}", dead.join("\n"));
}
