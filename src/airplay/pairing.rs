//! Persistent Ed25519 receiver identity and legacy AirPlay pairing (`pair-setup` + `pair-verify`).
//!
//! Matches UxPlay `lib/pairing.c`.

use aes::cipher::{KeyIvInit, StreamCipher};
use aes::Aes128;
use ctr::Ctr128BE;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use sha2::{Digest, Sha512};
use std::path::Path;
use x25519_dalek::{PublicKey as XPublic, StaticSecret as XSecret};

use super::hex;

type Aes128Ctr = Ctr128BE<Aes128>;

#[derive(Clone)]
pub struct Identity {
    signing: SigningKey,
}

impl Identity {
    /// Load 32-byte Ed25519 seed from `state_dir/identity.key`, or generate & persist one.
    pub fn load_or_create(state_dir: &Path, fallback_seed_hint: &[u8]) -> Self {
        let key_path = state_dir.join("identity.key");
        if let Ok(bytes) = std::fs::read(&key_path) {
            if bytes.len() == 32 {
                let mut seed = [0u8; 32];
                seed.copy_from_slice(&bytes);
                return Self {
                    signing: SigningKey::from_bytes(&seed),
                };
            }
        }
        // Generate random 32-byte seed using OS entropy (or deterministic fallback in constrained sandbox).
        let mut seed = [0u8; 32];
        if !fill_random(&mut seed) {
            let mut h = Sha512::new();
            h.update(b"yansit-tv-identity-seed");
            h.update(fallback_seed_hint);
            seed.copy_from_slice(&h.finalize()[..32]);
        }
        let _ = std::fs::create_dir_all(state_dir);
        let _ = std::fs::write(&key_path, &seed);
        Self {
            signing: SigningKey::from_bytes(&seed),
        }
    }

    pub fn from_seed(seed: [u8; 32]) -> Self {
        Self {
            signing: SigningKey::from_bytes(&seed),
        }
    }

    pub fn public_bytes(&self) -> [u8; 32] {
        self.signing.verifying_key().to_bytes()
    }

    pub fn public_hex(&self) -> String {
        hex(&self.public_bytes())
    }

    pub fn sign(&self, msg: &[u8]) -> [u8; 64] {
        self.signing.sign(msg).to_bytes()
    }
}

pub fn fill_random(buf: &mut [u8]) -> bool {
    use std::io::Read;
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        if f.read_exact(buf).is_ok() {
            return true;
        }
    }
    // Fallback PRNG mixed with monotonic clock
    let mut s = super::clock::monotonic_ns() ^ 0x9E37_79B9_7F4A_7C15;
    for b in buf.iter_mut() {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        *b = (s & 0xff) as u8;
    }
    true
}

/// Per-connection pairing state for `pair-setup` and `pair-verify`.
pub struct PairingSession {
    identity: Identity,
    client_ed_pk: [u8; 32],
    client_x_pk: [u8; 32],
    server_x_pk: [u8; 32],
    ecdh_secret: Option<[u8; 32]>,
    aes_key: [u8; 16],
    aes_iv: [u8; 16],
}

impl PairingSession {
    pub fn new(identity: Identity) -> Self {
        Self {
            identity,
            client_ed_pk: [0u8; 32],
            client_x_pk: [0u8; 32],
            server_x_pk: [0u8; 32],
            ecdh_secret: None,
            aes_key: [0u8; 16],
            aes_iv: [0u8; 16],
        }
    }

    pub fn ecdh_secret(&self) -> Option<&[u8; 32]> {
        self.ecdh_secret.as_ref()
    }

    pub fn set_ecdh_secret(&mut self, secret: [u8; 32]) {
        self.ecdh_secret = Some(secret);
    }

    /// `POST /pair-setup`: returns our 32-byte Ed25519 public key.
    pub fn pair_setup(&mut self, req: &[u8]) -> [u8; 32] {
        if req.len() >= 32 {
            self.client_ed_pk.copy_from_slice(&req[..32]);
        }
        self.identity.public_bytes()
    }

