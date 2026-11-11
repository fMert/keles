use argon2::{
    password_hash::{
        rand_core::{OsRng, RngCore},
        PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
    },
    Argon2,
};
use std::{
    collections::HashMap,
    fs,
    io::{self, Write},
    path::Path,
    sync::Mutex,
};

static HASH_GATE: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, PartialEq)]
pub struct Account {
    pub name: String,
    pub hash: String,
    pub rating: u32,
    pub wins: u32,
    pub losses: u32,
}

pub fn valid_username(name: &str) -> bool {
    (3..=16).contains(&name.len())
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

pub fn valid_password(password: &str) -> bool {
    (8..=64).contains(&password.chars().count()) && !password.chars().any(char::is_control)
}

// RustCrypto/password-hashes, argon2/src/lib.rs lines 36–58, commit 5b6a74e0799955b21c9d3854de1674a78a472648.
// https://github.com/RustCrypto/password-hashes/blob/5b6a74e0799955b21c9d3854de1674a78a472648/argon2/src/lib.rs#L36-L58
// MIT OR Apache-2.0. PHC API usage; HASH_GATE serializes the memory-heavy work.
pub fn hash(password: &str) -> Result<String, &'static str> {
    let _gate = HASH_GATE.lock().unwrap();
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| "full")
}

pub fn verify(password: &str, hash: &str) -> bool {
    let _gate = HASH_GATE.lock().unwrap();
    PasswordHash::new(hash).is_ok_and(|hash| {
        Argon2::default()
            .verify_password(password.as_bytes(), &hash)
            .is_ok()
    })
}

pub fn token() -> String {
    let mut bytes = [0; 32];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn load(path: &Path) -> io::Result<HashMap<String, Account>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(error) => return Err(error),
    };
    let mut lines = text.lines();
    if lines.next() != Some("# keles accounts v1") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Hesap dosyasının başlığı geçersiz",
        ));
    }
    let mut accounts = HashMap::new();
    for (line, text) in lines.enumerate() {
        let bad = || {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Hesap dosyasında bozuk satır: {}", line + 2),
            )
        };
        let fields: Vec<_> = text.split('\t').collect();
        if fields.len() != 6
            || !valid_username(fields[1])
            || fields[0] != fields[1].to_ascii_lowercase()
        {
            return Err(bad());
        }
        let phc = PasswordHash::new(fields[2]).map_err(|_| bad())?;
        if phc.algorithm.as_str() != "argon2id" || phc.salt.is_none() || phc.hash.is_none() {
            return Err(bad());
        }
        if argon2::Params::try_from(&phc).is_err() || phc.version != Some(19) {
            return Err(bad());
        }
        let account = Account {
            name: fields[1].to_owned(),
            hash: fields[2].to_owned(),
            rating: fields[3].parse().map_err(|_| bad())?,
            wins: fields[4].parse().map_err(|_| bad())?,
            losses: fields[5].parse().map_err(|_| bad())?,
        };
        if accounts.insert(fields[0].to_owned(), account).is_some() {
            return Err(bad());
        }
    }
    Ok(accounts)
}

pub fn save(path: &Path, accounts: &HashMap<String, Account>) -> io::Result<()> {
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp)?;
    writeln!(file, "# keles accounts v1")?;
    let mut entries: Vec<_> = accounts.iter().collect();
    entries.sort_unstable_by_key(|(key, _)| *key);
    for (key, a) in entries {
        writeln!(
            file,
            "{key}\t{}\t{}\t{}\t{}\t{}",
            a.name, a.hash, a.rating, a.wins, a.losses
        )?;
    }
    file.sync_all()?;
    fs::rename(temp, path)
}

pub fn elo(me: u32, opponent: u32, score: f64) -> u32 {
    let expected = 1.0 / (1.0 + 10.0_f64.powf((opponent as f64 - me as f64) / 400.0));
    (me as f64 + 32.0 * (score - expected)).round().max(0.0) as u32
}
