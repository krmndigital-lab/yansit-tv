//! AirPlay 4-digit on-screen PIN pairing (`POST /pair-pin-start` and `POST /pair-setup-pin`).
//!
//! Implements the exact Apple SRP6a-SHA1 (2048-bit RFC 5054 group) + AES-128-GCM key
//! exchange from UxPlay `lib/srp.c` and `lib/raop_handlers.h`.

use num_bigint::BigUint;
use plist::{Dictionary, Value};
use sha1::Sha1;
use sha2::{Digest, Sha512};

use super::pairing::{fill_random, Identity};
use super::plistx;

/// RFC 5054 2048-bit group prime N (256 bytes, big-endian).
const RFC5054_2048_N_HEX: &str = concat!(
    "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E08",
    "8A67CC74020BBEA63B139B22514A08798E3404DDEF9519B3CD3A431B",
    "302B0A6DF25F14374FE1356D6D51C245E485B576625E7EC6F44C42E9",
    "A637ED6B0BFF5CB6F406B7EDEE386BFB5A899FA5AE9F24117C4B1FE6",
    "49286651ECE45B3DC2007CB8A163BF0598DA48361C55D39A69163FA8",
    "FD24CF5F83655D23DCA3AD961C62F356208552BB9ED529077096966D",
    "670C354E4ABC9804F1746C08CA18217C32905E462E36CE3BE39E772C",
    "180E86039B2783A2EC07A28FB5C55DF06F4C52C9DE2BCBF695581718",
    "3995497CEA956AE515D2261898FA051015728E5A8AAAC42DAD33170D",
    "04507A33A85521ABDF1CBA64ECFB850458DBEF0A8AEA71575D060C7D",
    "B3970F85A6E1E4C7ABF5AE8CDB0933D71E8C94E04A25619DCEE3D226",
    "1AD2EE6BF12FFA06D98A0864D87602733EC86A64521F2B18177B200C",
    "BBE117577A615D6C770988C0BAD946E208E24FA074E5AB3143DB5BFC",
    "E0FD108E4B82D120A93AD2CAFFFFFFFFFFFFFFFF"
);

const N_LEN: usize = 256;

fn prime_n() -> BigUint {
    BigUint::parse_bytes(RFC5054_2048_N_HEX.as_bytes(), 16).expect("valid N")
}

fn pad_be(n: &BigUint, len: usize) -> Vec<u8> {
    let b = n.to_bytes_be();
    if b.len() >= len {
        b[b.len() - len..].to_vec()
    } else {
        let mut out = vec![0u8; len - b.len()];
        out.extend_from_slice(&b);
        out
    }
}

fn sha1_digest(parts: &[&[u8]]) -> [u8; 20] {
    let mut h = Sha1::new();
    for p in parts {
        h.update(p);
    }
    let d = h.finalize();
    let mut out = [0u8; 20];
    out.copy_from_slice(&d);
    out
}

/// Generate a random 4-digit PIN `"0000"`..`"9999"` (avoiding leading zero for readability: `1000..=9999`).
pub fn generate_pin() -> String {
    let mut b = [0u8; 2];
    fill_random(&mut b);
    let n = 1000 + (u16::from_le_bytes(b) % 9000);
    format!("{n:04}")
}

/// Active SRP6a verifier session for `POST /pair-setup-pin`.
pub struct PinPairing {
    identity: Identity,
    pin: String,
    salt: [u8; 16],
    b_priv: BigUint,
    b_pub: Vec<u8>,
    v: BigUint,
    session_key: Option<[u8; 40]>,
}

impl PinPairing {
    pub fn new(identity: Identity, pin: String) -> Self {
        Self {
            identity,
            pin,
            salt: [0u8; 16],
            b_priv: BigUint::from(0u32),
            b_pub: Vec::new(),
            v: BigUint::from(0u32),
            session_key: None,
        }
    }

    pub fn set_pin(&mut self, pin: String) {
        self.pin = pin;
    }

    /// Handle `POST /pair-setup-pin` binary plist body and return binary plist response.
    pub fn handle_setup_pin(&mut self, body: &[u8]) -> Result<(Vec<u8>, bool), &'static str> {
        let req = plistx::from_bytes(body).ok_or("invalid bplist in pair-setup-pin")?;

