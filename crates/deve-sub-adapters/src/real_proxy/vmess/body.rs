//! VMess AEAD body record framing.
//!
//! Each record: `[len(2 BE)][ciphertext + tag(16)]`. Nonce: `count(2 BE) || iv[2..12]`.
//! Request direction uses req_key/reqIV directly; response direction uses
//! SHA-256(req_key)[..16] / SHA-256(req_iv)[..16].

use aes_gcm::aead::Aead;
use aes_gcm::{Aes128Gcm, KeyInit as _, Nonce};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::header::response_body_keys;

const MAX_PLAINTEXT: usize = 16384 - 16;

enum RecordCipher {
    Aes128Gcm(Box<Aes128Gcm>),
}

impl RecordCipher {
    fn new(key: &[u8]) -> Self {
        // WHY: key is always 16 bytes from kdf16(), so new_from_slice is
        // infallible. A panic here would indicate a KDF contract violation.
        #[allow(clippy::expect_used, reason = "kdf16 guarantees 16-byte key")]
        let cipher = Aes128Gcm::new_from_slice(key).expect("16-byte key");
        Self::Aes128Gcm(Box::new(cipher))
    }

    fn seal(&self, nonce: &[u8; 12], plaintext: &[u8]) -> std::io::Result<Vec<u8>> {
        match self {
            Self::Aes128Gcm(c) => c
                .encrypt(Nonce::from_slice(nonce), plaintext)
                .map_err(|e| std::io::Error::other(format!("aes-gcm encrypt: {e}"))),
        }
    }

    fn open(&self, nonce: &[u8; 12], ciphertext: &[u8]) -> std::io::Result<Vec<u8>> {
        match self {
            Self::Aes128Gcm(c) => c
                .decrypt(Nonce::from_slice(nonce), ciphertext)
                .map_err(|e| std::io::Error::other(format!("aes-gcm decrypt: {e}"))),
        }
    }
}

fn record_nonce(iv: &[u8; 16], counter: u16) -> [u8; 12] {
    let mut nonce = [0u8; 12];
    nonce[..2].copy_from_slice(&counter.to_be_bytes());
    nonce[2..].copy_from_slice(&iv[2..12]);
    nonce
}

pub struct BodyCipher {
    write: RecordCipher,
    write_iv: [u8; 16],
    read: RecordCipher,
    read_iv: [u8; 16],
    write_counter: u16,
    read_counter: u16,
}

pub struct WriteHalf {
    cipher: RecordCipher,
    iv: [u8; 16],
    counter: u16,
}

pub struct ReadHalf {
    cipher: RecordCipher,
    iv: [u8; 16],
    counter: u16,
}

impl WriteHalf {
    pub async fn write_record<W: AsyncWrite + Unpin>(
        &mut self,
        writer: &mut W,
        plaintext: &[u8],
    ) -> std::io::Result<()> {
        let nonce = record_nonce(&self.iv, self.counter);
        self.counter = self.counter.wrapping_add(1);
        let ct = self.cipher.seal(&nonce, plaintext)?;
        let len = ct.len() as u16;
        writer.write_all(&len.to_be_bytes()).await?;
        writer.write_all(&ct).await?;
        writer.flush().await
    }
}

impl ReadHalf {
    pub async fn read_record<R: AsyncRead + Unpin>(
        &mut self,
        reader: &mut R,
    ) -> std::io::Result<Vec<u8>> {
        let mut len_buf = [0u8; 2];
        reader.read_exact(&mut len_buf).await?;
        let ct_len = u16::from_be_bytes(len_buf) as usize;
        if ct_len == 0 || ct_len > MAX_PLAINTEXT + 16 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "body record length out of range",
            ));
        }
        let mut ct = vec![0u8; ct_len];
        reader.read_exact(&mut ct).await?;
        let nonce = record_nonce(&self.iv, self.counter);
        self.counter = self.counter.wrapping_add(1);
        self.cipher.open(&nonce, &ct)
    }
}

impl BodyCipher {
    /// Client-side cipher: `write` encrypts with req_key (server reads),
    /// `read` decrypts with response keys (server writes).
    pub fn new(req_key: &[u8; 16], req_iv: &[u8; 16]) -> Self {
        let write = RecordCipher::new(req_key);
        let write_iv = *req_iv;
        let (resp_key, resp_iv) = response_body_keys(req_key, req_iv);
        let read = RecordCipher::new(&resp_key);
        Self {
            write,
            write_iv,
            read,
            read_iv: resp_iv,
            write_counter: 0,
            read_counter: 0,
        }
    }

    /// Server-side cipher: directions swapped relative to [`Self::new`].
    /// `read` decrypts client requests with req_key, `write` encrypts
    /// server responses with response keys.
    #[cfg(test)]
    pub fn new_server(req_key: &[u8; 16], req_iv: &[u8; 16]) -> Self {
        let read = RecordCipher::new(req_key);
        let read_iv = *req_iv;
        let (resp_key, resp_iv) = response_body_keys(req_key, req_iv);
        let write = RecordCipher::new(&resp_key);
        Self {
            write,
            write_iv: resp_iv,
            read,
            read_iv,
            write_counter: 0,
            read_counter: 0,
        }
    }

    pub fn split(self) -> (WriteHalf, ReadHalf) {
        (
            WriteHalf {
                cipher: self.write,
                iv: self.write_iv,
                counter: self.write_counter,
            },
            ReadHalf {
                cipher: self.read,
                iv: self.read_iv,
                counter: self.read_counter,
            },
        )
    }

    #[must_use]
    pub const fn max_plaintext() -> usize {
        MAX_PLAINTEXT
    }
}
