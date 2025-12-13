// TRC-011: Keystore is fully implemented but not yet integrated with UI triggers
#![allow(dead_code)]

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::PathBuf;

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
    Argon2,
};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use directories::BaseDirs;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::error::{Result, RidgeError};

const SERVICE_NAME: &str = "ridge-control";
const KEYS_DIR: &str = "keys";
const KEYSTORE_FILE: &str = "keystore.enc";
const SALT_FILE: &str = "keystore.salt";

/// A secret value that is zeroed on drop
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[REDACTED]")
    }
}

/// Key identifier for different API providers
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub enum KeyId {
    Anthropic,
    OpenAI,
    Gemini,
    Grok,
    Groq,
    Custom(String),
}

impl KeyId {
    pub fn as_str(&self) -> &str {
        match self {
            KeyId::Anthropic => "anthropic",
            KeyId::OpenAI => "openai",
            KeyId::Gemini => "gemini",
            KeyId::Grok => "grok",
            KeyId::Groq => "groq",
            KeyId::Custom(s) => s,
        }
    }
}

impl std::fmt::Display for KeyId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Result of a key store operation
#[derive(Debug)]
pub enum KeyStoreResult {
    Success,
    NotFound,
    Error(String),
}

/// Backend storage strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyStoreBackend {
    /// Use system keyring (Secret Service on Linux)
    SystemKeyring,
    /// Use encrypted file storage
    EncryptedFile,
}

/// Encrypted keystore file format
#[derive(Serialize, Deserialize)]
struct EncryptedKeyStore {
    version: u32,
    nonce: [u8; 12],
    ciphertext: Vec<u8>,
}

/// Plaintext keystore (before encryption)
#[derive(Serialize, Deserialize, Default)]
struct KeyStoreData {
    keys: HashMap<String, String>,
}

/// Secure key storage with multiple backends
pub struct KeyStore {
    backend: KeyStoreBackend,
    keys_dir: PathBuf,
    /// Cached master password hash for encrypted backend
    master_key: Option<[u8; 32]>,
}

impl KeyStore {
    /// Create a new KeyStore, automatically selecting the best available backend
    pub fn new() -> Result<Self> {
        let keys_dir = Self::get_keys_dir()?;
        
        // Try system keyring first, fall back to encrypted file
        let backend = if Self::is_keyring_available() {
            tracing::info!("Using system keyring for key storage");
            KeyStoreBackend::SystemKeyring
        } else {
            tracing::info!("System keyring unavailable, using encrypted file storage");
            KeyStoreBackend::EncryptedFile
        };

        Ok(Self {
            backend,
            keys_dir,
            master_key: None,
        })
    }

    /// Create a KeyStore with a specific backend
    pub fn with_backend(backend: KeyStoreBackend) -> Result<Self> {
        let keys_dir = Self::get_keys_dir()?;
        Ok(Self {
            backend,
            keys_dir,
            master_key: None,
        })
    }

    /// Get the keys directory path
    fn get_keys_dir() -> Result<PathBuf> {
        BaseDirs::new()
            .map(|dirs| dirs.config_dir().join(SERVICE_NAME).join(KEYS_DIR))
            .ok_or_else(|| RidgeError::Config("Could not determine config directory".to_string()))
    }

    /// Check if system keyring is available
    fn is_keyring_available() -> bool {
        // Try to create a test entry - if it fails, keyring is not available
        let entry = keyring::Entry::new(SERVICE_NAME, "__test_availability__");
        match entry {
            Ok(e) => {
                // Try to get (will fail but tells us if backend is working)
                match e.get_password() {
                    Err(keyring::Error::NoEntry) => true,
                    Err(keyring::Error::NoStorageAccess(_)) => false,
                    Err(keyring::Error::PlatformFailure(_)) => false,
                    _ => true,
                }
            }
            Err(_) => false,
        }
    }

    /// Get the current backend
    pub fn backend(&self) -> KeyStoreBackend {
        self.backend
    }

    /// Store an API key
    pub fn store(&mut self, key_id: &KeyId, secret: &SecretString) -> Result<()> {
        match self.backend {
            KeyStoreBackend::SystemKeyring => self.store_keyring(key_id, secret),
            KeyStoreBackend::EncryptedFile => self.store_encrypted(key_id, secret),
        }
    }

    /// Retrieve an API key
    pub fn get(&self, key_id: &KeyId) -> Result<Option<SecretString>> {
        match self.backend {
            KeyStoreBackend::SystemKeyring => self.get_keyring(key_id),
            KeyStoreBackend::EncryptedFile => self.get_encrypted(key_id),
        }
    }