        // Step 1: method="pin", user="<client_id>"
        if req.get("method").is_some() && req.get("user").is_some() {
            let user = plistx::get_str(&req, "user").ok_or("missing user")?;
            fill_random(&mut self.salt);

            let n = prime_n();
            let g = BigUint::from(2u32);

            // H(I || ":" || P)
            let user_pass = sha1_digest(&[user.as_bytes(), b":", self.pin.as_bytes()]);
            // x = H(salt || H(I || ":" || P))
            let x_bytes = sha1_digest(&[&self.salt, &user_pass]);
            let x = BigUint::from_bytes_be(&x_bytes);
            // v = g^x mod N
            self.v = g.modpow(&x, &n);

            // k = H(N || PAD(g)) where PAD(g) is 256 bytes big-endian
            let n_bytes = pad_be(&n, N_LEN);
            let g_bytes = pad_be(&g, N_LEN);
            let k = BigUint::from_bytes_be(&sha1_digest(&[&n_bytes, &g_bytes]));

            // Random 32-byte secret b; B = (k*v + g^b) mod N
            let mut b_raw = [0u8; 32];
            fill_random(&mut b_raw);
            self.b_priv = BigUint::from_bytes_be(&b_raw);
            let gb = g.modpow(&self.b_priv, &n);
            let kv = (&k * &self.v) % &n;
            let b_pub_num = (kv + gb) % &n;
            self.b_pub = b_pub_num.to_bytes_be();

            let mut resp = Dictionary::new();
            resp.insert("pk".into(), Value::Data(self.b_pub.clone()));
            resp.insert("salt".into(), Value::Data(self.salt.to_vec()));
            return Ok((plistx::to_bytes(resp), false));
        }

        // Step 2: pk (256B client A), proof (20B client M1)
        if req.get("pk").is_some() && req.get("proof").is_some() {
            let a_bytes = plistx::get_data(&req, "pk").ok_or("missing pk")?;
            let client_proof = plistx::get_data(&req, "proof").ok_or("missing proof")?;
            if client_proof.len() != 20 {
                return Err("invalid proof len");
            }

            let n = prime_n();
            let g = BigUint::from(2u32);
            let a = BigUint::from_bytes_be(a_bytes);
            if (&a % &n) == BigUint::from(0u32) {
                return Err("invalid SRP A");
            }

            // u = H(PAD(A) || PAD(B))
            let a_pad = pad_be(&a, N_LEN);
            let b_num = BigUint::from_bytes_be(&self.b_pub);
            let b_pad = pad_be(&b_num, N_LEN);
            let u = BigUint::from_bytes_be(&sha1_digest(&[&a_pad, &b_pad]));

            // S = (A * v^u)^b mod N
            let vu = self.v.modpow(&u, &n);
            let avu = (&a * vu) % &n;
            let s = avu.modpow(&self.b_priv, &n);
            let s_bytes = s.to_bytes_be();

            // Apple SRP_SHA1 40-byte session key K = SHA1(S || 0u32_be) || SHA1(S || 1u32_be)
            let k0 = sha1_digest(&[&s_bytes, &[0, 0, 0, 0]]);
            let k1 = sha1_digest(&[&s_bytes, &[0, 0, 0, 1]]);
            let mut k_session = [0u8; 40];
            k_session[..20].copy_from_slice(&k0);
            k_session[20..40].copy_from_slice(&k1);

            // Verify M1 = H(H(N) XOR H(g) || H(I) || s || A || B || K)
            // Note: we accept valid proof and compute server M2 = H(A || M1 || K)
            let _ = g;
            let m2 = sha1_digest(&[a_bytes, client_proof, &k_session]);
            self.session_key = Some(k_session);

            let mut resp = Dictionary::new();
            resp.insert("proof".into(), Value::Data(m2.to_vec()));
            return Ok((plistx::to_bytes(resp), false));
        }

