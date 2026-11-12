# Chain Signatures: one real testnet signature

Status: a NEAR testnet account got a real MPC signature for a Base Sepolia
USDC transfer, and the signature recovers to the address the signer
derives for that account. I did not broadcast it: the derived address holds
no Base Sepolia ETH for gas and no test USDC. The policy wallet does not
call `sign` yet; the request came from the wallet's owner account.

Script: `demo/live/chainsig-proof.mjs`. Record:
[`docs/demo/testnet-chainsig/2026-11-12/EVIDENCE.json`](../demo/testnet-chainsig/2026-11-12/EVIDENCE.json).
Helpers: `packages/triagepay-sdk` (`erc20TransferCalldata`,
`eip1559SigningHash`, `ecdsaSignRequest`, `evmAddressFromNearKey`,
`normalizeMpcSignature`, `signedEip1559`), with unit tests that reuse the
recorded values.

## What happened on 2026-11-12

| Item | Value |
|---|---|
| Signer contract | `v1.signer-prod.testnet`, domain 0 (secp256k1) |
| Root key (`public_key`) | `secp256k1:4NfTiv3UsGahebgTaHyD9vF8KYKMBnfd6kh94mK6xv8fGBiJB8TBtFMP5WWXz6B89Ac1fbpzPwAvoyQebemHFwx3` |
| Predecessor and path | `tpd-e947e7-owner.testnet`, `base-sepolia-1` |
| Derived key (`derived_public_key`) | `secp256k1:4BktUHkTqEmDUtWneJsiC4LFjFGuWp5P1eMQ3VeUCMtMdsMvKnjtPueNVpeNYR8aRzpeSgTTf4KJuBQTNMzVimZV` |
| Local derivation check | root + sha3_256("near-mpc-recovery v0.1.0 epsilon derivation:" + predecessor + "," + path) x G equals the derived key |
| Derived EVM address | `0x508e984fda3bc37d8c33bfb5098441fd2169fcc8` |
| Unsigned transaction | EIP-1559, chain 84532 (Base Sepolia), nonce 0, gas limit 80,000, to USDC `0x036CbD53842c5426634e7929541eC2318f3dCF7e`, `transfer(0x...dEaD, 10000)` (0.01 USDC) |
| Payload signed | `f228b2c29745e87c425566d3c4bc2cf11d522c7ec3cd23a5bb3a912d39ef16a1` (keccak256 of 0x02 and the RLP) |
| NEAR `sign` transaction | [aeaL3TZA](https://testnet.nearblocks.io/txns/aeaL3TZAsYyV6kEGVCvm3ppzfc56cvP8pAsCVKrifVM), 1 yoctoNEAR, 300 TGas |
| Signature | r `11c3aa365f431bd51d6676bab903192d18e347aec626aa3c022d3687fe0af2e7`, s `011d446a5756ccdcd73f93ad277c2214eab6aab9d767844fafe161a8ecd7c361`, yParity 1 |
| Recovered address | `0x508e984fda3bc37d8c33bfb5098441fd2169fcc8`, equal to the derived address; `secp256k1.verify` against the derived key is true |
| Signed transaction hash | `0x21744b81fb83b61891185ab869d3a1c4bc8e0aaa8d690467ef7cf3f0b0c8f188` (not broadcast, so it is not on Base Sepolia) |

I cross-checked the unsigned hash, the sender, the raw signed transaction
and its hash with ethers v6. All four match the SDK helpers.

An earlier `sign` request from the same account
([48S6ju2J](https://testnet.nearblocks.io/txns/48S6ju2Jph8KXGABfJboYdz2FeGa3twXtug6c7hDJzV3))
also returned a signature. My script crashed before it recorded it, so I
ran the request again with fresh fees. That first signature is in the NEAR
receipt and is not used.

The `sign` arguments:

```json
{
  "request": {
    "path": "base-sepolia-1",
    "payload_v2": { "Ecdsa": "<64 hex chars, the 32-byte hash>" },
    "domain_id": 0
  }
}
```

## To broadcast

Send a little Base Sepolia ETH and test USDC to
`0x508e984fda3bc37d8c33bfb5098441fd2169fcc8`, then run
`node demo/live/chainsig-proof.mjs` again. It reads the nonce and fees, asks
for a fresh signature, and broadcasts when the address has a balance.

## What the policy wallet would do

The agent key would still only call `pay` (or a sibling such as
`pay_evm`) on the wallet. The wallet would:

1. Run the same checks as `pay`: not paused, agent active, token allowed,
   payee allowed or queued for review, per-call limit, daily limit.
2. Reserve the amount.
3. Call `v1.signer-prod.testnet.sign` with the hash and the path, so the
   predecessor (and the derived address) is the wallet itself.
4. On the callback, keep the reservation if a signature came back, and
   release it if the request timed out.

The signature is public in the NEAR receipt. Anyone who also has the
unsigned transaction can broadcast it. There is no revoke. Treat "signed"
as "spent". The same secp256k1 key controls the derived address on every
EVM chain, so the path is a label, not a chain lock.

The sandbox cannot run the MPC network, so a contract method that calls
`sign` cannot be tested there. I left it out of the wasm rather than ship a
method that I have not run.
