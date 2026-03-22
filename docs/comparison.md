# ShadowMesh vs Alternatives

An honest comparison of ShadowMesh against other content delivery and hosting platforms. Every tool makes tradeoffs -- this page tries to lay them out plainly.

## Feature Comparison Table

| Feature | ShadowMesh | Vercel | Fleek | Pinata | 4EVERLAND |
|---|:---:|:---:|:---:|:---:|:---:|
| Decentralized hosting | Yes | No | Partial | No | Partial |
| Privacy / onion routing | Yes | No | No | No | No |
| Content-addressed (CID) | Yes | No | Yes | Yes | Yes |
| Censorship detection | Yes | No | No | No | No |
| .shadow naming (DHT) | Yes | No | No | No | No |
| ENS integration | Yes | No | Yes | No | Yes |
| WebRTC P2P delivery | Yes | No | No | No | No |
| Global edge CDN | No | Yes | No | No | Partial |
| Serverless functions | No | Yes | No | No | Yes |
| Polished UI / DX | Basic | Excellent | Good | Good | Good |
| Git-push deploys | Yes | Yes | Yes | No | Yes |
| Custom domains (DNS) | No (use .shadow / ENS) | Yes | Yes | N/A | Yes |
| Free tier | Self-host | Yes | Yes | Yes | Yes |
| Open source | Yes (MIT) | Partial | Partial | No | Partial |
| Encrypted storage | Yes (ChaCha20-Poly1305) | No | No | No | No |
| Content fragmentation | Yes | No | No | No | No |
| Rate limiting / bandwidth tracking | Built-in | Platform-managed | Platform-managed | Plan-based | Platform-managed |
| WASM browser SDK | Yes | N/A | No | No | No |

## Detailed Comparisons

### ShadowMesh vs Vercel

Vercel is the gold standard for frontend DX: `git push`, preview deploys, serverless functions, edge caching across dozens of PoPs. If you want the smoothest possible developer experience and do not care about decentralization, Vercel is hard to beat.

**Where ShadowMesh wins:**
- No corporate chokepoint. Vercel can (and does) take down deployments on request. ShadowMesh content is replicated across independent peers.
- Onion routing means no single node sees both who is requesting content and what the content is.
- Content is encrypted at rest and in transit between peers, not just over TLS to an edge server.
- No vendor lock-in. Your content is addressed by CID; any node can serve it.

**Where Vercel wins:**
- Far more polished UI, CLI, and documentation.
- Serverless functions, ISR, edge middleware -- ShadowMesh does static content only.
- Global edge network with sub-50ms latency worldwide. ShadowMesh latency depends on peer proximity.
- Custom domains with automatic TLS. ShadowMesh uses .shadow names and ENS instead of DNS.
- Mature ecosystem: integrations, analytics, team management.

**Bottom line:** Use Vercel when you need production-grade DX and do not have censorship or privacy requirements. Use ShadowMesh when you need content that cannot be taken down or surveilled.

### ShadowMesh vs Fleek

Fleek is the closest competitor in spirit -- both target decentralized web hosting. Fleek deploys to IPFS and offers ENS integration, a CLI, and a dashboard.

**Where ShadowMesh wins:**
- True privacy layer. Fleek deploys to IPFS through centralized infrastructure (their own build servers, Cloudflare IPFS gateway). ShadowMesh adds onion routing so no single hop sees the full picture.
- Censorship detection is built into the routing layer. If a path to a peer starts getting blocked, the adaptive router reroutes automatically.
- WebRTC P2P means browsers can fetch content directly from other browsers or nodes without touching a gateway.
- .shadow naming is fully DHT-native with no DNS dependency. Fleek still depends on DNS for its dashboard and build pipeline.

**Where Fleek wins:**
- More mature product with a larger user base.
- Better build pipeline: framework detection, environment variables, build caching.
- Managed IPFS pinning -- you do not have to run your own infrastructure.
- Better documentation and onboarding flow.

**Bottom line:** Fleek is decentralized at the storage layer but centralized at the build and delivery layers. ShadowMesh is decentralized end-to-end but less polished.

### ShadowMesh vs Pinata

Pinata is an IPFS pinning service. It stores your content on IPFS and gives you a gateway URL. It is not a hosting platform in the same way -- no builds, no deploys, just pin-and-serve.

**Where ShadowMesh wins:**
- Full CDN with routing, not just pinning. ShadowMesh handles content fragmentation, multi-path delivery, peer scoring, and adaptive routing.
- Privacy. Pinata knows exactly what you pin and who requests it. ShadowMesh's onion routing prevents that.
- No API key required to fetch content (public gateway or P2P).
- Encrypted at rest. Pinata stores content in the clear on IPFS.

**Where Pinata wins:**
- Simple, focused product. Pin a file, get a URL. No P2P networking to understand.
- Dedicated IPFS gateway with good uptime and CDN caching.
- Generous free tier (100 files, 1 GB).
- Well-documented REST API.
- Submarine feature (private IPFS pinning before making content public).

**Bottom line:** Pinata is a good choice for straightforward IPFS pinning. ShadowMesh is overkill if you just need to host some images on IPFS, but necessary if you need privacy or censorship resistance on top of content addressing.

### ShadowMesh vs 4EVERLAND

4EVERLAND positions itself as a Web3 cloud computing platform with IPFS hosting, Arweave integration, and gateway services. It bridges Web2 DX with Web3 storage.

**Where ShadowMesh wins:**
- Fully open source. 4EVERLAND's core infrastructure is proprietary.
- Privacy-first architecture. 4EVERLAND routes through their own centralized gateways.
- No token or account required to run a node or serve content.
- Censorship detection and adaptive routing -- 4EVERLAND has no equivalent.
- .shadow naming is independent of any blockchain or token.

**Where 4EVERLAND wins:**
- Multi-chain storage (IPFS, Arweave, BNB Greenfield). ShadowMesh uses its own content-addressed network.
- Better dashboard and DX for typical web hosting workflows.
- Managed hosting -- no need to run your own nodes.
- Serverless and compute capabilities beyond static hosting.

**Bottom line:** 4EVERLAND offers more features for general Web3 hosting. ShadowMesh is more specialized: if your priority is privacy and censorship resistance over convenience, ShadowMesh delivers that in ways 4EVERLAND does not.

## What ShadowMesh Does Not Do

Honesty about limitations:

- **No serverless functions.** ShadowMesh serves static content only. If you need server-side logic, you need a separate backend.
- **No managed hosting.** You either use the public gateway or run your own node. There is no "ShadowMesh Cloud" with a billing page.
- **Less polished DX.** The dashboard is functional but basic. The CLI works but lacks framework detection and build caching.
- **Smaller network.** Fewer nodes means fewer replicas and higher average latency compared to a global CDN.
- **No custom DNS domains.** You use .shadow names, ENS, or direct CID links. Traditional domains with A/CNAME records are not supported.

## What Makes ShadowMesh Unique

Features no competitor currently offers in combination:

1. **Onion routing for content delivery.** No single relay node knows both the requester's identity and the content being fetched.
2. **.shadow naming via DHT.** Name resolution works without DNS, without a blockchain, and without any centralized registry.
3. **Censorship detection built into routing.** The adaptive router tracks failure patterns (TCP RST, TLS failures, DNS poisoning) per path and automatically reroutes when a path is suspected blocked.
4. **WebRTC P2P with gateway fallback.** Browsers fetch content directly from peers when possible, falling back to the HTTP gateway transparently.
5. **Content fragmentation with encryption.** Files are split into encrypted chunks distributed across multiple peers. No single peer holds the complete content.
