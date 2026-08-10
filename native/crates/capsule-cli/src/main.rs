use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

use ed25519_dalek::SigningKey;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde_json::{Value, json};
use sqlite_capsule_core::inspect_metadata;
use sqlite_capsule_crypto::{
    ALGORITHM, PROFILE, application_digest, key_id, publisher_identity, sign_digest,
    signature_inventory, verify_signatures,
};
use sqlite_capsule_launch::{inspect_launch, verify_structure as verify_launch_structure};
use sqlite_capsule_policy::{CapabilityDecision, EvaluationContext, LaunchEvidence, TrustStore};

const SIGNED_SCHEMA: &str = include_str!("../../../../format/capsule-signed-app-v0.2.sql");

fn main() {
    if let Err(error) = run() {
        eprintln!("{}", json!({"ok": false, "error": error.to_string()}));
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = env::args_os();
    let _program = arguments.next();
    let Some(command) = arguments.next().and_then(|value| value.into_string().ok()) else {
        return Err(usage().into());
    };
    let remaining: Vec<_> = arguments.collect();
    match command.as_str() {
        "inspect" if remaining.len() == 1 => inspect(Path::new(&remaining[0])),
        "verify" if remaining.len() == 1 => verify(Path::new(&remaining[0])),
        "digest" if remaining.len() == 1 => digest(Path::new(&remaining[0])),
        "verify-signature" if remaining.len() == 1 => verify_signature(Path::new(&remaining[0])),
        "key-id" if remaining.len() == 1 => print_key_id(Path::new(&remaining[0])),
        "sign" => sign_command(&remaining),
        "trust" => trust_command(&remaining),
        _ => Err(usage().into()),
    }
}

fn usage() -> &'static str {
    "usage: capsule-native <inspect|verify|digest|verify-signature|key-id|sign|trust> ...\n\
     sign <source> <output> --publisher-id <id> --publisher-name <name> \
     --key <32-byte-or-hex-seed-file> --signed-at <YYYY-MM-DDTHH:MM:SSZ>\n\
     trust <init|inspect|trust-key|trust-release|trust-file|deny-file|grant|\
     revoke-key|audit|export|backup|reset> ..."
}

fn verify(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let identity = verify_structure(path)?;
    let connection = open_read_only(path)?;
    let publisher_present = has_table(&connection, "capsule_publisher")?;
    let signature_present = has_table(&connection, "capsule_signature")?;
    let signed_extension_valid = if publisher_present || signature_present {
        publisher_present
            && signature_present
            && publisher_identity(&connection).is_ok()
            && signature_inventory(&connection).is_ok()
    } else {
        false
    };
    if publisher_present != signature_present || (publisher_present && !signed_extension_valid) {
        return Err("signed-app extension is malformed".into());
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "ok": true,
            "integrity_ok": true,
            "structure_ok": true,
            "signature_extension_present": publisher_present,
            "signature_extension_valid": signed_extension_valid,
            "signature_valid": null,
            "publisher_known": false,
            "publisher_trusted": false,
            "revocation_status": "not_checked",
            "executable_allowed": false,
            "identity": identity,
            "note": "Structure verification does not authenticate or trust a publisher."
        }))?
    );
    Ok(())
}

fn inspect(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let identity = inspect_metadata(path)?;
    let connection = open_read_only(path)?;
    let signed = has_table(&connection, "capsule_publisher")?
        || has_table(&connection, "capsule_signature")?;
    let signature_count = if signed {
        signature_inventory(&connection)?.len()
    } else {
        0
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "ok": true,
            "integrity_ok": null,
            "signature_valid": null,
            "publisher_known": null,
            "publisher_trusted": null,
            "revocation_status": "not_checked",
            "executable_allowed": false,
            "identity": identity,
            "signature_extension_present": signed,
            "signature_count": signature_count,
            "note": "Metadata inspection does not authenticate a publisher."
        }))?
    );
    Ok(())
}

fn digest(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    inspect_metadata(path)?;
    let connection = open_read_only(path)?;
    let digest = application_digest(&connection)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "ok": true,
            "profile": PROFILE,
            "application_digest_sha256": lower_hex(&digest)
        }))?
    );
    Ok(())
}

