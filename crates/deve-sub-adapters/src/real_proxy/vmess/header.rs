//! VMess AEAD request/response header sealing.
//!
//! Mirrors v2ray `SealVMessAEADHeader` / `OpenVMessAEADHeader`. Wire layout:
//! `auth_id(16) || encrypted_length(2+16) || conn_nonce(8) || encrypted_header(N+16)`.

use aes::Aes128;
use aes::cipher::{BlockEncrypt, KeyInit};
use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes128Gcm, Nonce};
use md5::{Digest, Md5};
use rand::RngCore;
use sha2::Sha256;
use tokio::io::AsyncReadExt as _;

use super::kdf::{kdf12, kdf16};

const VMESS_MAGIC: &[u8] = b"c48619fe-8f02-49e0-b9e9-edf763e17e21";
const CMD_TCP: u8 = 0x01;
const ADDR_IPV4: u8 = 0x01;
const ADDR_DOMAIN: u8 = 0x02;
const ADDR_IPV6: u8 = 0x03;
const OPT_STANDARD: u8 = 0x01;
const SECURITY_AES_128_GCM: u8 = 0x03;

pub fn cmd_key(uuid: &[u8; 16]) -> [u8; 16] {
    let mut hasher = Md5::new();
    hasher.update(uuid);
    hasher.update(VMESS_MAGIC);
    hasher.finalize().into()
}

pub struct SealedHeader {
    pub bytes: Vec<u8>,
    pub req_key: [u8; 16],
    pub req_iv: [u8; 16],
    pub resp_v: u8,
}

/// WHY: the `.expect()` calls below are infallible — `kdf16` always
/// produces a 16-byte key (AES-128-GCM contract) and AES-GCM `encrypt`
/// only fails on nonce/key length mismatch, which the KDF prevents.
#[allow(clippy::expect_used, reason = "kdf16/kdf12 guarantee correct lengths")]
pub fn seal_request_header(
    cmd_key: &[u8; 16],
    dst_port: u16,
    host: &str,
    rng: &mut impl RngCore,
) -> SealedHeader {
    let mut req_key = [0u8; 16];
    let mut req_iv = [0u8; 16];
    rng.fill_bytes(&mut req_key);
    rng.fill_bytes(&mut req_iv);
    let resp_v: u8 = rng.next_u32() as u8;
    let mut conn_nonce = [0u8; 8];
    rng.fill_bytes(&mut conn_nonce);

    let auth_id = build_auth_id(cmd_key, rng);
    let plaintext = build_header_plaintext(&req_key, &req_iv, resp_v, dst_port, host, rng);

    let header_key = kdf16(cmd_key, &[b"VMess Header AEAD Key", &auth_id, &conn_nonce]);
    let header_iv = kdf12(
        cmd_key,
        &[b"VMess Header AEAD Nonce", &auth_id, &conn_nonce],
    );
    let length_key = kdf16(
        cmd_key,
        &[b"VMess Header AEAD Key_Length", &auth_id, &conn_nonce],
    );
    let length_iv = kdf12(
        cmd_key,
        &[b"VMess Header AEAD Nonce_Length", &auth_id, &conn_nonce],
    );

    let header_cipher = Aes128Gcm::new_from_slice(&header_key).expect("16-byte key");
    let encrypted_header = header_cipher
        .encrypt(
            Nonce::from_slice(&header_iv),
            Payload {
                msg: &plaintext,
                aad: &auth_id,
            },
        )
        .expect("header encrypt");

    let header_len = plaintext.len() as u16;
    let length_cipher = Aes128Gcm::new_from_slice(&length_key).expect("16-byte key");
    let encrypted_length = length_cipher
        .encrypt(
            Nonce::from_slice(&length_iv),
            Payload {
                msg: &header_len.to_be_bytes(),
                aad: &auth_id,
            },
        )
        .expect("length encrypt");

    let mut out = Vec::with_capacity(16 + encrypted_length.len() + 8 + encrypted_header.len());
    out.extend_from_slice(&auth_id);
    out.extend_from_slice(&encrypted_length);
    out.extend_from_slice(&conn_nonce);
    out.extend_from_slice(&encrypted_header);

    SealedHeader {
        bytes: out,
        req_key,
        req_iv,
        resp_v,
    }
}

pub fn response_body_keys(req_key: &[u8; 16], req_iv: &[u8; 16]) -> ([u8; 16], [u8; 16]) {
    let bk: [u8; 32] = Sha256::digest(req_key).into();
    let bi: [u8; 32] = Sha256::digest(req_iv).into();
    let mut resp_key = [0u8; 16];
    let mut resp_iv = [0u8; 16];
    resp_key.copy_from_slice(&bk[..16]);
    resp_iv.copy_from_slice(&bi[..16]);
    (resp_key, resp_iv)
}

