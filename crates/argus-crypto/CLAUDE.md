# Argus-Crypto

> 特性：AES-256-GCM 加密工具，支持主密钥管理。

## 模块结构

```
src/
├── lib.rs           # 公共 API 导出
├── cipher.rs       # Cipher、EncryptedSecret
├── key_source.rs   # KeyMaterialSource、FileKeySource
└── error.rs        # CryptoError
```

## 核心概念

### 1. Cipher

**Cipher** 提供 AES-256-GCM 加密：

```rust
pub struct Cipher {
    key: [u8; 32],  // 256-bit key
}

pub struct EncryptedSecret {
    nonce: [u8; 12],     // 96-bit nonce
    ciphertext: Vec<u8>,  // 密文
    tag: [u8; 16],       // 认证标签
}
```

### 2. KeyMaterialSource

**KeyMaterialSource** trait 支持多种密钥来源：

```rust
pub trait KeyMaterialSource: Send + Sync {
    fn get_key(&self) -> Result<[u8; 32], CryptoError>;
}
```

**内置实现**：
- `StaticKeySource`：静态密钥
- `FileKeySource`：从文件读取密钥

## 公共 API

```rust
use argus_crypto::{Cipher, EncryptedSecret, FileKeySource};

// 创建 Cipher
let key_source = FileKeySource::new("master.key")?;
let cipher = Cipher::new(key_source)?;

// 加密
let encrypted = cipher.encrypt(plaintext)?;

// 解密
let plaintext = cipher.decrypt(&encrypted)?;
```

## 依赖关系

### 上游依赖
- `argus-protocol`：SecretString 类型

### 下游消费者
- `argus-auth`：凭证加密
- `argus-llm`：API key 加密

## 设计原则

### 1. AEAD 加密
- 使用 AES-256-GCM
- 提供认证加密

### 2. 密钥管理
- 支持多种密钥来源
- 主密钥自管理