fn verify_signature(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    inspect_metadata(path)?;
    let connection = open_read_only(path)?;
    if !has_table(&connection, "capsule_publisher")?
        && !has_table(&connection, "capsule_signature")?
    {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "ok": true,
                "signature_extension_present": false,
                "signature_valid": false,
                "publisher_known": false,
                "publisher_trusted": false,
                "revocation_status": "not_checked",
                "executable_allowed": false
            }))?
        );
        return Ok(());
    }

    let publisher = publisher_identity(&connection)?;
    let digest = application_digest(&connection)?;
    let reports = verify_signatures(&connection)?;
    let signatures: Vec<Value> = reports
        .iter()
        .map(|report| {
            json!({
                "key_id": report.key_id,
                "cryptographically_valid": report.cryptographically_valid,
                "digest_matches": report.digest_matches,
                "signed_at": report.signed_at
            })
        })
        .collect();
    let valid = reports
        .iter()
        .any(|report| report.cryptographically_valid && report.digest_matches);
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "ok": true,
            "profile": PROFILE,
            "publisher": {
                "id": publisher.publisher_id,
                "name": publisher.publisher_name
            },
            "application_digest_sha256": lower_hex(&digest),
            "signature_valid": valid,
            "publisher_known": false,
            "publisher_trusted": false,
            "revocation_status": "not_checked",
            "executable_allowed": false,
            "signatures": signatures
        }))?
    );
    Ok(())
}

fn print_key_id(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let signing_key = read_signing_key(path)?;
    let public_key = signing_key.verifying_key().to_bytes();
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "ok": true,
            "key_id": key_id(&public_key),
            "public_key_hex": lower_hex(&public_key)
        }))?
    );
    Ok(())
}

fn sign_command(arguments: &[std::ffi::OsString]) -> Result<(), Box<dyn std::error::Error>> {
    if arguments.len() != 10 {
        return Err(usage().into());
    }
    let source = PathBuf::from(&arguments[0]);
    let output = PathBuf::from(&arguments[1]);
    let options = parse_sign_options(&arguments[2..])?;
    sign_capsule(
        &source,
        &output,
        &options.publisher_id,
        &options.publisher_name,
        &options.key,
        &options.signed_at,
    )?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "ok": true,
            "source": source,
            "output": output,
            "profile": PROFILE,
            "publisher_id": options.publisher_id,
            "signed_at": options.signed_at
        }))?
    );
    Ok(())
}

struct SignOptions {
    publisher_id: String,
    publisher_name: String,
    key: PathBuf,
    signed_at: String,
}

fn parse_sign_options(arguments: &[std::ffi::OsString]) -> Result<SignOptions, &'static str> {
    let mut publisher_id = None;
    let mut publisher_name = None;
    let mut key = None;
    let mut signed_at = None;
    for pair in arguments.chunks_exact(2) {
        let option = pair[0].to_str().ok_or("options must be UTF-8")?;
        match option {
            "--publisher-id" => {
                if publisher_id.is_some() {
                    return Err("unknown or duplicated sign option");
                }
                publisher_id = Some(
                    pair[1]
                        .to_str()
                        .ok_or("publisher id must be UTF-8")?
                        .to_owned(),
                )
            }
            "--publisher-name" => {
                if publisher_name.is_some() {
                    return Err("unknown or duplicated sign option");
                }
                publisher_name = Some(
                    pair[1]
                        .to_str()
                        .ok_or("publisher name must be UTF-8")?
                        .to_owned(),
                )
            }
            "--key" => {
                if key.is_some() {
                    return Err("unknown or duplicated sign option");
                }
                key = Some(PathBuf::from(&pair[1]));
            }
            "--signed-at" => {
                if signed_at.is_some() {
                    return Err("unknown or duplicated sign option");
                }
                signed_at = Some(
                    pair[1]
                        .to_str()
                        .ok_or("signed_at must be UTF-8")?
                        .to_owned(),
                )
            }
            _ => return Err("unknown or duplicated sign option"),
        }
    }
    Ok(SignOptions {
        publisher_id: publisher_id.ok_or("missing --publisher-id")?,
        publisher_name: publisher_name.ok_or("missing --publisher-name")?,
        key: key.ok_or("missing --key")?,
        signed_at: signed_at.ok_or("missing --signed-at")?,
    })
}

