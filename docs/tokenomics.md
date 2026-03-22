# ShadowMesh Tokenomics

> Canonical reference for the MESH token incentive layer.
> Last updated: 2026-03-22

---

## Table of Contents

1. [Overview](#overview)
2. [Token Details](#token-details)
3. [Distribution](#distribution)
4. [Reward Mechanics](#reward-mechanics)
5. [Anti-Gaming Measures](#anti-gaming-measures)
6. [Contract Architecture](#contract-architecture)
7. [Integration Points](#integration-points)
8. [Phased Roadmap](#phased-roadmap)
9. [FAQ](#faq)

---

## Overview

ShadowMesh is a decentralized content delivery and pinning network. Nodes donate bandwidth and storage to relay data for other participants. Without an incentive layer, this model depends entirely on altruism, which does not scale.

The MESH token exists to solve a concrete problem: **aligning individual node operator incentives with network-wide health.** Nodes that serve more data, stay online longer, and behave honestly earn more MESH. Nodes that freeload, game metrics, or disappear earn nothing.

This is not a speculative asset. During Phase 1 (Sepolia testnet), MESH tokens carry no monetary value. The purpose of the testnet phase is to validate the reward formulas, stress-test the anti-gaming mechanisms, and gather real operational data before any mainnet deployment.

The model described here will evolve. Parameters like epoch pool size, minimum thresholds, and cap percentages are tunable and will be adjusted based on empirical evidence from testnet operation.

---

## Token Details

| Property         | Value                          |
|------------------|--------------------------------|
| Name             | ShadowMesh Token               |
| Symbol           | MESH                           |
| Decimals         | 18                             |
| Total Supply Cap | 100,000,000 MESH               |
| Initial Mint     | 60,000,000 MESH                |
| Mintable         | Yes, by contract owner, up to cap |
| Chain (Phase 1)  | Sepolia testnet                |
| Chain (Phase 2)  | Mainnet (TBD)                  |
| Upgradeability   | None. Contracts are immutable for Phase 1. |

The initial mint of 60M MESH covers the rewards pool (50M) and the community/airdrop allocation (10M). The remaining 40M can be minted by the contract owner as needed, but the hard cap of 100M can never be exceeded. There is no inflation beyond the cap.

---

## Distribution

```
Total Supply: 100,000,000 MESH

  Rewards Pool .............. 50%   50,000,000 MESH
  Treasury .................. 20%   20,000,000 MESH
  Team ...................... 15%   15,000,000 MESH
  Community / Airdrops ...... 10%   10,000,000 MESH
  Initial Liquidity .........  5%    5,000,000 MESH
```

### Allocation Rationale

**Rewards Pool (50% -- 50,000,000 MESH)**
The largest allocation, by design. The network only works if node operators are compensated for the bandwidth and uptime they provide. At 500,000 MESH per weekly epoch, the rewards pool sustains 100 epochs (approximately 2 years) before requiring any additional minting. This gives the project a long runway to prove the model before touching the remaining mintable supply.

**Treasury (20% -- 20,000,000 MESH)**
Reserved for ecosystem grants, partnership incentives, protocol development funding, and liquidity provisioning. Treasury disbursements will be publicly documented. In later phases, a DAO will govern treasury spend.

**Team (15% -- 15,000,000 MESH)**
Allocated to the core contributors building and maintaining the protocol. Subject to a 12-month linear vesting schedule with a 3-month cliff. No team tokens are liquid at launch. This ensures the team is committed for at least a year and their incentives are aligned with long-term network health.

**Community / Airdrops (10% -- 10,000,000 MESH)**
Rewards for early adopters, testnet participants, and bug bounty hunters. Part of the initial mint so it is available immediately. This allocation recognizes the people who help battle-test the network before it reaches production.

**Initial Liquidity (5% -- 5,000,000 MESH)**
Set aside for DEX liquidity pools if and when the token reaches mainnet. Not used during Phase 1.

---

## Reward Mechanics

### Epoch Structure

- **Epoch duration:** 7 days (weekly, starting Monday 00:00 UTC)
- **Reward per epoch:** 500,000 MESH
- **Distribution method:** Pull-based claiming (nodes call `claimRewards()`)

### Eligibility Requirements

A node must meet **both** thresholds to qualify for rewards in a given epoch:

1. **Minimum bandwidth:** Serve at least 100 MB of data during the epoch.
2. **Minimum uptime:** Respond to at least 3 out of 7 daily liveness polls.

Nodes that fail either threshold receive zero rewards for that epoch. Their bandwidth does not count toward the total pool denominator.

### Reward Formula

Rewards are distributed using linear proportional allocation:

```
node_reward = (node_bytes / total_eligible_bytes) * epoch_pool
```

Where:
- `node_bytes` = bandwidth served by this node during the epoch (bytes)
- `total_eligible_bytes` = sum of bandwidth from all eligible nodes
- `epoch_pool` = 500,000 MESH

A per-node cap applies: no single node can earn more than **10% of the epoch pool** (50,000 MESH) in a single epoch. Excess is redistributed proportionally among remaining eligible nodes.

### Worked Example

Suppose three nodes operate during Epoch 12:

| Node   | Bandwidth Served | Uptime (polls) | Eligible? |
|--------|-----------------|----------------|-----------|
| Node A | 10 GB           | 7/7            | Yes       |
| Node B | 5 GB            | 5/7            | Yes       |
| Node C | 50 MB           | 2/7            | No (below both thresholds) |

Node C is excluded (below 100 MB minimum and below 3/7 uptime). Only Node A and Node B share the epoch pool.

**Total eligible bandwidth:** 10 GB + 5 GB = 15 GB

**Node A reward:**
```
(10 / 15) * 500,000 = 333,333.33 MESH
```

**Node B reward:**
```
(5 / 15) * 500,000 = 166,666.67 MESH
```

Neither node exceeds the 50,000 MESH cap (both are well below), so no redistribution is needed.

**Node C reward:** 0 MESH.

### Cap Redistribution Example

Now suppose Node X serves 900 GB and Node Y serves 100 GB in a total pool of 1,000 GB:

**Uncapped calculation:**
- Node X: `(900 / 1000) * 500,000 = 450,000 MESH` -- exceeds 50,000 cap
- Node Y: `(100 / 1000) * 500,000 = 50,000 MESH` -- at cap exactly

**After capping Node X at 50,000 MESH:**
- Excess from Node X: `450,000 - 50,000 = 400,000 MESH`
- Remaining eligible nodes share the excess proportionally.
- In practice with only two nodes, Node Y would absorb the excess, but Node Y is also subject to the 50,000 cap.
- Unclaimed MESH rolls back into the rewards pool for future epochs.

This cap prevents a single high-bandwidth node from draining the entire epoch allocation and ensures broader distribution across the network.

### Distribution Flow

1. Gateway collects bandwidth telemetry throughout the epoch.
2. At epoch close, an external reporter script reads the gateway's `/api/bandwidth/report` endpoint.
3. The reporter submits the bandwidth report on-chain via `RewardDistributor.submitReport()`.
4. Each eligible node calls `claimRewards()` to pull their allocation to their registered wallet.

---

## Anti-Gaming Measures

Running a permissionless reward system invites abuse. The following mechanisms are in place from Phase 1, with more planned for later phases.

### Phase 1 (Active)

| Mechanism | What It Prevents | How It Works |
|-----------|-----------------|--------------|
| **Minimum bandwidth (100 MB/week)** | Idle nodes registering to claim dust rewards | Nodes below threshold are excluded from the reward calculation entirely. |
| **Maximum reward cap (10% of pool)** | Single-node dominance, Sybil farms with traffic funneling | No node can earn more than 50,000 MESH per epoch regardless of bandwidth served. |
| **Minimum uptime (3/7 polls)** | Ghost nodes that serve traffic in bursts then disappear | Nodes must demonstrate consistent availability across the epoch. |
| **Admin flagging** | Detected cheaters and anomalous behavior | Admin can flag a node address; flagged nodes earn zero rewards until manually unflagged. All flags are visible on-chain. |

### Future Phases (Planned)

- **Proof of delivery:** Cryptographic receipts proving data was actually served to real clients, not self-generated traffic.
- **Random sampling:** Validator nodes issue random retrieval challenges; failure to respond correctly results in reward reduction.
- **Reputation decay:** Long-dormant nodes lose accumulated reputation and must re-earn eligibility gradually.
- **Minimum stake (Phase 2):** Nodes must lock MESH tokens to participate. Misbehavior results in slashing, creating a direct economic cost to cheating.

These measures are not exhaustive. Anti-gaming is an ongoing arms race, and the parameters above will be tuned based on observed behavior during testnet operation.

---

## Contract Architecture

```
+------------------+       +-------------------+       +---------------------+
|                  |       |                   |       |                     |
|    MeshToken     |       |   NodeRegistry    |       | RewardDistributor   |
|    (ERC-20)      |       |                   |       |                     |
+------------------+       +-------------------+       +---------------------+
| - name: MESH     |       | - peerId -> wallet|       | - epochPool: 500K   |
| - decimals: 18   |       | - wallet -> peerId|       | - epochDuration: 7d |
| - cap: 100M      |       | - isRegistered()  |       | - minBandwidth      |
| - mint(to, amt)  |       | - register(peer)  |       | - minUptime         |
| - Ownable        |       | - enumerable      |       | - maxCapPct: 10%    |
+--------+---------+       | - free on testnet |       | - submitReport()    |
         |                 +--------+----------+       | - claimRewards()    |
         |                          |                  | - Pausable          |
         |                          |                  +----------+----------+
         |                          |                             |
         +--------------------------+-----------------------------+
                                    |
                          +---------+---------+
                          |                   |
                          |   Relationships   |
                          |                   |
                          +-------------------+
                          | RewardDistributor |
                          |   reads from      |
                          |   NodeRegistry    |
                          |   to verify node  |
                          |   eligibility     |
                          |                   |
                          | RewardDistributor |
                          |   calls mint()    |
                          |   on MeshToken    |
                          |   (or transfers   |
                          |   from pre-minted |
                          |   pool)           |
                          +-------------------+

Deployment Chain: Sepolia (Phase 1) -> Mainnet TBD (Phase 2)
Tooling: Foundry (forge build / forge test / cast send)
Upgradeability: None. All contracts are immutable in Phase 1.
```

### Contract Descriptions

**MeshToken**
Standard ERC-20 with an owner-only `mint()` function and a hard cap of 100,000,000 tokens. Built on OpenZeppelin's ERC20 and Ownable. No proxy, no upgradeability. If a breaking change is needed, a new token contract is deployed and a migration path is provided.

**NodeRegistry**
Maps libp2p peer IDs to Ethereum wallet addresses. Registration is free on testnet (no gas sponsorship needed on Sepolia). The registry is enumerable so the RewardDistributor can iterate over registered nodes. A node must be registered before it can claim rewards.

**RewardDistributor**
Holds the rewards pool and manages epoch-based distribution. The admin submits a bandwidth report at the end of each epoch via `submitReport()`. The contract computes each node's allocation based on the proportional formula and stores claimable balances. Nodes pull their rewards via `claimRewards()`. The contract is pausable so distribution can be halted in an emergency.

---

## Integration Points

### Node Registration

```bash
smesh register --wallet 0xYourWalletAddress
```

The CLI writes the peer ID and wallet association to the NodeRegistry contract. This is a one-time operation per node.

### Bandwidth Reporting

The gateway exposes telemetry at:

```
GET /api/bandwidth/report?epoch={epoch_number}
```

An external reporter script (cron job or manual) reads this endpoint at the end of each epoch and submits the data on-chain:

- **Rust path:** Uses `alloy` to construct and send the `submitReport()` transaction.
- **TypeScript path:** Uses `viem` for the same purpose.

### Dashboard

The web dashboard integrates via `viem` + `wagmi`:

- Wallet connect for node operators
- Earnings page showing per-epoch reward history
- Claim button that invokes `claimRewards()` on the RewardDistributor
- Node registration status and uptime metrics

---

## Phased Roadmap

### Phase 1 -- Testnet (Current)

| Aspect | Detail |
|--------|--------|
| Chain | Sepolia |
| Token value | None. Testnet tokens only. |
| Registration | Free, no stake required |
| Distribution | Manual. Admin runs reporter script weekly. |
| Contracts | Immutable. No proxy pattern. |
| Goal | Validate reward formulas, test anti-gaming, gather real bandwidth data. |

### Phase 2 -- Mainnet Beta

| Aspect | Detail |
|--------|--------|
| Chain | Ethereum mainnet (or L2, decision pending data from Phase 1) |
| Stake-to-serve | Nodes must lock a minimum MESH stake to participate |
| Slashing | Light slashing for provable misbehavior (offline during claimed uptime, etc.) |
| Distribution | Automated. Reporter service runs on a schedule with on-chain verification. |
| Contracts | Potentially upgradeable via proxy if Phase 1 reveals the need. Decision made before mainnet deploy. |

### Phase 3 -- Full Economy

| Aspect | Detail |
|--------|--------|
| Pay-per-pin | Clients pay MESH to pin content; fees flow to serving nodes |
| Market pricing | Bandwidth and storage prices set by supply/demand, not fixed formula |
| DAO governance | Treasury spend, parameter changes, and contract upgrades governed by MESH holders |
| Reputation system | On-chain reputation scores influencing reward multipliers and routing priority |

There are no fixed dates for Phase 2 or Phase 3. Transitions happen when the data from the current phase justifies the added complexity.

---

## FAQ

**Q: Do MESH tokens have monetary value?**
No. During Phase 1, MESH exists only on Sepolia testnet and has no monetary value. Even after a potential mainnet launch, the token's purpose is to coordinate network incentives, not to function as a speculative asset.

**Q: What happens if I run a node but serve less than 100 MB in a week?**
You receive zero rewards for that epoch. The 100 MB minimum exists to filter out idle or misconfigured nodes. Your node remains registered and will earn normally in any epoch where it meets the threshold.

**Q: Can one person run multiple nodes?**
Yes, but each node is subject to the same eligibility requirements and the 10% per-node cap. Running 20 Sybil nodes that each serve trivial bandwidth is not profitable -- they each need to meet the 100 MB minimum independently, and the cap prevents funneling all traffic through a single node to maximize one reward.

**Q: What stops someone from generating fake traffic to inflate their bandwidth numbers?**
In Phase 1, the admin flagging mechanism is the primary defense. Anomalous traffic patterns (e.g., a node reporting 500 GB of bandwidth with no corresponding client requests) will result in flagging and zero rewards. In later phases, proof of delivery and random sampling will make self-dealing cryptographically difficult.

**Q: How long does the rewards pool last?**
At 500,000 MESH per epoch and 50,000,000 MESH in the initial pool, the rewards sustain 100 epochs (approximately 23 months). Additional MESH can be minted by the owner up to the 100M cap, extending the runway by up to another 100 epochs if the full remaining supply is allocated to rewards.

**Q: Why not use a proxy/upgradeable contract?**
Immutable contracts are simpler to audit, reason about, and trust. For a testnet deployment where the stakes are zero, the cost of redeploying a new contract is trivial. If Phase 1 reveals that upgradeability is necessary for mainnet, that decision will be made with real data backing it.

**Q: Why pull-based claiming instead of push-based distribution?**
Push-based distribution (the contract sends rewards to each node) has two problems: (1) gas costs scale linearly with the number of nodes, and (2) it can fail if a recipient is a contract that reverts on receive. Pull-based claiming puts the gas cost on the claimant and eliminates griefing vectors.

**Q: What if a node misses the claim window?**
There is no claim window. Unclaimed rewards remain available indefinitely. A node can claim rewards from any past epoch at any time.

**Q: Will the reward formula change?**
Almost certainly. The linear proportional model is a starting point. Real network data from Phase 1 will reveal whether the formula adequately incentivizes the behavior we want (geographic distribution, uptime consistency, serving unpopular content). Expect parameter adjustments and potentially a formula change before mainnet.

**Q: How are epoch bandwidth reports verified?**
In Phase 1, the admin is the trusted reporter. This is centralized and acknowledged as a limitation. Phase 2 introduces on-chain verification mechanisms and multiple independent reporters. Phase 3 targets fully decentralized verification via proof of delivery.

---

*This document describes the current design as of Phase 1 (Sepolia testnet). All parameters, formulas, and mechanisms are subject to change based on empirical data from network operation. The goal is to build an incentive system that works in practice, not one that looks good on paper.*