pub async fn read_aead_response_header<R: tokio::io::AsyncRead + Unpin>(
    rd: &mut R,
    resp_body_key: &[u8; 16],
    resp_body_iv: &[u8; 16],
    resp_v: u8,
) -> std::io::Result<()> {
    let invalid = |msg: &'static str| std::io::Error::new(std::io::ErrorKind::InvalidData, msg);

    let len_key = kdf16(resp_body_key, &[b"AEAD Resp Header Len Key"]);
    let len_iv = kdf12(resp_body_iv, &[b"AEAD Resp Header Len IV"]);
    let mut len_ct = [0u8; 18];
    rd.read_exact(&mut len_ct).await?;
    let len_cipher = Aes128Gcm::new_from_slice(&len_key).map_err(|_| invalid("len cipher init"))?;
    let len_pt = len_cipher
        .decrypt(Nonce::from_slice(&len_iv), len_ct.as_ref())
        .map_err(|_| invalid("response length AEAD open failed"))?;
    if len_pt.len() != 2 {
        return Err(invalid("response length not 2 bytes"));
    }
    let hdr_len = u16::from_be_bytes([len_pt[0], len_pt[1]]) as usize;

    let hdr_key = kdf16(resp_body_key, &[b"AEAD Resp Header Key"]);
    let hdr_iv = kdf12(resp_body_iv, &[b"AEAD Resp Header IV"]);
    let mut hdr_ct = vec![0u8; hdr_len + 16];
    rd.read_exact(&mut hdr_ct).await?;
    let hdr_cipher = Aes128Gcm::new_from_slice(&hdr_key).map_err(|_| invalid("hdr cipher init"))?;
    let hdr = hdr_cipher
        .decrypt(Nonce::from_slice(&hdr_iv), hdr_ct.as_ref())
        .map_err(|_| invalid("response header AEAD open failed"))?;
    if hdr.first() != Some(&resp_v) {
        return Err(invalid("response header verification byte mismatch"));
    }
    Ok(())
}

/// WHY: `kdf16` produces exactly 16 bytes, so AES-128 `new_from_slice`
/// is infallible.
#[allow(clippy::expect_used, reason = "kdf16 guarantees 16-byte key")]
fn build_auth_id(cmd_key: &[u8; 16], rng: &mut impl RngCore) -> [u8; 16] {
    let auth_id_key = kdf16(cmd_key, &[b"AES Auth ID Encryption"]);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut block = [0u8; 16];
    block[..8].copy_from_slice(&now.to_be_bytes());
    rng.fill_bytes(&mut block[8..12]);
    let crc = crc32fast::hash(&block[..12]);
    block[12..16].copy_from_slice(&crc.to_be_bytes());

    let aes = Aes128::new_from_slice(&auth_id_key).expect("16-byte key");
    aes.encrypt_block(aes::Block::from_mut_slice(&mut block));
    block
}

fn build_header_plaintext(
    req_key: &[u8; 16],
    req_iv: &[u8; 16],
    resp_v: u8,
    dst_port: u16,
    host: &str,
    rng: &mut impl RngCore,
) -> Vec<u8> {
    let padding_len = (rng.next_u32() % 16) as u8;
    let mut buf = Vec::with_capacity(64);
    buf.push(0x01);
    buf.extend_from_slice(req_iv);
    buf.extend_from_slice(req_key);
    buf.push(resp_v);
    buf.push(OPT_STANDARD);
    buf.push((padding_len << 4) | SECURITY_AES_128_GCM);
    buf.push(0x00);
    buf.push(CMD_TCP);
    buf.extend_from_slice(&dst_port.to_be_bytes());
    encode_address(&mut buf, host);
    if padding_len > 0 {
        let mut pad = [0u8; 15];
        rng.fill_bytes(&mut pad[..padding_len as usize]);
        buf.extend_from_slice(&pad[..padding_len as usize]);
    }
    let hash = fnv1a32(&buf);
    buf.extend_from_slice(&hash.to_be_bytes());
    buf
}

fn encode_address(buf: &mut Vec<u8>, host: &str) {
    if let Ok(ipv4) = host.parse::<std::net::Ipv4Addr>() {
        buf.push(ADDR_IPV4);
        buf.extend_from_slice(&ipv4.octets());
    } else if let Ok(ipv6) = host.parse::<std::net::Ipv6Addr>() {
        buf.push(ADDR_IPV6);
        buf.extend_from_slice(&ipv6.octets());
    } else {
        buf.push(ADDR_DOMAIN);
        buf.push(host.len() as u8);
        buf.extend_from_slice(host.as_bytes());
    }
}

fn fnv1a32(data: &[u8]) -> u32 {
    let mut hash = 0x811c_9dc5u32;
    for &byte in data {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}
