use ed25519_dalek::{Signer, SigningKey};

fn hex(b: &[u8]) -> String {
    b.iter().map(|b| format!("{b:02x}")).collect()
}

fn key_from_env() -> SigningKey {
    let h = std::env::var("UPDATE_SIGNING_KEY").expect("UPDATE_SIGNING_KEY is not set");
    let h = h.trim();
    let bytes: Vec<u8> = (0..h.len()).step_by(2).map(|i| u8::from_str_radix(&h[i..i + 2], 16).expect("hex")).collect();
    SigningKey::from_bytes(&bytes.try_into().expect("32-byte key"))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("keygen") => {
            let path = args.get(2).expect("keygen <private-key-file>");
            assert!(!std::path::Path::new(path).exists(), "{path} already exists; not overwriting a key");
            let mut seed = [0u8; 32];
            getrandom::getrandom(&mut seed).expect("OS random");
            let key = SigningKey::from_bytes(&seed);
            std::fs::write(path, hex(&key.to_bytes())).expect("write private key");
            println!("{}", hex(&key.verifying_key().to_bytes()));
        }
        Some("sign") => {
            let path = args.get(2).expect("sign <file>");
            let data = std::fs::read(path).expect("read file to sign");
            let sig = key_from_env().sign(&data);
            std::fs::write(format!("{path}.sig"), hex(&sig.to_bytes())).expect("write signature");
            eprintln!("signed {path}");
        }
        Some("public") => println!("{}", hex(&key_from_env().verifying_key().to_bytes())),
        _ => eprintln!("usage: update_sign keygen <file> | sign <file> | public"),
    }
}