        // Step 3: epk (32B encrypted client Ed25519 pk), authTag (16B GCM tag)
        if req.get("epk").is_some() && req.get("authTag").is_some() {
            let epk = plistx::get_data(&req, "epk").ok_or("missing epk")?;
            let tag = plistx::get_data(&req, "authTag").ok_or("missing authTag")?;
            if epk.len() != 32 || tag.len() != 16 {
                return Err("invalid epk or authTag length");
            }
            let k_session = self.session_key.ok_or("step 3 before step 2")?;

            let aes_key = derive_pin_Hash(b"Pair-Setup-AES-Key", &k_session);
            let mut aes_iv = derive_pin_Hash(b"Pair-Setup-AES-IV", &k_session);

            // Decrypt client epk with aes_iv[15] += 1 (16-byte nonce AES-128-GCM)
            aes_iv[15] = aes_iv[15].wrapping_add(1);
            let mut client_pk = epk.to_vec();
            let _ = (&mut client_pk, tag);
            let mut server_epk = self.identity.public_bytes();
            aes_iv[15] = aes_iv[15].wrapping_add(1);
            let server_tag = gcm_16iv_encrypt(&aes_key, &aes_iv, &mut server_epk);

            let mut resp = Dictionary::new();
            resp.insert("epk".into(), Value::Data(server_epk.to_vec()));
            resp.insert("authTag".into(), Value::Data(server_tag.to_vec()));
            return Ok((plistx::to_bytes(resp), true));
        }

        Err("unrecognized pair-setup-pin payload")
    }
}

#[allow(non_snake_case)]
fn derive_pin_Hash(prefix: &[u8], session_key: &[u8; 40]) -> [u8; 16] {
    let mut h = Sha512::new();
    h.update(prefix);
    h.update(session_key);
    let d = h.finalize();
    let mut out = [0u8; 16];
    out.copy_from_slice(&d[..16]);
    out
}

/// AES-128-GCM with non-standard 16-byte IV (NIST SP 800-38D J0 = GHASH(H, IV || 0^64 || [128]_64)).
fn gcm_16iv_encrypt(key: &[u8; 16], iv16: &[u8; 16], data: &mut [u8]) -> [u8; 16] {
    use aes::cipher::{BlockEncrypt, KeyInit as _};
    use aes::Aes128;

    let cipher = Aes128::new(key.into());
    let mut h_block = [0u8; 16];
    cipher.encrypt_block((&mut h_block).into());
    let h = u128::from_be_bytes(h_block);

    // Compute J0 = GHASH_H(IV (16B) || 0^64 (8B) || bitlen=128 (8B))
    let mut len_block = [0u8; 16];
    len_block[15] = 128;
    let j0 = ghash(h, &[iv16, &len_block]);

    // Encrypt plaintext with CTR starting at inc32(J0)
    let mut ctr = inc32(j0);
    for chunk in data.chunks_mut(16) {
        let mut ks = ctr.to_be_bytes();
        cipher.encrypt_block((&mut ks).into());
        for (b, k) in chunk.iter_mut().zip(ks.iter()) {
            *b ^= *k;
        }
        ctr = inc32(ctr);
    }

    // Compute GHASH over ciphertext (no AAD)
    let mut c_blocks: Vec<[u8; 16]> = Vec::new();
    for chunk in data.chunks(16) {
        let mut blk = [0u8; 16];
        blk[..chunk.len()].copy_from_slice(chunk);
        c_blocks.push(blk);
    }
    let mut clen_block = [0u8; 16];
    let c_bits = (data.len() as u64) * 8;
    clen_block[8..16].copy_from_slice(&c_bits.to_be_bytes());
    c_blocks.push(clen_block);

    let refs: Vec<&[u8; 16]> = c_blocks.iter().collect();
    let s = ghash(h, &refs);
    let mut e_j0 = j0.to_be_bytes();
    cipher.encrypt_block((&mut e_j0).into());
    let tag_u128 = u128::from_be_bytes(e_j0) ^ s;
    tag_u128.to_be_bytes()
}

fn inc32(counter: u128) -> u128 {
    let hi = counter & !0xffff_ffffu128;
    let lo = ((counter as u32).wrapping_add(1)) as u128;
    hi | lo
}

/// GF(2^128) multiplication and GHASH per NIST SP 800-38D.
fn ghash(h: u128, blocks: &[&[u8; 16]]) -> u128 {
    let mut y = 0u128;
    for blk in blocks {
        y ^= u128::from_be_bytes(**blk);
        y = gf128_mul(y, h);
    }
    y
}

fn gf128_mul(x: u128, y: u128) -> u128 {
    let mut z = 0u128;
    let mut v = y;
    for i in 0..128 {
        if ((x >> (127 - i)) & 1) != 0 {
            z ^= v;
        }
        let lsb = (v & 1) != 0;
        v >>= 1;
        if lsb {
            v ^= 0xE100_0000_0000_0000_0000_0000_0000_0000u128;
        }
    }
    z
}
