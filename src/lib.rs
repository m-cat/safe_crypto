// Copyright 2018 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

extern crate maidsafe_utilities;
extern crate rust_sodium;
extern crate serde;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate quick_error;
#[macro_use]
extern crate unwrap;

use maidsafe_utilities::serialisation::{deserialise, serialise, SerialisationError};
use rust_sodium::crypto::{box_, sealedbox, sign};
use serde::{de::DeserializeOwned, Serialize};
use std::sync::Arc;

#[derive(Serialize, Deserialize, Clone)]
pub struct PackedNonce {
    nonce: [u8; box_::NONCEBYTES],
    ciphertext: Vec<u8>,
}

#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Clone)]
pub struct PublicId {
    sign: sign::PublicKey,
    encrypt: box_::PublicKey,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct SecretId {
    inner: Arc<SecretIdInner>,
    public: PublicId,
}

#[derive(Debug, PartialEq, Eq, Clone)]
struct SecretIdInner {
    sign: sign::SecretKey,
    encrypt: box_::SecretKey,
}

#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Clone)]
pub struct Signature {
    signature: sign::Signature,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct SharedSecretKey {
    precomputed: Arc<box_::PrecomputedKey>,
}

impl PublicId {
    pub fn encrypt_anonymous<T>(&self, plaintext: &T) -> Result<Vec<u8>, EncryptError>
    where
        T: Serialize,
    {
        let bytes = serialise(plaintext).map_err(EncryptError::Serialisation)?;
        Ok(self.encrypt_anonymous_bytes(&bytes))
    }

    pub fn encrypt_anonymous_bytes(&self, plaintext: &[u8]) -> Vec<u8> {
        sealedbox::seal(plaintext, &self.encrypt)
    }

    pub fn verify_detached(&self, signature: &sign::Signature, data: &[u8]) -> bool {
        sign::verify_detached(signature, data, &self.sign)
    }
}

#[cfg_attr(feature = "cargo-clippy", allow(new_without_default))]
impl SecretId {
    pub fn new() -> SecretId {
        let (sign_pk, sign_sk) = sign::gen_keypair();
        let (encrypt_pk, encrypt_sk) = box_::gen_keypair();
        let public = PublicId {
            sign: sign_pk,
            encrypt: encrypt_pk,
        };
        SecretId {
            public,
            inner: Arc::new(SecretIdInner {
                sign: sign_sk,
                encrypt: encrypt_sk,
            }),
        }
    }

    pub fn public_id(&self) -> &PublicId {
        &self.public
    }

    pub fn decrypt_anonymous<T>(&self, cyphertext: &[u8]) -> Result<T, DecryptError>
    where
        T: Serialize + DeserializeOwned,
    {
        let bytes = self
            .decrypt_anonymous_bytes(cyphertext)
            .map_err(|e| match e {
                DecryptBytesError::DecryptVerify => DecryptError::DecryptVerify,
                DecryptBytesError::Deserialisation(cause) => DecryptError::Deserialisation(cause),
            })?;
        deserialise(&bytes).map_err(DecryptError::Deserialisation)
    }

    pub fn decrypt_anonymous_bytes(&self, cyphertext: &[u8]) -> Result<Vec<u8>, DecryptBytesError> {
        sealedbox::open(cyphertext, &self.public.encrypt, &self.inner.encrypt)
            .map_err(|()| DecryptBytesError::DecryptVerify)
    }

    pub fn sign_detached(&self, data: &[u8]) -> sign::Signature {
        sign::sign_detached(data, &self.inner.sign)
    }

    pub fn shared_key(&self, their_pk: &PublicId) -> SharedSecretKey {
        let precomputed = box_::precompute(&their_pk.encrypt, &self.inner.encrypt);
        SharedSecretKey {
            precomputed: Arc::new(precomputed),
        }
    }
}

impl SharedSecretKey {
    pub fn encrypt_bytes(&self, plaintext: &[u8]) -> Result<Vec<u8>, EncryptError> {
        let nonce = box_::gen_nonce();
        let ciphertext = box_::seal_precomputed(plaintext, &nonce, &self.precomputed);
        Ok(serialise(&PackedNonce {
            nonce: nonce.0,
            ciphertext,
        }).map_err(EncryptError::Serialisation)?)
    }

    pub fn encrypt<T>(&self, plaintext: &T) -> Result<Vec<u8>, EncryptError>
    where
        T: Serialize,
    {
        let bytes = serialise(plaintext).map_err(EncryptError::Serialisation)?;
        self.encrypt_bytes(&bytes)
    }

    pub fn decrypt_bytes(&self, encoded: &[u8]) -> Result<Vec<u8>, DecryptBytesError> {
        let PackedNonce { nonce, ciphertext } =
            deserialise(encoded).map_err(DecryptBytesError::Deserialisation)?;
        box_::open_precomputed(&ciphertext, &box_::Nonce(nonce), &self.precomputed)
            .map_err(|()| DecryptBytesError::DecryptVerify)
    }

    pub fn decrypt<T>(&self, cyphertext: &[u8]) -> Result<T, DecryptError>
    where
        T: Serialize + DeserializeOwned,
    {
        let bytes = self.decrypt_bytes(cyphertext).map_err(|e| match e {
            DecryptBytesError::DecryptVerify => DecryptError::DecryptVerify,
            DecryptBytesError::Deserialisation(cause) => DecryptError::Deserialisation(cause),
        })?;
        deserialise(&bytes).map_err(DecryptError::Deserialisation)
    }
}

quick_error! {
    #[derive(Debug)]
    pub enum EncryptError {
        Serialisation(e: SerialisationError) {
            description("error serializing message")
            display("error serializing message: {}", e)
            cause(e)
        }
    }
}

quick_error! {
    #[derive(Debug)]
    pub enum DecryptError {
        DecryptVerify {
            description("error decrypting/verifying message")
        }
        Deserialisation(e: SerialisationError) {
            description("error deserializing decrypted message")
            display("error deserializing decrypted message: {}", e)
            cause(e)
        }
    }
}

quick_error! {
    #[derive(Debug)]
    pub enum DecryptBytesError {
        DecryptVerify {
            description("error decrypting/verifying message")
        }
        Deserialisation(e: SerialisationError) {
            description("error deserializing decrypted message")
            display("error deserializing decrypted message: {}", e)
            cause(e)
        }
    }
}
