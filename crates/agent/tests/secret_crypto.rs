use agent::llm::secret::{ApiKeyCipher, StaticKeyMaterialSource};

#[test]
fn api_key_cipher_round_trips_with_fixed_key_material() {
    let cipher = ApiKeyCipher::new(StaticKeyMaterialSource::new(b"fixed-test-key".to_vec()));
    let encrypted = cipher.encrypt("sk-secret").expect("secret should encrypt");
    let decrypted = cipher
        .decrypt(&encrypted.nonce, &encrypted.ciphertext)
        .expect("secret should decrypt");

    assert_eq!(decrypted.expose_secret(), "sk-secret");
    assert_ne!(encrypted.ciphertext, b"sk-secret");
}
