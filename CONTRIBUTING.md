# Contributing to ShadowMesh

Thank you for your interest in contributing to ShadowMesh! This document provides guidelines and information for contributors.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
- [Development Setup](#development-setup)
- [Making Changes](#making-changes)
- [Pull Request Process](#pull-request-process)
- [Coding Standards](#coding-standards)
- [Testing](#testing)
- [Documentation](#documentation)
- [Community](#community)

## Code of Conduct

We are committed to providing a welcoming and inclusive experience for everyone. Please read and follow our Code of Conduct:

- **Be respectful** - Treat everyone with respect and consideration
- **Be constructive** - Provide helpful feedback and be open to receiving it
- **Be inclusive** - Welcome newcomers and help them learn
- **Be professional** - Focus on what is best for the community and project

## Getting Started

### Prerequisites

- **Rust** 1.70+ - Install via [rustup](https://rustup.rs/)
- **Node.js** 18+ - For SDK development
- **Git** - Version control
- **Docker** (optional) - For containerized development

### Fork and Clone

1. Fork the repository on GitHub
2. Clone your fork locally:

```bash
git clone https://github.com/YOUR-USERNAME/shadowmesh.git
cd shadowmesh
```

3. Add upstream remote:

```bash
git remote add upstream https://github.com/Antismart/shadowmesh.git
```

## Development Setup

### Building the Project

```bash
# Build all components
cargo build

# Build in release mode
cargo build --release

# Build specific component
cargo build -p protocol
cargo build -p gateway
cargo build -p node-runner
```

### Running Tests

```bash
# Run all tests
cargo test --workspace

# Run with output
cargo test --workspace -- --nocapture

# Run specific test
cargo test test_name

# Run benchmarks
cargo bench -p benchmarks
```

### SDK Development

```bash
cd sdk

# Install dependencies
npm install

# Build
npm run build

# Run tests
npm test

# Lint
npm run lint
```

### Running Locally

```bash
# Terminal 1: Gateway
cargo run -p gateway

# Terminal 2: Node Runner
cargo run -p node-runner

# Terminal 3: Test
curl http://localhost:3000/health
```

## Making Changes

### Branch Naming

Use descriptive branch names:

- `feature/` - New features (e.g., `feature/webrtc-transport`)
- `fix/` - Bug fixes (e.g., `fix/cache-expiration`)
- `docs/` - Documentation (e.g., `docs/api-reference`)
- `refactor/` - Code refactoring (e.g., `refactor/crypto-module`)
- `test/` - Test additions (e.g., `test/integration-tests`)

### Commit Messages

We follow [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <description>

[optional body]

[optional footer]
```

**Types:**
- `feat` - New feature
- `fix` - Bug fix
- `docs` - Documentation only
- `style` - Code style (formatting, etc.)
- `refactor` - Code refactoring
- `test` - Adding tests
- `chore` - Maintenance tasks
- `perf` - Performance improvements

**Examples:**
```
feat(protocol): add WebRTC transport support

Implements WebRTC data channels for browser-to-browser
communication without requiring a relay server.

Closes #123
```

```
fix(gateway): handle empty content uploads

Previously, uploading empty content would cause a panic.
Now returns a proper 400 Bad Request error.
```

### Keeping Your Branch Updated

```bash
# Fetch upstream changes
git fetch upstream

# Rebase your branch
git rebase upstream/main

# Or merge
git merge upstream/main
```

## Pull Request Process

### Before Submitting

1. **Update your branch** with the latest changes from `main`
2. **Run all tests** and ensure they pass
3. **Run linting** and fix any issues
4. **Update documentation** if needed
5. **Add tests** for new functionality

### Checklist

```markdown
- [ ] Code follows project style guidelines
- [ ] Tests added/updated and passing
- [ ] Documentation updated
- [ ] Commit messages follow convention
- [ ] Branch is up-to-date with main
- [ ] No merge conflicts
```

### Submitting

1. Push your branch to your fork
2. Create a Pull Request on GitHub
3. Fill out the PR template
4. Link any related issues
5. Request review from maintainers

### Review Process

- PRs require at least one approval
- Address reviewer feedback promptly
- Keep discussions constructive
- Squash commits if requested

## Coding Standards

### Rust

We follow the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/):

```rust
// Use rustfmt for formatting
cargo fmt

// Use clippy for linting
cargo clippy -- -D warnings
```

**Style Guidelines:**

```rust
// Use descriptive names
fn calculate_content_hash(content: &[u8]) -> String { ... }

// Document public APIs
/// Encrypts content using ChaCha20-Poly1305.
///
/// # Arguments
/// * `content` - The plaintext content to encrypt
///
/// # Returns
/// The encrypted data with embedded nonce
///
/// # Errors
/// Returns `CryptoError` if encryption fails
pub fn encrypt(&self, content: &[u8]) -> Result<EncryptedData, CryptoError> { ... }

// Use Result for fallible operations
pub fn process(&self) -> Result<Output, Error> { ... }

// Prefer &str over String for function parameters
pub fn lookup(&self, cid: &str) -> Option<Content> { ... }

// Use builder pattern for complex configurations
let config = ConfigBuilder::new()
    .with_cache_size(1024)
    .with_timeout(Duration::from_secs(30))
    .build();
```

### TypeScript

We use ESLint and Prettier:

```bash
# Format code
npm run format

# Lint
npm run lint

# Fix lint issues
npm run lint:fix
```

**Style Guidelines:**

```typescript
// Use TypeScript strict mode
// tsconfig.json: "strict": true

// Define interfaces for data structures
interface ContentMetadata {
  cid: string;
  size: number;
  createdAt: Date;
}

// Use async/await over raw promises
async function retrieveContent(cid: string): Promise<Uint8Array> {
  const response = await fetch(`/content/${cid}`);
  return new Uint8Array(await response.arrayBuffer());
}

// Handle errors properly
try {
  const content = await client.retrieve(cid);
} catch (error) {
  if (error instanceof ShadowMeshError) {
    // Handle specific error
  }
  throw error;
}

// Document with JSDoc
/**
 * Deploys content to the ShadowMesh network.
 * @param content - The content bytes to deploy
 * @param options - Optional deployment settings
 * @returns The deployment result including CID
 * @throws {ShadowMeshError} If deployment fails
 */
async deploy(content: Uint8Array, options?: DeployOptions): Promise<DeployResult>;
```

## Testing

### Rust Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // Unit test
    #[test]
    fn test_hash_content() {
        let content = b"test content";
        let hash = hash_content(content);
        assert!(!hash.is_empty());
        assert_eq!(hash.len(), 64);
    }

    // Async test
    #[tokio::test]
    async fn test_async_operation() {
        let result = async_function().await;
        assert!(result.is_ok());
    }

    // Test with setup
    #[test]
    fn test_with_fixture() {
        let fixture = create_test_fixture();
        let result = process(&fixture);
        assert_eq!(result, expected);
    }
}
```

### TypeScript Tests

```typescript
describe('ShadowMeshClient', () => {
  let client: ShadowMeshClient;

  beforeEach(() => {
    client = new ShadowMeshClient({
      gatewayUrl: 'http://localhost:3000',
    });
  });

  it('should deploy content', async () => {
    const content = new TextEncoder().encode('test');
    const result = await client.deploy(content);
    expect(result.cid).toBeDefined();
  });

  it('should handle errors', async () => {
    await expect(client.retrieve('invalid')).rejects.toThrow();
  });
});
```

### Integration Tests

```rust
// tests/integration_tests.rs

#[tokio::test]
async fn test_full_workflow() {
    // Setup
    let gateway = start_test_gateway().await;
    let client = create_test_client(&gateway);

    // Upload
    let content = b"test content";
    let cid = client.upload(content).await.unwrap();

    // Retrieve
    let retrieved = client.retrieve(&cid).await.unwrap();
    assert_eq!(retrieved, content);

    // Cleanup
    gateway.shutdown().await;
}
```

## Documentation

### Code Documentation

- Document all public APIs
- Include examples in doc comments
- Keep documentation up-to-date with code changes

### Updating Docs

Documentation lives in the `docs/` directory:

- `README.md` - Project overview
- `docs/architecture.md` - System architecture
- `docs/api-reference.md` - API documentation
- `docs/sdk-guide.md` - SDK usage guide
- `docs/deployment.md` - Deployment instructions
- `docs/protocol-spec.md` - Protocol specification

### Doc Comments

```rust
/// A brief description of what this does.
///
/// More detailed explanation if needed. Can span
/// multiple paragraphs.
///
/// # Examples
///
/// ```rust
/// let manager = CryptoManager::new();
/// let encrypted = manager.encrypt(b"hello").unwrap();
/// ```
///
/// # Panics
///
/// Panics if the key is invalid.
///
/// # Errors
///
/// Returns `CryptoError::EncryptionFailed` if encryption fails.
pub fn encrypt(&self, data: &[u8]) -> Result<EncryptedData, CryptoError> {
    // ...
}
```

## Community

### Getting Help

- **GitHub Issues** - Bug reports and feature requests
- **GitHub Discussions** - Questions and general discussion
- **Discord** - Real-time chat (coming soon)

### Recognition

Contributors are recognized in:

- `CONTRIBUTORS.md` file
- Release notes
- Project README

### Maintainers

Current maintainers:

- [@Antismart](https://github.com/Antismart) - Project Lead

To become a maintainer, demonstrate consistent quality contributions over time.

## License

By contributing to ShadowMesh, you agree that your contributions will be licensed under the MIT License.

---

Thank you for contributing to ShadowMesh! Your efforts help build a more private and decentralized web. 🌐