fn sign_capsule(
    source: &Path,
    output: &Path,
    publisher_id: &str,
    publisher_name: &str,
    key_path: &Path,
    signed_at: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    verify_structure(source)?;
    if output.exists() {
        return Err("refusing to replace an existing output".into());
    }
    let source_canonical = fs::canonicalize(source)?;
    let parent = output
        .parent()
        .ok_or("output must have a parent directory")?;
    let parent_canonical = fs::canonicalize(parent)?;
    if source_canonical.parent() == Some(parent_canonical.as_path())
        && source_canonical.file_name() == output.file_name()
    {
        return Err("refusing in-place signing".into());
    }
    if publisher_id.is_empty()
        || publisher_id.len() > 512
        || publisher_name.is_empty()
        || publisher_name.len() > 512
    {
        return Err("publisher identity is empty or oversized".into());
    }

    let temporary = parent.join(format!(
        ".{}.signing-{}.tmp",
        output
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or("output filename must be UTF-8")?,
        std::process::id()
    ));
    if temporary.exists() {
        return Err("temporary signing path already exists".into());
    }
    let mut guard = TemporaryOutput::new(temporary.clone());

    let source_connection = open_read_only(source)?;
    source_connection.backup(rusqlite::MAIN_DB, &temporary, None)?;
    let signing_key = read_signing_key(key_path)?;
    let mut destination = Connection::open(&temporary)?;
    destination.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA foreign_keys=ON;")?;
    let publisher_present = has_table(&destination, "capsule_publisher")?;
    let signature_present = has_table(&destination, "capsule_signature")?;
    if publisher_present != signature_present {
        return Err("partial signed-app extension is not accepted".into());
    }

    let transaction = destination.transaction()?;
    if !publisher_present {
        transaction.execute_batch(SIGNED_SCHEMA)?;
        transaction.execute(
            "INSERT INTO capsule_publisher \
             (id, profile, publisher_id, publisher_name) VALUES (1, ?1, ?2, ?3)",
            params![PROFILE, publisher_id, publisher_name],
        )?;
    } else {
        let publisher = publisher_identity(&transaction)?;
        if publisher.publisher_id != publisher_id || publisher.publisher_name != publisher_name {
            return Err("existing signed publisher identity does not match".into());
        }
    }

    let digest = application_digest(&transaction)?;
    let envelope = sign_digest(&signing_key, digest, signed_at)?;
    transaction.execute(
        "INSERT INTO capsule_signature \
         (key_id, algorithm, public_key, application_digest, signature, signed_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            envelope.key_id,
            ALGORITHM,
            envelope.public_key.as_slice(),
            envelope.application_digest.as_slice(),
            envelope.signature.as_slice(),
            envelope.signed_at,
        ],
    )?;
    transaction.commit()?;
    drop(destination);

    inspect_metadata(&temporary)?;
    let finished = open_read_only(&temporary)?;
    let reports = verify_signatures(&finished)?;
    if !reports
        .iter()
        .any(|report| report.cryptographically_valid && report.digest_matches)
    {
        return Err("finished output failed independent signature verification".into());
    }
    drop(finished);
    fs::rename(&temporary, output)?;
    guard.keep();
    Ok(())
}

