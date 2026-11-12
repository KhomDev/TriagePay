# Chain Signatures: design and partial proof of concept

Status: partial. I did not request a signature and I did not broadcast a
transaction on Base Sepolia. The bytes below are what a later call would
send. The package that builds them is `packages/triagepay-sdk`
(`ecdsaSignRequest`, `erc20TransferCalldata`), covered by unit tests.

## What I checked on testnet

On 2026-11-12, `v1.signer-prod.testnet` answered `public_key` for domain 0:

`secp256k1:4NfTiv3UsGahebgTaHyD9vF8KYKMBnfd6kh94mK6xv8fGBiJB8TBtFMP5WWXz6B89Ac1fbpzPwAvoyQebemHFwx3`

That is the root key. A child key is derived from the predecessor account
and a path. I did not derive the Base address. Mainnet's signer is
`v1.signer`. Testnet's is `v1.signer-prod.testnet`.

The current `sign` arguments, from the NEAR chain-signatures contract, are:

```json
{
  "request": {
    "path": "base-sepolia-1",
    "payload_v2": { "Ecdsa": "<64 hex chars, the 32-byte hash>" },
    "domain_id": 0
  }
}
```

Deposit is 1 yoctoNEAR. Gas on the order of 300 TGas leaves room for the
MPC callback. The contract signs the 32 bytes it is given. For an EIP-1559
transaction those bytes are `keccak256` of the RLP-encoded unsigned
transaction, not the RLP itself.

## What the policy wallet would do

The agent key would still only call `pay` (or a sibling such as
`pay_evm`) on the wallet. The wallet would:

1. Run the same checks as `pay`: not paused, agent active, token allowed,
   payee allowed or queued for review, per-call limit, daily limit.
2. Reserve the amount.
3. Call `v1.signer-prod.testnet.sign` with the hash and the path.
4. On the callback, keep the reservation if a signature came back, and
   release it if the request timed out (`Request has timed out` after the
   yield window).

The signature is public in the NEAR receipt. Anyone who also has the
unsigned transaction can broadcast it. There is no revoke. Treat "signed"
as "spent". One path per chain (`base-sepolia-1`) keeps the keys apart.
The same secp256k1 key controls that address on every EVM chain, so the
path is a label, not a chain lock.

Circle publishes USDC on Base Sepolia at
`0x036CbD53842c5426634e7929541eC2318f3dCF7e`. The calldata helper builds
`transfer(address,uint256)` (selector `a9059cbb`) and stops there. It does
not build the EIP-1559 RLP, so it does not produce the hash the signer
must see.

## What is left

- Derive the EVM address from the root key, the wallet account, and the path.
- Build the unsigned EIP-1559 transaction (nonce, fees, chain id 84532) and hash it.
- Fund that address with Base Sepolia ETH for gas, and with test USDC.
- Submit `sign` from an account that can attach 1 yoctoNEAR. The wallet
  account has no key, so the call has to start inside the contract.
- Broadcast the signed transaction and record both the NEAR receipt and the
  Base transaction.

The sandbox cannot run the MPC network, so a contract method that calls
`sign` cannot be proven here. I left it out of the wasm rather than ship a
method that looks live and cannot be completed.