    /// Delete an API key
    pub fn delete(&mut self, key_id: &KeyId) -> Result<()> {
        match self.backend {
            KeyStoreBackend::SystemKeyring => self.delete_keyring(key_id),
            KeyStoreBackend::EncryptedFile => self.delete_encrypted(key_id),
        }
    }

    /// List all stored key IDs
    pub fn list(&self) -> Result<Vec<KeyId>> {
        match self.backend {
            KeyStoreBackend::SystemKeyring => self.list_keyring(),
            KeyStoreBackend::EncryptedFile => self.list_encrypted(),
        }
    }

    /// Check if a key exists
    pub fn exists(&self, key_id: &KeyId) -> Result<bool> {
        match self.get(key_id) {
            Ok(Some(_)) => Ok(true),
            Ok(None) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Unlock the encrypted keystore with a master password
    /// Only needed for EncryptedFile backend
    pub fn unlock(&mut self, master_password: &str) -> Result<()> {
        if self.backend != KeyStoreBackend::EncryptedFile {
            return Ok(());
        }

        let salt = self.load_or_create_salt()?;
        let key = self.derive_key(master_password, &salt)?;
        self.master_key = Some(key);

        // Verify the password by attempting to decrypt
        if self.keystore_file().exists() {
            let _ = self.load_encrypted_data()?;
        }

        Ok(())
    }

    /// Check if the keystore is unlocked
    pub fn is_unlocked(&self) -> bool {
        match self.backend {
            KeyStoreBackend::SystemKeyring => true,
            KeyStoreBackend::EncryptedFile => self.master_key.is_some(),
        }
    }

    /// Initialize encrypted storage with a new master password
    pub fn init_encrypted(&mut self, master_password: &str) -> Result<()> {
        self.ensure_keys_dir()?;

        let salt = SaltString::generate(&mut OsRng);
        let salt_path = self.salt_file();
        fs::write(&salt_path, salt.as_str())
            .map_err(|e| RidgeError::Config(format!("Failed to write salt: {}", e)))?;

        // Set restrictive permissions on salt file
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&salt_path, fs::Permissions::from_mode(0o600))
                .map_err(|e| RidgeError::Config(format!("Failed to set salt permissions: {}", e)))?;
        }

        let key = self.derive_key(master_password, &salt)?;
        self.master_key = Some(key);

        // Create empty keystore
        self.save_encrypted_data(&KeyStoreData::default())?;

        Ok(())
    }

    /// Change the master password
    pub fn change_password(&mut self, old_password: &str, new_password: &str) -> Result<()> {
        if self.backend != KeyStoreBackend::EncryptedFile {
            return Err(RidgeError::Config(
                "Password change only applies to encrypted file backend".to_string(),
            ));
        }

        // Unlock with old password first
        self.unlock(old_password)?;

        // Load current data
        let data = self.load_encrypted_data()?;

        // Create new salt and key
        let new_salt = SaltString::generate(&mut OsRng);
        let salt_path = self.salt_file();
        fs::write(&salt_path, new_salt.as_str())
            .map_err(|e| RidgeError::Config(format!("Failed to write salt: {}", e)))?;

        let new_key = self.derive_key(new_password, &new_salt)?;
        self.master_key = Some(new_key);

        // Re-encrypt data with new key
        self.save_encrypted_data(&data)?;

        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // System Keyring Backend
    // ─────────────────────────────────────────────────────────────────────────

    fn store_keyring(&self, key_id: &KeyId, secret: &SecretString) -> Result<()> {
        let entry = keyring::Entry::new(SERVICE_NAME, key_id.as_str())
            .map_err(|e| RidgeError::Config(format!("Failed to create keyring entry: {}", e)))?;

        entry
            .set_password(secret.expose())
            .map_err(|e| RidgeError::Config(format!("Failed to store key in keyring: {}", e)))?;

        Ok(())
    }

    fn get_keyring(&self, key_id: &KeyId) -> Result<Option<SecretString>> {
        let entry = keyring::Entry::new(SERVICE_NAME, key_id.as_str())
            .map_err(|e| RidgeError::Config(format!("Failed to create keyring entry: {}", e)))?;

        match entry.get_password() {
            Ok(password) => Ok(Some(SecretString::new(password))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(RidgeError::Config(format!(
                "Failed to retrieve key from keyring: {}",
                e
            ))),
        }
    }

    fn delete_keyring(&self, key_id: &KeyId) -> Result<()> {
        let entry = keyring::Entry::new(SERVICE_NAME, key_id.as_str())
            .map_err(|e| RidgeError::Config(format!("Failed to create keyring entry: {}", e)))?;

        match entry.delete_credential() {
            Ok(_) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()), // Already deleted
            Err(e) => Err(RidgeError::Config(format!(
                "Failed to delete key from keyring: {}",
                e
            ))),
        }
    }

    fn list_keyring(&self) -> Result<Vec<KeyId>> {
        // System keyring doesn't support listing, so we check known providers
        let known_ids = [
            KeyId::Anthropic,
            KeyId::OpenAI,
            KeyId::Gemini,
            KeyId::Grok,
            KeyId::Groq,
        ];

        let mut result = Vec::new();
        for id in &known_ids {
            if let Ok(Some(_)) = self.get_keyring(id) {
                result.push(id.clone());
            }
        }

        Ok(result)
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Encrypted File Backend
    // ─────────────────────────────────────────────────────────────────────────

    fn keystore_file(&self) -> PathBuf {
        self.keys_dir.join(KEYSTORE_FILE)
    }

    fn salt_file(&self) -> PathBuf {
        self.keys_dir.join(SALT_FILE)
    }

    fn ensure_keys_dir(&self) -> Result<()> {
        if !self.keys_dir.exists() {
            fs::create_dir_all(&self.keys_dir)
                .map_err(|e| RidgeError::Config(format!("Failed to create keys dir: {}", e)))?;

            // Set restrictive permissions on keys directory
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&self.keys_dir, fs::Permissions::from_mode(0o700)).map_err(
                    |e| RidgeError::Config(format!("Failed to set keys dir permissions: {}", e)),
                )?;
            }
        }
        Ok(())
    }