fn trust_command(arguments: &[std::ffi::OsString]) -> Result<(), Box<dyn std::error::Error>> {
    let Some(action) = arguments.first().and_then(|value| value.to_str()) else {
        return Err(usage().into());
    };
    match action {
        "init" if arguments.len() == 2 => {
            let store = TrustStore::open(Path::new(&arguments[1]))?;
            print_json(json!({
                "ok": true,
                "action": "trust.init",
                "store": store.path(),
                "note": "The trust store is host-local and contains no private keys."
            }))
        }
        "inspect" if arguments.len() == 3 => {
            let mut store = TrustStore::open(Path::new(&arguments[1]))?;
            let evidence = policy_evidence(Path::new(&arguments[2]))?;
            let decision = store.evaluate(&evidence, &EvaluationContext::default())?;
            print_policy_result("trust.inspect", store.path(), &evidence, decision)
        }
        "trust-key" if arguments.len() == 9 => {
            let mut store = TrustStore::open(Path::new(&arguments[1]))?;
            let evidence = policy_evidence(Path::new(&arguments[2]))?;
            let options = utf8_options(&arguments[3..])?;
            let key_id = required_option(&options, "--key-id")?;
            let reason = required_option(&options, "--reason")?;
            let confirmation = required_option(&options, "--confirm")?;
            if confirmation != key_id {
                return Err("confirmation must exactly match --key-id".into());
            }
            let publisher = evidence
                .publisher
                .as_ref()
                .ok_or("signed publisher is missing")?;
            let signature = evidence
                .signatures
                .iter()
                .find(|signature| {
                    signature.key_id == key_id
                        && signature.cryptographically_valid
                        && signature.digest_matches
                })
                .ok_or("key does not have a current valid signature")?;
            store.trust_publisher_key(publisher, key_id, &signature.public_key, reason)?;
            let decision = store.evaluate(&evidence, &EvaluationContext::default())?;
            print_policy_result("trust.trust_key", store.path(), &evidence, decision)
        }
        "trust-release" if arguments.len() == 9 => {
            let mut store = TrustStore::open(Path::new(&arguments[1]))?;
            let evidence = policy_evidence(Path::new(&arguments[2]))?;
            let options = utf8_options(&arguments[3..])?;
            let key_id = required_option(&options, "--key-id")?;
            let reason = required_option(&options, "--reason")?;
            let confirmation = required_option(&options, "--confirm")?;
            let digest = evidence
                .application_digest
                .as_ref()
                .map(|value| lower_hex(value))
                .ok_or("signed application digest is missing")?;
            if confirmation != digest {
                return Err("confirmation must exactly match the application digest".into());
            }
            store.trust_exact_release(&evidence, key_id, reason)?;
            let decision = store.evaluate(&evidence, &EvaluationContext::default())?;
            print_policy_result("trust.trust_release", store.path(), &evidence, decision)
        }
        "trust-file" | "deny-file" if arguments.len() == 7 => {
            let mut store = TrustStore::open(Path::new(&arguments[1]))?;
            let evidence = policy_evidence(Path::new(&arguments[2]))?;
            let options = utf8_options(&arguments[3..])?;
            let reason = required_option(&options, "--reason")?;
            let confirmation = required_option(&options, "--confirm")?;
            let source_sha256 = lower_hex(&evidence.source_sha256);
            if confirmation != source_sha256 {
                return Err("confirmation must exactly match the source SHA-256".into());
            }
            if action == "trust-file" {
                store.trust_exact_file(&evidence, reason)?;
            } else {
                store.deny_exact_file(&evidence, reason)?;
            }
            let decision = store.evaluate(&evidence, &EvaluationContext::default())?;
            print_policy_result(
                if action == "trust-file" {
                    "trust.trust_file"
                } else {
                    "trust.deny_file"
                },
                store.path(),
                &evidence,
                decision,
            )
        }
        "grant" if arguments.len() == 11 => {
            let mut store = TrustStore::open(Path::new(&arguments[1]))?;
            let evidence = policy_evidence(Path::new(&arguments[2]))?;
            let options = utf8_options(&arguments[3..])?;
            let capability = required_option(&options, "--capability")?;
            let decision = match required_option(&options, "--decision")? {
                "allow" => CapabilityDecision::Allow,
                "deny" => CapabilityDecision::Deny,
                _ => return Err("grant decision must be allow or deny".into()),
            };
            let reason = required_option(&options, "--reason")?;
            let confirmation = required_option(&options, "--confirm")?;
            if !evidence.requested_capabilities.contains(capability) {
                return Err("capability was not requested by the capsule".into());
            }
            if !evidence
                .signatures
                .iter()
                .any(|signature| signature.cryptographically_valid && signature.digest_matches)
            {
                return Err("persistent grants require a current valid signature".into());
            }
            let digest = evidence
                .application_digest
                .as_ref()
                .map(|value| lower_hex(value))
                .ok_or("signed application digest is missing")?;
            if confirmation != digest {
                return Err("confirmation must exactly match the application digest".into());
            }
            store.set_persistent_grant(&evidence, capability, decision, reason)?;
            let launch = store.evaluate(&evidence, &EvaluationContext::default())?;
            print_policy_result("trust.grant", store.path(), &evidence, launch)
        }
        "revoke-key" if arguments.len() == 8 => {
            let mut store = TrustStore::open(Path::new(&arguments[1]))?;
            let options = utf8_options(&arguments[2..])?;
            let key_id = required_option(&options, "--key-id")?;
            let reason = required_option(&options, "--reason")?;
            let confirmation = required_option(&options, "--confirm")?;
            if confirmation != key_id {
                return Err("confirmation must exactly match --key-id".into());
            }
            store.revoke_publisher_key(key_id, reason)?;
            print_json(json!({
                "ok": true,
                "action": "trust.revoke_key",
                "store": store.path(),
                "key_id": key_id,
                "revoked": true
            }))
        }
        "audit" if arguments.len() == 2 || arguments.len() == 4 => {
            let store = TrustStore::open(Path::new(&arguments[1]))?;
            let limit = if arguments.len() == 4 {
                if arguments[2] != "--limit" {
                    return Err(usage().into());
                }
                arguments[3]
                    .to_str()
                    .ok_or("audit limit must be UTF-8")?
                    .parse::<usize>()?
            } else {
                100
            };
            print_json(json!({
                "ok": true,
                "action": "trust.audit",
                "store": store.path(),
                "events": store.audit_events(limit)?
            }))
        }
        "export" if arguments.len() == 2 => {
            let store = TrustStore::open(Path::new(&arguments[1]))?;
            print_json(json!({
                "ok": true,
                "action": "trust.export",
                "export": store.export_redacted()?
            }))
        }
        "backup" if arguments.len() == 3 => {
            let store = TrustStore::open(Path::new(&arguments[1]))?;
            let output = Path::new(&arguments[2]);
            store.backup_to(output)?;
            print_json(json!({
                "ok": true,
                "action": "trust.backup",
                "store": store.path(),
                "output": output
            }))
        }
        "reset" if arguments.len() == 5 => {
            let mut store = TrustStore::open(Path::new(&arguments[1]))?;
            let backup = Path::new(&arguments[2]);
            if arguments[3] != "--confirm" {
                return Err(usage().into());
            }
            let confirmation = arguments[4]
                .to_str()
                .ok_or("reset confirmation must be UTF-8")?;
            if confirmation != "ERASE-TRUST-DECISIONS" {
                return Err("reset requires exact confirmation ERASE-TRUST-DECISIONS".into());
            }
            store.backup_to(backup)?;
            store.reset_decisions(confirmation)?;
            print_json(json!({
                "ok": true,
                "action": "trust.reset",
                "store": store.path(),
                "verified_backup": backup,
                "decisions_erased": true
            }))
        }
        _ => Err(usage().into()),
    }
}

