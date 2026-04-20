# Argus-Crypto

> 特性：凭证加解密与密钥来源抽象，供账号、provider 和持久化层复用。

## 作用域

- 本文件适用于 `crates/argus-crypto/` 及其子目录。

## 核心职责

- `Cipher` 负责加解密 secret
- `EncryptedSecret` 表达持久化后的密文载荷
- `KeyMaterialSource` 抽象主密钥来源，支持文件或静态字节

## 关键模块

- `src/cipher.rs`：`Cipher`、`EncryptedSecret`
- `src/key_source.rs`：`KeyMaterialSource`、`FileKeySource`、`StaticKeySource`
- `src/error.rs`：`CryptoError`

## 公开入口

- `Cipher`
- `EncryptedSecret`
- `KeyMaterialSource`
- `FileKeySource`、`StaticKeySource`

## 依赖边界

- 上游依赖：`argus-protocol` 的 `SecretString`
- 下游消费者：`argus-auth`、`argus-repository`、`argus-llm`

## 修改守则

- 保持密钥来源与业务逻辑解耦；不要在这里感知账号或 provider 语义
- 兼顾现有读密钥回退场景，避免破坏已持久化 secret 的解密能力
- 所有密文结构变更都要同步检查 repository 读写路径