    fn load_or_create_salt(&self) -> Result<SaltString> {
        let salt_path = self.salt_file();

        if salt_path.exists() {
            let salt_str = fs::read_to_string(&salt_path)
                .map_err(|e| RidgeError::Config(format!("Failed to read salt: {}", e)))?;

            SaltString::from_b64(salt_str.trim())
                .map_err(|e| RidgeError::Config(format!("Invalid salt: {}", e)))
        } else {
            Err(RidgeError::Config(
                "Keystore not initialized. Call init_encrypted() first.".to_string(),
            ))
        }
    }

    fn derive_key(&self, password: &str, salt: &SaltString) -> Result<[u8; 32]> {
        let argon2 = Argon2::default();

        // Hash the password
        let hash = argon2
            .hash_password(password.as_bytes(), salt)
            .map_err(|e| RidgeError::Config(format!("Failed to derive key: {}", e)))?;

        // Extract the hash output (32 bytes)
        let output = hash
            .hash
            .ok_or_else(|| RidgeError::Config("No hash output".to_string()))?;

        let bytes = output.as_bytes();
        if bytes.len() < 32 {
            return Err(RidgeError::Config("Hash too short".to_string()));
        }

        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes[..32]);
        Ok(key)
    }

    fn load_encrypted_data(&self) -> Result<KeyStoreData> {
        let keystore_path = self.keystore_file();

        if !keystore_path.exists() {
            return Ok(KeyStoreData::default());
        }

        let master_key = self.master_key.ok_or_else(|| {
            RidgeError::Config("Keystore is locked. Call unlock() first.".to_string())
        })?;

        let encrypted_json = fs::read_to_string(&keystore_path)
            .map_err(|e| RidgeError::Config(format!("Failed to read keystore: {}", e)))?;

        let encrypted: EncryptedKeyStore = serde_json::from_str(&encrypted_json)
            .map_err(|e| RidgeError::Config(format!("Invalid keystore format: {}", e)))?;

        // Decrypt
        let cipher = ChaCha20Poly1305::new_from_slice(&master_key)
            .map_err(|e| RidgeError::Config(format!("Failed to create cipher: {}", e)))?;

        let nonce = Nonce::from_slice(&encrypted.nonce);
        let plaintext = cipher
            .decrypt(nonce, encrypted.ciphertext.as_ref())
            .map_err(|_| {
                RidgeError::Config("Failed to decrypt keystore (wrong password?)".to_string())
            })?;

        let data: KeyStoreData = serde_json::from_slice(&plaintext)
            .map_err(|e| RidgeError::Config(format!("Invalid keystore data: {}", e)))?;

        Ok(data)
    }

    fn save_encrypted_data(&self, data: &KeyStoreData) -> Result<()> {
        self.ensure_keys_dir()?;

        let master_key = self.master_key.ok_or_else(|| {
            RidgeError::Config("Keystore is locked. Call unlock() first.".to_string())
        })?;

        let plaintext = serde_json::to_vec(data)
            .map_err(|e| RidgeError::Config(format!("Failed to serialize keystore: {}", e)))?;

        // Generate random nonce
        let mut nonce_bytes = [0u8; 12];
        getrandom(&mut nonce_bytes)?;
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Encrypt
        let cipher = ChaCha20Poly1305::new_from_slice(&master_key)
            .map_err(|e| RidgeError::Config(format!("Failed to create cipher: {}", e)))?;

        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_ref())
            .map_err(|e| RidgeError::Config(format!("Failed to encrypt keystore: {}", e)))?;

        let encrypted = EncryptedKeyStore {
            version: 1,
            nonce: nonce_bytes,
            ciphertext,
        };

        let encrypted_json = serde_json::to_string_pretty(&encrypted)
            .map_err(|e| RidgeError::Config(format!("Failed to serialize encrypted data: {}", e)))?;

        let keystore_path = self.keystore_file();
        fs::write(&keystore_path, &encrypted_json)
            .map_err(|e| RidgeError::Config(format!("Failed to write keystore: {}", e)))?;

        // Set restrictive permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&keystore_path, fs::Permissions::from_mode(0o600)).map_err(
                |e| RidgeError::Config(format!("Failed to set keystore permissions: {}", e)),
            )?;
        }

        Ok(())
    }

    fn store_encrypted(&mut self, key_id: &KeyId, secret: &SecretString) -> Result<()> {
        let mut data = self.load_encrypted_data()?;
        data.keys.insert(key_id.as_str().to_string(), secret.expose().to_string());
        self.save_encrypted_data(&data)
    }

    fn get_encrypted(&self, key_id: &KeyId) -> Result<Option<SecretString>> {
        let data = self.load_encrypted_data()?;
        Ok(data.keys.get(key_id.as_str()).map(|s| SecretString::new(s.clone())))
    }

    fn delete_encrypted(&mut self, key_id: &KeyId) -> Result<()> {
        let mut data = self.load_encrypted_data()?;
        data.keys.remove(key_id.as_str());
        self.save_encrypted_data(&data)
    }

    fn list_encrypted(&self) -> Result<Vec<KeyId>> {
        let data = self.load_encrypted_data()?;
        Ok(data
            .keys
            .keys()
            .map(|k| match k.as_str() {
                "anthropic" => KeyId::Anthropic,
                "openai" => KeyId::OpenAI,
                "gemini" => KeyId::Gemini,
                "grok" => KeyId::Grok,
                "groq" => KeyId::Groq,
                other => KeyId::Custom(other.to_string()),
            })
            .collect())
    }
}