    /// `POST /pair-verify`:
    /// - `req[0] == 1` (step 1, 68 bytes): returns 96 bytes (`server_x_pk || encrypted_sig`)
    /// - `req[0] == 0` (step 2, 68 bytes): verifies client signature, returns empty `Vec`
    pub fn pair_verify(&mut self, req: &[u8]) -> Result<Vec<u8>, &'static str> {
        if req.len() < 68 {
            return Err("pair-verify body too short");
        }
        match req[0] {
            1 => {
                self.client_x_pk.copy_from_slice(&req[4..36]);
                self.client_ed_pk.copy_from_slice(&req[36..68]);

                let mut x_seed = [0u8; 32];
                fill_random(&mut x_seed);
                let server_x_sk = XSecret::from(x_seed);
                let server_x_pk = XPublic::from(&server_x_sk);
                let client_x_pk = XPublic::from(self.client_x_pk);
                let shared = server_x_sk.diffie_hellman(&client_x_pk);

                self.server_x_pk = server_x_pk.to_bytes();
                let secret_bytes = shared.to_bytes();
                self.ecdh_secret = Some(secret_bytes);

                // Sign server_x_pk(32) || client_x_pk(32) with our Ed25519 identity key.
                let mut msg = [0u8; 64];
                msg[..32].copy_from_slice(&self.server_x_pk);
                msg[32..64].copy_from_slice(&self.client_x_pk);
                let mut sig = self.identity.sign(&msg);

                // Derive AES-128-CTR key & IV from SHA-512.
                self.aes_key = derive_sha512_16(b"Pair-Verify-AES-Key", &secret_bytes);
                self.aes_iv = derive_sha512_16(b"Pair-Verify-AES-IV", &secret_bytes);

                let mut cipher = Aes128Ctr::new((&self.aes_key).into(), (&self.aes_iv).into());
                cipher.apply_keystream(&mut sig);

                let mut out = Vec::with_capacity(96);
                out.extend_from_slice(&self.server_x_pk);
                out.extend_from_slice(&sig);
                Ok(out)
            }
            0 => {
                // Step 2 uses the SAME AES-CTR stream starting at byte offset 64 (after step 1's 64-byte sig).
                let mut cipher = Aes128Ctr::new((&self.aes_key).into(), (&self.aes_iv).into());
                let mut dummy = [0u8; 64];
                cipher.apply_keystream(&mut dummy);

                let mut client_sig = [0u8; 64];
                client_sig.copy_from_slice(&req[4..68]);
                cipher.apply_keystream(&mut client_sig);

                let mut msg = [0u8; 64];
                msg[..32].copy_from_slice(&self.client_x_pk);
                msg[32..64].copy_from_slice(&self.server_x_pk);

                let vk = VerifyingKey::from_bytes(&self.client_ed_pk)
                    .map_err(|_| "invalid client Ed25519 public key")?;
                let sig = Signature::from_bytes(&client_sig);
                vk.verify(&msg, &sig)
                    .map_err(|_| "pair-verify client signature mismatch")?;
                Ok(Vec::new())
            }
            _ => Err("unknown pair-verify phase"),
        }
    }
}

pub fn derive_sha512_16(prefix: &[u8], secret: &[u8]) -> [u8; 16] {
    let mut h = Sha512::new();
    h.update(prefix);
    h.update(secret);
    let digest = h.finalize();
    let mut out = [0u8; 16];
    out.copy_from_slice(&digest[..16]);
    out
}

/// Combine FairPlay decrypted `raw_aeskey(16)` with `ecdh_secret(32)` from `pair-verify`.
pub fn derive_session_aeskey(raw_aeskey: &[u8; 16], ecdh_secret: Option<&[u8; 32]>) -> [u8; 16] {
    if let Some(secret) = ecdh_secret {
        derive_sha512_16(raw_aeskey, secret)
    } else {
        *raw_aeskey
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pair_verify_roundtrip() {
        let server_id = Identity::from_seed([0x11; 32]);
        let client_id = Identity::from_seed([0x22; 32]);
        let mut session = PairingSession::new(server_id.clone());

        // Client generates ephemeral X25519
        let client_x_sk = XSecret::from([0x33; 32]);
        let client_x_pk = XPublic::from(&client_x_sk).to_bytes();
        let client_ed_pk = client_id.public_bytes();

        // Step 1 request
        let mut req1 = vec![1u8, 0, 0, 0];
        req1.extend_from_slice(&client_x_pk);
        req1.extend_from_slice(&client_ed_pk);
        let resp1 = session.pair_verify(&req1).expect("step 1");
        assert_eq!(resp1.len(), 96);

        let mut server_x_pk = [0u8; 32];
        server_x_pk.copy_from_slice(&resp1[..32]);
        let shared = client_x_sk
            .diffie_hellman(&XPublic::from(server_x_pk))
            .to_bytes();
        assert_eq!(session.ecdh_secret(), Some(&shared));

        let key = derive_sha512_16(b"Pair-Verify-AES-Key", &shared);
        let iv = derive_sha512_16(b"Pair-Verify-AES-IV", &shared);
        let mut cipher = Aes128Ctr::new((&key).into(), (&iv).into());

        // Client decrypts server sig (first 64 bytes of stream)
        let mut srv_sig = [0u8; 64];
        srv_sig.copy_from_slice(&resp1[32..96]);
        cipher.apply_keystream(&mut srv_sig);
        let mut srv_msg = [0u8; 64];
        srv_msg[..32].copy_from_slice(&server_x_pk);
        srv_msg[32..64].copy_from_slice(&client_x_pk);
        let srv_vk = VerifyingKey::from_bytes(&server_id.public_bytes()).unwrap();
        srv_vk
            .verify(&srv_msg, &Signature::from_bytes(&srv_sig))
            .expect("server sig valid");

        // Client signs client_x_pk || server_x_pk and encrypts with next 64 bytes of stream
        let mut cli_msg = [0u8; 64];
        cli_msg[..32].copy_from_slice(&client_x_pk);
        cli_msg[32..64].copy_from_slice(&server_x_pk);
        let mut cli_sig = client_id.sign(&cli_msg);
        cipher.apply_keystream(&mut cli_sig);

        let mut req2 = vec![0u8, 0, 0, 0];
        req2.extend_from_slice(&cli_sig);
        let resp2 = session.pair_verify(&req2).expect("step 2");
        assert!(resp2.is_empty());
    }
}