fn policy_evidence(path: &Path) -> Result<LaunchEvidence, Box<dyn std::error::Error>> {
    Ok(inspect_launch(path)?.evidence)
}

fn utf8_options(
    arguments: &[std::ffi::OsString],
) -> Result<BTreeMap<String, String>, Box<dyn std::error::Error>> {
    if !arguments.len().is_multiple_of(2) {
        return Err("options must be name/value pairs".into());
    }
    let mut options = BTreeMap::new();
    for pair in arguments.chunks_exact(2) {
        let name = pair[0].to_str().ok_or("option name must be UTF-8")?;
        let value = pair[1].to_str().ok_or("option value must be UTF-8")?;
        if !name.starts_with("--") || options.insert(name.to_owned(), value.to_owned()).is_some() {
            return Err("unknown or duplicated option".into());
        }
    }
    Ok(options)
}

fn required_option<'a>(
    options: &'a BTreeMap<String, String>,
    name: &str,
) -> Result<&'a str, Box<dyn std::error::Error>> {
    options
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| format!("missing {name}").into())
}

fn print_policy_result(
    action: &str,
    store: &Path,
    evidence: &LaunchEvidence,
    decision: sqlite_capsule_policy::LaunchDecision,
) -> Result<(), Box<dyn std::error::Error>> {
    print_json(json!({
        "ok": true,
        "action": action,
        "store": store,
        "capsule": {
            "capsule_id": evidence.capsule_id,
            "application_id": evidence.application_id,
            "source_sha256": lower_hex(&evidence.source_sha256)
        },
        "publisher": evidence.publisher.as_ref().map(|publisher| json!({
            "id": publisher.publisher_id,
            "name": publisher.publisher_name
        })),
        "decision": decision
    }))
}