/// Get random bytes using the OS random source
fn getrandom(buf: &mut [u8]) -> Result<()> {
    use std::fs::File;
    
    #[cfg(unix)]
    {
        let mut f = File::open("/dev/urandom")
            .map_err(|e| RidgeError::Config(format!("Failed to open /dev/urandom: {}", e)))?;
        f.read_exact(buf)
            .map_err(|e| RidgeError::Config(format!("Failed to read random bytes: {}", e)))?;
    }
    
    #[cfg(not(unix))]
    {
        return Err(RidgeError::Config("Random number generation not supported on this platform".to_string()));
    }
    
    Ok(())
}

/// Prompt for master password from terminal
pub fn prompt_master_password(prompt: &str) -> Result<String> {
    rpassword::prompt_password(prompt)
        .map_err(|e| RidgeError::Config(format!("Failed to read password: {}", e)))
}

/// Prompt for master password with confirmation
pub fn prompt_new_password(prompt: &str, confirm_prompt: &str) -> Result<String> {
    loop {
        let password = rpassword::prompt_password(prompt)
            .map_err(|e| RidgeError::Config(format!("Failed to read password: {}", e)))?;
        
        if password.len() < 8 {
            eprintln!("Password must be at least 8 characters");
            continue;
        }
        
        let confirm = rpassword::prompt_password(confirm_prompt)
            .map_err(|e| RidgeError::Config(format!("Failed to read password: {}", e)))?;
        
        if password != confirm {
            eprintln!("Passwords do not match");
            continue;
        }
        
        return Ok(password);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_keystore() -> (KeyStore, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let keys_dir = temp_dir.path().to_path_buf();
        
        let mut store = KeyStore {
            backend: KeyStoreBackend::EncryptedFile,
            keys_dir: keys_dir.clone(),
            master_key: None,
        };
        
        // Initialize with test password
        let salt = SaltString::generate(&mut OsRng);
        fs::create_dir_all(&keys_dir).unwrap();
        fs::write(keys_dir.join(SALT_FILE), salt.as_str()).unwrap();
        
        let key = store.derive_key("test_password", &salt).unwrap();
        store.master_key = Some(key);
        
        // Create empty keystore
        store.save_encrypted_data(&KeyStoreData::default()).unwrap();
        
        (store, temp_dir)
    }

    #[test]
    fn test_secret_string_debug_redacted() {
        let secret = SecretString::new("my_api_key");
        let debug_str = format!("{:?}", secret);
        assert_eq!(debug_str, "[REDACTED]");
        assert!(!debug_str.contains("my_api_key"));
    }

    #[test]
    fn test_secret_string_expose() {
        let secret = SecretString::new("my_api_key");
        assert_eq!(secret.expose(), "my_api_key");
    }

    #[test]
    fn test_key_id_as_str() {
        assert_eq!(KeyId::Anthropic.as_str(), "anthropic");
        assert_eq!(KeyId::OpenAI.as_str(), "openai");
        assert_eq!(KeyId::Custom("mykey".to_string()).as_str(), "mykey");
    }

    #[test]
    fn test_store_and_retrieve_encrypted() {
        let (mut store, _temp) = test_keystore();
        
        let key_id = KeyId::Anthropic;
        let secret = SecretString::new("sk-ant-test-key-12345");
        
        store.store(&key_id, &secret).unwrap();
        
        let retrieved = store.get(&key_id).unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().expose(), "sk-ant-test-key-12345");
    }

    #[test]
    fn test_delete_encrypted() {
        let (mut store, _temp) = test_keystore();
        
        let key_id = KeyId::OpenAI;
        let secret = SecretString::new("sk-openai-test");
        
        store.store(&key_id, &secret).unwrap();
        assert!(store.exists(&key_id).unwrap());
        
        store.delete(&key_id).unwrap();
        assert!(!store.exists(&key_id).unwrap());
    }

    #[test]
    fn test_list_encrypted() {
        let (mut store, _temp) = test_keystore();
        
        store.store(&KeyId::Anthropic, &SecretString::new("key1")).unwrap();
        store.store(&KeyId::OpenAI, &SecretString::new("key2")).unwrap();
        
        let keys = store.list().unwrap();
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&KeyId::Anthropic));
        assert!(keys.contains(&KeyId::OpenAI));
    }

    #[test]
    fn test_get_nonexistent_key() {
        let (store, _temp) = test_keystore();
        
        let result = store.get(&KeyId::Gemini).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_encrypted_keystore_format() {
        let encrypted = EncryptedKeyStore {
            version: 1,
            nonce: [0u8; 12],
            ciphertext: vec![1, 2, 3, 4],
        };
        
        let json = serde_json::to_string(&encrypted).unwrap();
        let parsed: EncryptedKeyStore = serde_json::from_str(&json).unwrap();
        
        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.nonce, [0u8; 12]);
        assert_eq!(parsed.ciphertext, vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_key_derivation_produces_consistent_results() {
        let temp_dir = TempDir::new().unwrap();
        let keys_dir = temp_dir.path().to_path_buf();
        
        let store = KeyStore {
            backend: KeyStoreBackend::EncryptedFile,
            keys_dir,
            master_key: None,
        };
        
        let salt = SaltString::from_b64("dGVzdHNhbHQxMjM0NQ").unwrap();
        
        let key1 = store.derive_key("password123", &salt).unwrap();
        let key2 = store.derive_key("password123", &salt).unwrap();
        
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_different_passwords_produce_different_keys() {
        let temp_dir = TempDir::new().unwrap();
        let keys_dir = temp_dir.path().to_path_buf();
        
        let store = KeyStore {
            backend: KeyStoreBackend::EncryptedFile,
            keys_dir,
            master_key: None,
        };
        
        let salt = SaltString::from_b64("dGVzdHNhbHQxMjM0NQ").unwrap();
        
        let key1 = store.derive_key("password123", &salt).unwrap();
        let key2 = store.derive_key("password456", &salt).unwrap();
        
        assert_ne!(key1, key2);
    }
}