fn print_json(value: Value) -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn verify_structure(
    path: &Path,
) -> Result<sqlite_capsule_core::CapsuleIdentity, Box<dyn std::error::Error>> {
    Ok(verify_launch_structure(path)?)
}

fn read_signing_key(path: &Path) -> Result<SigningKey, Box<dyn std::error::Error>> {
    let mut bytes = fs::read(path)?;
    let mut seed = if bytes.len() == 32 {
        <[u8; 32]>::try_from(bytes.as_slice()).map_err(|_| "key must contain 32 bytes")?
    } else {
        let text = std::str::from_utf8(&bytes)?.trim();
        decode_hex_32(text)?
    };
    bytes.fill(0);
    let signing_key = SigningKey::from_bytes(&seed);
    seed.fill(0);
    Ok(signing_key)
}

fn decode_hex_32(value: &str) -> Result<[u8; 32], &'static str> {
    if value.len() != 64 {
        return Err("hex key must contain exactly 64 lowercase or uppercase digits");
    }
    let mut bytes = [0_u8; 32];
    for (index, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = hex_value(chunk[0]).ok_or("invalid hex key")?;
        let low = hex_value(chunk[1]).ok_or("invalid hex key")?;
        bytes[index] = (high << 4) | low;
    }
    Ok(bytes)
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn open_read_only(path: &Path) -> rusqlite::Result<Connection> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA foreign_keys=ON;")?;
    Ok(connection)
}

fn has_table(connection: &Connection, name: &str) -> rusqlite::Result<bool> {
    Ok(connection
        .query_row(
            "SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1",
            [name],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

struct TemporaryOutput {
    path: PathBuf,
    remove: bool,
}

impl TemporaryOutput {
    fn new(path: PathBuf) -> Self {
        Self { path, remove: true }
    }

    fn keep(&mut self) {
        self.remove = false;
    }
}

impl Drop for TemporaryOutput {
    fn drop(&mut self) {
        if self.remove {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::OsString,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use super::*;

    static TEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(name: &str) -> Self {
            let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!(
                "sqlite-capsule-cli-{name}-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&path).expect("create test directory");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn checked_capsule() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .join("capsules/diagram-studio.capsule.sqlite")
    }

    fn test_key(directory: &TestDirectory) -> PathBuf {
        let path = directory.path().join("publisher.seed");
        fs::write(&path, [42_u8; 32]).expect("write test seed");
        path
    }

    #[test]
    fn signs_a_copy_and_separates_domain_edits_from_application_tampering() {
        let directory = TestDirectory::new("signed-copy");
        let source = checked_capsule();
        let source_before = fs::read(&source).expect("read checked source");
        let key = test_key(&directory);
        let output = directory.path().join("signed.capsule.sqlite");
        sign_capsule(
            &source,
            &output,
            "org.example.publisher",
            "Example Publisher",
            &key,
            "2026-08-08T12:34:56Z",
        )
        .expect("sign checked capsule");

        assert_eq!(source_before, fs::read(&source).expect("reread source"));
        inspect_metadata(&output).expect("inspect signed output");
        let connection = Connection::open(&output).expect("open signed output");
        let digest = application_digest(&connection).expect("signed digest");
        let reports = verify_signatures(&connection).expect("verify signed output");
        assert_eq!(reports.len(), 1);
        assert!(reports[0].cryptographically_valid);
        assert!(reports[0].digest_matches);

        connection
            .execute(
                "UPDATE diagram_document SET title = title || ' (edited)' WHERE id = 'diagram-main'",
                [],
            )
            .expect("domain-only edit");
        assert_eq!(
            digest,
            application_digest(&connection).expect("digest after domain edit")
        );
        assert!(verify_signatures(&connection).expect("verify domain edit")[0].digest_matches);

        connection
            .execute(
                "UPDATE capsule_asset SET content = X'00' WHERE path = 'app/app.js'",
                [],
            )
            .expect("tamper application asset");
        let tampered = verify_signatures(&connection).expect("verify tampered output");
        assert!(tampered[0].cryptographically_valid);
        assert!(!tampered[0].digest_matches);
    }

    #[test]
    fn schema_tampering_breaks_the_application_digest() {
        let directory = TestDirectory::new("schema-tamper");
        let source = checked_capsule();
        let key = test_key(&directory);
        let output = directory.path().join("signed.capsule.sqlite");
        sign_capsule(
            &source,
            &output,
            "org.example.publisher",
            "Example Publisher",
            &key,
            "2026-08-08T12:34:56Z",
        )
        .expect("sign checked capsule");
        let connection = Connection::open(&output).expect("open signed output");
        assert!(verify_signatures(&connection).expect("verify")[0].digest_matches);
        connection
            .execute_batch("CREATE INDEX diagram_document_title_idx ON diagram_document(title);")
            .expect("tamper schema");
        assert!(!verify_signatures(&connection).expect("verify tamper")[0].digest_matches);
    }

    #[test]
    fn signing_never_replaces_source_or_existing_output() {
        let directory = TestDirectory::new("refusal");
        let source = checked_capsule();
        let key = test_key(&directory);
        assert!(
            sign_capsule(
                &source,
                &source,
                "org.example.publisher",
                "Example Publisher",
                &key,
                "2026-08-08T12:34:56Z",
            )
            .is_err()
        );

        let output = directory.path().join("existing.capsule.sqlite");
        fs::write(&output, b"preserve me").expect("write existing output");
        assert!(
            sign_capsule(
                &source,
                &output,
                "org.example.publisher",
                "Example Publisher",
                &key,
                "2026-08-08T12:34:56Z",
            )
            .is_err()
        );
        assert_eq!(
            fs::read(&output).expect("read existing output"),
            b"preserve me"
        );
    }

    #[test]
    fn signing_rejects_a_structurally_invalid_source() {
        let directory = TestDirectory::new("invalid-source");
        let source = directory.path().join("invalid.capsule.sqlite");
        fs::copy(checked_capsule(), &source).expect("copy source");
        let connection = Connection::open(&source).expect("open source");
        connection
            .execute_batch("PRAGMA foreign_keys=OFF;")
            .expect("disable foreign keys");
        connection
            .execute(
                "UPDATE diagram_node SET diagram_id = 'missing' WHERE id = 'node-capsule-shell'",
                [],
            )
            .expect("create foreign-key violation");
        drop(connection);

        let key = test_key(&directory);
        let output = directory.path().join("signed.capsule.sqlite");
        assert!(
            sign_capsule(
                &source,
                &output,
                "org.example.publisher",
                "Example Publisher",
                &key,
                "2026-08-08T12:34:56Z",
            )
            .is_err()
        );
        assert!(!output.exists());
    }

    #[test]
    fn duplicate_sign_options_are_rejected() {
        let arguments = [
            OsString::from("--publisher-id"),
            OsString::from("one"),
            OsString::from("--publisher-id"),
            OsString::from("two"),
            OsString::from("--key"),
            OsString::from("key"),
            OsString::from("--signed-at"),
            OsString::from("2026-08-08T12:34:56Z"),
        ];
        assert!(parse_sign_options(&arguments).is_err());
    }
}
